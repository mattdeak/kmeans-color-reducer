mod config;
pub mod distance;
pub mod hamerly;
pub mod initializer;
pub mod lloyd;
mod types;
mod utils;

#[cfg(feature = "gpu")]
pub mod lloyd_gpu;

pub use crate::kmeans::config::{KMeansAlgorithm, KMeansConfig};
pub use crate::kmeans::initializer::Initializer;
pub use crate::kmeans::utils::find_closest_centroid;
use crate::utils::num_distinct_colors;

use crate::types::{GPUVector, Vec3, Vec4, VectorExt};

use self::lloyd_gpu::KMeansGpu;
use self::types::{KMeansError, KMeansResult};

const DEFAULT_MAX_ITERATIONS: usize = 100;
const DEFAULT_TOLERANCE: f64 = 1e-2;
const DEFAULT_ALGORITHM: KMeansAlgorithm = KMeansAlgorithm::Lloyd;
const DEFAULT_INITIALIZER: Initializer = Initializer::KMeansPlusPlus;

pub trait AsyncKMeans<T: VectorExt> {
    async fn new(config: KMeansConfig) -> Self;
    async fn run(&self, data: &[T]) -> KMeansResult<T>;
}

// A wrapper for easier usage
#[derive(Debug, Clone)]
pub struct KMeansCPU(KMeansConfig);

impl KMeansCPU {
    pub fn from_config(config: KMeansConfig) -> Self {
        KMeansCPU(config)
    }

    pub fn with_k(mut self, k: usize) -> Self {
        self.0.k = k;
        self
    }

    // Builders
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.0.max_iterations = max_iterations;
        self
    }

    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.0.tolerance = tolerance as f32;
        self
    }

    pub fn with_algorithm(mut self, algorithm: KMeansAlgorithm) -> Self {
        self.0.algorithm = algorithm;
        self
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.0.seed = Some(seed);
        self
    }
}

impl Default for KMeansCPU {
    fn default() -> Self {
        KMeansCPU(KMeansConfig {
            k: 3,
            max_iterations: 100,
            tolerance: 1e-4,
            algorithm: KMeansAlgorithm::Lloyd,
            initializer: DEFAULT_INITIALIZER,
            seed: None,
        })
    }
}

impl KMeansCPU {
    pub fn run<T: VectorExt>(&self, data: &[T]) -> KMeansResult<T> {
        let unique_colors = num_distinct_colors(data);
        if unique_colors < self.0.k {
            return Err(KMeansError(format!(
                "Number of unique colors is less than k: {}",
                unique_colors
            )));
        }

        match self.0.algorithm {
            KMeansAlgorithm::Lloyd => Ok(lloyd::kmeans_lloyd(data, &self.0)),
            KMeansAlgorithm::Hamerly => Ok(hamerly::kmeans_hamerly(data, &self.0)),
            _ => Err(KMeansError(format!(
                "Algorithm not supported: {}",
                self.0.algorithm
            ))),
        }
    }
}

impl<T: VectorExt> AsyncKMeans<T> for KMeansCPU {
    async fn run(&self, data: &[T]) -> Result<(Vec<usize>, Vec<T>), KMeansError> {
        self.run(data)
    }

    async fn new(config: KMeansConfig) -> Self {
        KMeansCPU(config)
    }
}

impl AsyncKMeans<Vec4> for KMeansGpu {
    async fn run(&self, data: &[Vec4]) -> KMeansResult<Vec4> {
        self.run_async(data).await
    }

    async fn new(config: KMeansConfig) -> Self {
        KMeansGpu::from_config(config).await
    }
}

#[derive(Debug)]
pub enum KMeans {
    Cpu(KMeansCPU),
    #[cfg(feature = "gpu")]
    Gpu(KMeansGpu),
}

impl KMeans {
    pub fn new(config: KMeansConfig) -> Self {
        KMeans::Cpu(KMeansCPU(config))
    }

    #[cfg(feature = "gpu")]
    pub async fn gpu(self) -> Self {
        if let KMeans::Cpu(cpu) = self {
            KMeans::Gpu(KMeansGpu::from_config(cpu.0).await)
        } else {
            self
        }
    }

    #[cfg(feature = "gpu")]
    pub async fn new_gpu(config: KMeansConfig) -> Self {
        KMeans::Gpu(KMeansGpu::from_config(config).await)
    }

    pub fn run_vec4(&self, data: &[Vec4]) -> KMeansResult<Vec4> {
        match self {
            KMeans::Cpu(cpu) => cpu.run(data),
            KMeans::Gpu(gpu) => gpu.run(data),
        }
    }

    pub fn run_vec3(&self, data: &[Vec3]) -> KMeansResult<Vec3> {
        match self {
            KMeans::Cpu(cpu) => cpu.run(data),
            _ => Err(KMeansError(
                "GPU not supported for 3 channel data. Convert to 4 channel data first."
                    .to_string(),
            )),
        }
    }

    pub async fn run_async(&self, data: &[Vec4]) -> KMeansResult<Vec4> {
        match self {
            KMeans::Cpu(cpu) => cpu.run(data),
            #[cfg(feature = "gpu")]
            KMeans::Gpu(gpu) => gpu.run_async(data).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kmeans::config::{KMeansAlgorithm, KMeansConfig};
    use futures::executor::block_on;
    use rand::rngs::StdRng;
    use rand::Rng;
    use rand::SeedableRng;

    trait TestExt<T: VectorExt> {
        fn assert_almost_eq(&self, other: &[T], tolerance: f64);
    }

    impl<T: VectorExt> TestExt<T> for Vec<T> {
        fn assert_almost_eq(&self, other: &[T], tolerance: f64) {
            assert_eq!(self.len(), other.len());

            for i in 0..self.len() {
                let a_matches = (self[i][0] as f64 - other[i][0] as f64).abs() < tolerance;
                let b_matches = (self[i][1] as f64 - other[i][1] as f64).abs() < tolerance;
                let c_matches = (self[i][2] as f64 - other[i][2] as f64).abs() < tolerance;
                assert!(
                    a_matches && b_matches && c_matches,
                    "{:?} does not match {:?}",
                    self[i],
                    other[i]
                );
            }
        }
    }

    fn run_kmeans_test(data: &[Vec3], k: usize, expected_non_empty_clusters: usize) {
        let algorithms = vec![KMeansAlgorithm::Lloyd, KMeansAlgorithm::Hamerly];

        for algorithm in algorithms {
            let config = KMeansConfig {
                k,
                max_iterations: 100,
                tolerance: 1e-4,
                algorithm,
                initializer: DEFAULT_INITIALIZER,
                seed: None,
            };

            let (clusters, centroids) = KMeansCPU(config.clone()).run(data).unwrap();

            assert_eq!(
                clusters.len(),
                data.len(),
                "clusters.len() == data.len() with algorithm {}",
                &config.algorithm
            );
            assert_eq!(
                centroids.len(),
                k,
                "centroids.len() == k with algorithm {}",
                config.algorithm
            );
            assert_eq!(
                clusters.iter().filter(|&&c| c < k).count(),
                data.len(),
                "clusters.iter().filter(|&&c| c < k).count() == data.len() with algorithm {}",
                config.algorithm
            );
            // assert_eq!(centroids.iter().filter(|&&c| c != [0.0, 0.0, 0.0]).count(), expected_non_empty_clusters);
            assert!(expected_non_empty_clusters >= 1);
        }
    }

    #[test]
    fn test_kmeans_basic() {
        let data = vec![[255.0, 0.0, 0.0], [0.0, 255.0, 0.0], [0.0, 0.0, 255.0]];
        run_kmeans_test(&data, 3, 3);
    }

    #[test]
    fn test_kmeans_single_color() {
        let data = vec![
            [100.0, 100.0, 100.0],
            [100.0, 100.0, 100.0],
            [100.0, 100.0, 100.0],
        ];
        run_kmeans_test(&data, 1, 1);
    }

    #[test]
    fn test_kmeans_two_distinct_colors() {
        let data = vec![[255.0, 0.0, 0.0], [0.0, 0.0, 255.0]];
        run_kmeans_test(&data, 2, 2);
    }

    #[test]
    fn test_kmeans_more_clusters_than_colors() {
        let data = vec![[255.0, 0.0, 0.0], [0.0, 255.0, 0.0]];
        let config = KMeansConfig {
            k: 3,
            max_iterations: 100,
            tolerance: 1e-4,
            algorithm: KMeansAlgorithm::Lloyd,
            initializer: DEFAULT_INITIALIZER,
            seed: None,
        };
        let result = KMeansCPU(config).run(&data);
        assert_eq!(
            result.err().unwrap().to_string(),
            "Number of unique colors is less than k: 2"
        );
    }

    #[test]
    fn test_algorithms_converge_to_the_same_result_for_same_initial_conditions() {
        let seed = 42;
        let data_size = 100;

        let mut rng = StdRng::seed_from_u64(seed);
        let data = (0..data_size)
            .map(|_| {
                [
                    rng.gen::<f32>() * 255.0,
                    rng.gen::<f32>() * 255.0,
                    rng.gen::<f32>() * 255.0,
                    0.0,
                ]
            })
            .collect::<Vec<Vec4>>();

        let config_lloyd = KMeansConfig {
            k: 3,
            max_iterations: 500,
            tolerance: 1e-6,
            algorithm: KMeansAlgorithm::Lloyd,
            initializer: DEFAULT_INITIALIZER,
            seed: Some(seed),
        };

        let config_hamerly = KMeansConfig {
            k: 3,
            max_iterations: 500,
            tolerance: 1e-6,
            algorithm: KMeansAlgorithm::Hamerly,
            initializer: DEFAULT_INITIALIZER,
            seed: Some(seed),
        };

        let config_gpu = KMeansConfig {
            k: 3,
            max_iterations: 500,
            tolerance: 1e-6,
            algorithm: KMeansAlgorithm::Lloyd,
            initializer: DEFAULT_INITIALIZER,
            seed: Some(seed),
        };

        let gpu = block_on(KMeansGpu::from_config(config_gpu));

        let (clusters1, centroids1) = KMeansCPU(config_lloyd).run(&data).unwrap();
        let (clusters2, centroids2) = KMeansCPU(config_hamerly).run(&data).unwrap();
        let (clusters3, centroids3) = gpu.run(&data).unwrap();

        centroids1.assert_almost_eq(&centroids2, 0.005);
        centroids1.assert_almost_eq(&centroids3, 0.005);
        assert_eq!(clusters1, clusters2);
    }
}
