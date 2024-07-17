use super::buffers::MappableBuffer;
use crate::kmeans::types::KMeansResult;
use crate::kmeans::utils::has_converged;
use crate::kmeans::KMeansConfig;
use crate::types::{Vec4, Vec4u};
use futures::executor::block_on;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, Buffer, BufferBindingType, BufferDescriptor, BufferUsages,
    CommandEncoderDescriptor, ComputePassDescriptor, ComputePipeline, ComputePipelineDescriptor,
    Device, DeviceDescriptor, PipelineCompilationOptions, PipelineLayout, PipelineLayoutDescriptor,
    Queue, RequestAdapterOptions, ShaderModuleDescriptor, ShaderSource, ShaderStages,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
struct CentroidInfo {
    x: u32,
    y: u32,
    z: u32,
    count: u32,
}

type Centroids = Vec<Vec4>;

struct ProcessBuffers {
    pixel_buffer: Buffer,
    centroid_buffer: Buffer,
    assignment_buffer: MappableBuffer,
    centroid_info_buffer: MappableBuffer,
    bind_group: BindGroup,
}

#[derive(Debug)]
pub struct LloydAssignmentsAndCentroidInfo {
    device: Device,
    queue: Queue,
    compute_pipeline: ComputePipeline,
    bind_group_layout: BindGroupLayout,
    pipeline_layout: PipelineLayout,
    config: KMeansConfig,
}

impl LloydAssignmentsAndCentroidInfo {
    // useful because the initialization function is big
    // and we don't want to recompile the shader
    // every time we change the number of clusters
    pub fn set_k(&mut self, k: usize) {
        self.config.k = k;
    }

    fn make_bind_group_layout(device: &Device) -> BindGroupLayout {
        let entries = [
            // Pixel Group
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // Centroids
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // Assignments
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // Centroid Info
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ]
        .to_vec();

        device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("kmeans_bind_group_layout".into()),
            entries: &entries,
        })
    }

    pub async fn from_config(config: KMeansConfig) -> Self {
        let instance = wgpu::Instance::default();

        let adapter = instance
            .request_adapter(&RequestAdapterOptions::default())
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(&DeviceDescriptor::default(), None)
            .await
            .unwrap();

        let bind_group_layout = Self::make_bind_group_layout(&device);

        let shader_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("kmeans_shader".into()),
            source: ShaderSource::Wgsl(include_str!("lloyd_gpu4.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("kmeans_pipeline_layout".into()),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let compute_pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("kmeans_compute_pipeline".into()),
            layout: Some(&pipeline_layout),
            module: &shader_module,
            entry_point: "main",
            compilation_options: PipelineCompilationOptions::default(),
        });

        Self {
            device,
            queue,
            compute_pipeline,
            bind_group_layout,
            pipeline_layout,
            config,
        }
    }

    fn prepare_buffers(
        &self,
        pixels: &[Vec4u],
        centroids: &[Vec4],
        assignments: &[u32],
    ) -> Result<ProcessBuffers, &'static str> {
        // Pixel Buffer (in shader these are vec4<u32>)
        let pixel_buffer = self.device.create_buffer(&BufferDescriptor {
            label: None,
            size: std::mem::size_of_val(pixels) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.queue
            .write_buffer(&pixel_buffer, 0, bytemuck::cast_slice(pixels));

        // Centroid Buffer (in shader these are vec3)
        let centroid_buffer = self.device.create_buffer(&BufferDescriptor {
            label: None,
            // 3 floats per centroid, 4 bytes per float (as they are f32), but we have to align
            // to 16 bytes to match the alignment of the pixel buffer
            size: std::mem::size_of_val(centroids) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        self.queue
            .write_buffer(&centroid_buffer, 0, bytemuck::cast_slice(centroids));

        let assignment_size: u64 = (pixels.len() * std::mem::size_of::<u32>()) as u64;
        let assignment_buffer = self.device.create_buffer(&BufferDescriptor {
            label: None,
            // 1 int per assignment
            // technically I think we can get away with a u8 here
            // since our color space is limited to 256 colors
            // but alignment. we'll maybe do this later
            size: assignment_size,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let assignment_staging_buffer = self.device.create_buffer(&BufferDescriptor {
            label: None,
            size: assignment_size,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        // this should just be zeros but that's fine
        self.queue
            .write_buffer(&assignment_buffer, 0, bytemuck::cast_slice(assignments));

        let centroid_info_buffer = self.device.create_buffer(&BufferDescriptor {
            label: None,
            size: (self.config.k * std::mem::size_of::<CentroidInfo>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let centroid_info_staging_buffer = self.device.create_buffer(&BufferDescriptor {
            label: None,
            size: (self.config.k * std::mem::size_of::<CentroidInfo>()) as u64,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let buffers = vec![
            BindGroupEntry {
                binding: 0,
                resource: pixel_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: centroid_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: assignment_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 3,
                resource: centroid_info_buffer.as_entire_binding(),
            },
        ];

        let bind_group = self.device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &self.bind_group_layout,
            entries: &buffers,
        });

        Ok(ProcessBuffers {
            pixel_buffer,
            centroid_buffer,
            assignment_buffer: MappableBuffer {
                gpu_buffer: assignment_buffer,
                staging_buffer: assignment_staging_buffer,
                size: assignment_size,
            },
            centroid_info_buffer: MappableBuffer {
                gpu_buffer: centroid_info_buffer,
                staging_buffer: centroid_info_staging_buffer,
                size: (self.config.k * std::mem::size_of::<CentroidInfo>()) as u64,
            },
            bind_group,
        })
    }

    pub fn run(&self, pixels: &[Vec4u]) -> KMeansResult<Vec4> {
        block_on(self.run_async(pixels))
    }

    pub async fn run_async(&self, pixels: &[Vec4u]) -> KMeansResult<Vec4> {
        let vec4_pixels: Vec<Vec4> = pixels
            .iter()
            .map(|v| [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32])
            .collect();
        let mut centroids: Vec<Vec4> = self.config.initializer.initialize_centroids(
            &vec4_pixels,
            self.config.k,
            self.config.seed,
        );

        let assignments: Vec<u32> = vec![0; pixels.len()];

        let process_buffers = self
            .prepare_buffers(pixels, &centroids, &assignments)
            .unwrap();

        let mut iterations = 0;

        while iterations < self.config.max_iterations {
            let new_centroids = self.run_iteration(&process_buffers, pixels.len()).await?;

            if has_converged(&centroids, &new_centroids, self.config.tolerance) {
                centroids = new_centroids;
                break;
            }

            self.queue.write_buffer(
                &process_buffers.centroid_buffer,
                0,
                bytemuck::cast_slice(&new_centroids),
            );
            centroids = new_centroids;

            iterations += 1;
        }

        // Read back final assignments
        let assignments = self.read_assignments(&process_buffers).await?;

        Ok((assignments, centroids))
    }

    async fn run_iteration(
        &self,
        process_buffers: &ProcessBuffers,
        pixel_count: usize,
    ) -> Result<Centroids, &'static str> {
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor { label: None });

        // This should probably be a variable we can configure
        // but it requires templating the shader, which I don't want to do yet.
        let num_workgroups = ((pixel_count as u32 + 63) / 64) as u32;

        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });

            pass.set_pipeline(&self.compute_pipeline);
            pass.set_bind_group(0, &process_buffers.bind_group, &[]);
            pass.insert_debug_marker("kmeans_iteration");
            pass.dispatch_workgroups(num_workgroups, 1, 1);
        }

        process_buffers
            .centroid_info_buffer
            .copy_to_staging_buffer(&mut encoder);
        self.queue.submit(Some(encoder.finish()));

        let centroid_info = process_buffers
            .centroid_info_buffer
            .read_back(&self.device)
            .await?;
        Ok(self.process_centroid_info(&centroid_info))
    }

    fn process_centroid_info(&self, centroid_info: &[CentroidInfo]) -> Vec<Vec4> {
        let mut centroids: Vec<Vec4> = vec![];
        for centroid in centroid_info {
            centroids.push([
                centroid.x as f32 / centroid.count as f32,
                centroid.y as f32 / centroid.count as f32,
                centroid.z as f32 / centroid.count as f32,
                0.0,
            ]);
        }
        centroids
    }

    async fn read_assignments(
        &self,
        process_buffers: &ProcessBuffers,
    ) -> Result<Vec<usize>, &'static str> {
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor { label: None });

        process_buffers
            .assignment_buffer
            .copy_to_staging_buffer(&mut encoder);
        self.queue.submit(Some(encoder.finish()));
        let assignments: Vec<u32> = process_buffers
            .assignment_buffer
            .read_back(&self.device)
            .await?;
        Ok(assignments.into_iter().map(|a| a as usize).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kmeans::gpu::GpuAlgorithm;
    use crate::kmeans::initializer::Initializer;
    use futures::executor::block_on;
    use rand::prelude::*;
    use rand::thread_rng;

    fn create_test_config() -> KMeansConfig {
        KMeansConfig {
            k: 3,
            max_iterations: 10,
            tolerance: 0.001,
            algorithm: GpuAlgorithm::LloydAssignmentsAndCentroidInfo.into(),
            initializer: Initializer::Random,
            seed: Some(42),
        }
    }

    #[test]
    fn test_kmeans_gpu_basic() {
        let config = create_test_config();
        let pixels: Vec<Vec4u> = vec![
            [0, 0, 0, 0],
            [1, 1, 1, 1],
            [2, 2, 2, 2],
            [10, 10, 10, 10],
            [11, 11, 11, 11],
            [12, 12, 12, 12],
            [20, 20, 20, 20],
            [21, 21, 21, 21],
            [22, 22, 22, 22],
        ];

        let kmeans = block_on(LloydAssignmentsAndCentroidInfo::from_config(config));
        let (assignments, centroids) = block_on(kmeans.run_async(&pixels)).unwrap();

        assert_eq!(assignments.len(), pixels.len());

        // Check that pixels close to each other are in the same cluster
        assert_eq!(assignments[0], assignments[1]);
        assert_eq!(assignments[1], assignments[2]);
        assert_eq!(assignments[3], assignments[4]);
        assert_eq!(assignments[4], assignments[5]);
        assert_eq!(assignments[6], assignments[7]);
        assert_eq!(assignments[7], assignments[8]);

        // Check that pixels far from each other are in different clusters
        assert_ne!(assignments[0], assignments[3]);
        assert_ne!(assignments[3], assignments[6]);
        assert_ne!(assignments[0], assignments[6]);
    }

    #[test]
    fn test_kmeans_gpu_convergence() {
        let config = KMeansConfig {
            k: 2,
            max_iterations: 100,
            tolerance: 0.001,
            algorithm: GpuAlgorithm::LloydAssignmentsAndCentroidInfo.into(),
            initializer: Initializer::Random,
            seed: Some(42),
        };

        let pixels: Vec<Vec4u> = vec![
            [0, 0, 0, 0],
            [1, 1, 1, 1],
            [2, 2, 2, 2],
            [10, 10, 10, 10],
            [11, 11, 11, 11],
            [12, 12, 12, 12],
        ];

        let kmeans = block_on(LloydAssignmentsAndCentroidInfo::from_config(config));
        let (assignments, centroids) = block_on(kmeans.run_async(&pixels)).unwrap();

        assert_eq!(assignments.len(), pixels.len());

        // Check that the algorithm converged to two clusters
        assert_eq!(assignments[0], assignments[1]);
        assert_eq!(assignments[1], assignments[2]);
        assert_eq!(assignments[3], assignments[4]);
        assert_eq!(assignments[4], assignments[5]);
        assert_ne!(assignments[0], assignments[3]);
    }

    #[test]
    fn test_kmeans_gpu_empty_input() {
        let config = create_test_config();
        let pixels: Vec<Vec4u> = vec![];

        let kmeans = block_on(LloydAssignmentsAndCentroidInfo::from_config(config));
        let (assignments, centroids) = block_on(kmeans.run_async(&pixels)).unwrap();

        assert_eq!(assignments.len(), 0);
        assert_eq!(centroids.len(), 0);
    }

    #[test]
    fn test_big_and_varied_input() {
        let mut config = create_test_config();
        config.k = 15;

        let kmeans = block_on(LloydAssignmentsAndCentroidInfo::from_config(config.clone()));
        let mut rng = thread_rng();

        let image_size = 2000;
        let pixels: Vec<Vec4u> = (0..image_size * image_size)
            .map(|_| {
                [
                    (rng.gen::<f32>() * 255.) as u32,
                    (rng.gen::<f32>() * 255.) as u32,
                    (rng.gen::<f32>() * 255.) as u32,
                    0,
                ]
            })
            .collect();

        let (assignments, centroids) = block_on(kmeans.run_async(&pixels)).unwrap();

        assert_eq!(assignments.len(), pixels.len());
        assert_eq!(centroids.len(), config.k);
    }
}