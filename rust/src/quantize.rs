use crate::kmeans::find_closest_centroid;
use crate::kmeans::Initializer;
use crate::kmeans::KMeans;
use crate::kmeans::KMeansAlgorithm;
use crate::kmeans::KMeansConfig;
use crate::types::Vec4u;
use crate::utils::num_distinct_colors_u32;

#[derive(Debug)]
pub struct ColorCruncher {
    kmeans: KMeans,
    max_colors: usize,
    pub sample_rate: usize,
    pub channels: usize,
}

#[derive(Clone, Debug, Default)]
pub struct ColorCruncherBuilder {
    pub max_colors: Option<usize>,
    pub channels: Option<usize>,
    pub sample_rate: Option<usize>,
    pub tolerance: Option<f32>,
    pub max_iterations: Option<usize>,
    pub initializer: Option<Initializer>,
    pub algorithm: Option<KMeansAlgorithm>,
    pub seed: Option<u64>,
}

impl ColorCruncherBuilder {
    pub fn new() -> Self {
        Self::default()
    }


    pub fn with_max_colors(mut self, max_colors: usize) -> Self {
        self.max_colors = Some(max_colors);
        self
    }

    pub fn with_channels(mut self, channels: usize) -> Self {
        self.channels = Some(channels);
        self
    }

    pub fn with_sample_rate(mut self, sample_rate: usize) -> Self {
        self.sample_rate = Some(sample_rate);
        self
    }

    pub fn with_tolerance(mut self, tolerance: f32) -> Self {
        self.tolerance = Some(tolerance);
        self
    }

    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = Some(max_iterations);
        self
    }

    pub fn with_initializer(mut self, initializer: Initializer) -> Self {
        self.initializer = Some(initializer);
        self
    }

    pub fn with_algorithm(mut self, algorithm: KMeansAlgorithm) -> Self {
        self.algorithm = Some(algorithm);
        self
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    pub async fn build(&self) -> ColorCruncher {
        let kmeans_config = self.build_config();
        let kmeans = KMeans::new(kmeans_config.clone()).await;

        ColorCruncher {
            kmeans,
            max_colors: kmeans_config.k,
            sample_rate: self.sample_rate.unwrap_or(1),
            channels: self.channels.unwrap_or(3),
        }
    }

    fn build_config(&self) -> KMeansConfig {
        let default_config = KMeansConfig::default();
        let mut config = KMeansConfig::default();
        config.k = self.max_colors.unwrap_or_else(|| default_config.k);
        config.max_iterations = self
            .max_iterations
            .unwrap_or_else(|| default_config.max_iterations);
        config.tolerance = self.tolerance.unwrap_or_else(|| default_config.tolerance);
        config.algorithm = self
            .algorithm
            .clone()
            .unwrap_or_else(|| default_config.algorithm);
        config.initializer = self
            .initializer
            .clone()
            .unwrap_or_else(|| default_config.initializer);
        config.seed = self.seed;
        config
    }
}

impl ColorCruncher {
    fn chunk_pixels_vec4u(&self, pixels: &[u8]) -> Vec<Vec4u> {
        pixels
            .chunks_exact(self.channels)
            .step_by(self.sample_rate)
            .map(|chunk| {
                [
                    chunk[0] as u32,
                    chunk[1] as u32,
                    chunk[2] as u32,
                    chunk[3] as u32,
                ]
            })
            .collect()
    }

    pub async fn quantize_image(&self, pixels: &[u8]) -> Vec<u8> {
        let image_data = self.chunk_pixels_vec4u(pixels);

        // If there's already less than or equal to the max number of colors, return the original pixels
        if num_distinct_colors_u32(&image_data) <= self.max_colors {
            return pixels.to_vec();
        }

        let (_, centroids) = self.kmeans.run_async(&image_data).await.unwrap();

        let mut new_image = Vec::with_capacity(pixels.len());
        for pixel in pixels.chunks_exact(self.channels) {
            let px_vec = [
                pixel[0] as f32,
                pixel[1] as f32,
                pixel[2] as f32,
                pixel[3] as f32,
            ];
            let closest_centroid = find_closest_centroid(&px_vec, &centroids);
            let new_color = &centroids[closest_centroid];

            if self.channels == 3 {
                new_image.extend_from_slice(&[
                    new_color[0] as u8,
                    new_color[1] as u8,
                    new_color[2] as u8,
                ]);
            } else {
                new_image.extend_from_slice(&[
                    new_color[0] as u8,
                    new_color[1] as u8,
                    new_color[2] as u8,
                    pixel[3],
                ]);
            }
        }

        new_image
    }

    pub async fn create_palette(&self, pixels: &[u8]) -> Vec<[u8; 3]> {
        let image_data = self.chunk_pixels_vec4u(pixels);

        // If there's already less than or equal to the max number of colors, return the original pixels
        if num_distinct_colors_u32(&image_data) < self.max_colors {
            // todo
            todo!()
        }

        let (_, centroids) = self.kmeans.run_async(&image_data).await.unwrap();
        centroids
            .iter()
            .map(|color| [color[0] as u8, color[1] as u8, color[2] as u8])
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::*;

    #[test]
    fn test_reduce_colorspace() {
        let data = vec![
            255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255,
        ];
        let max_colors = 2;
        let sample_rate = 1;
        let channels = 4;

        let quantizer = block_on(ColorCruncherBuilder::default()
            .with_max_colors(max_colors)
            .with_sample_rate(sample_rate)
            .with_channels(channels)
            .build());

        let result = block_on(quantizer.quantize_image(&data));
        assert_eq!(result.len(), data.len());
    }
}
