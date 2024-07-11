#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(feature = "python")]
pub mod python;

use std::collections::HashSet;
use rand::Rng;
use rand::seq::SliceRandom;

pub mod utils;
pub mod types;

use utils::{check_convergence, find_closest_centroid};
use types::ColorVec;


const MAX_ITERATIONS: usize = 300;
const TOLERANCE: f32 = 0.02;


fn num_distinct_colors(data: &[ColorVec]) -> usize {
    let mut color_hashset = HashSet::new();
    for pixel in data {
        // hacky but its fine, it only occurs once at the beginning
        let hash_key = pixel[0] as u8 * 2 + pixel[1] as u8 * 3 + pixel[2] as u8 * 5;
        color_hashset.insert(hash_key);
    }
    color_hashset.len()
}

// Ok we're using the K-Means++ initialization
// I think this is right? Seems to work
pub fn initialize_centroids(data: &[ColorVec], k: usize) -> Vec<ColorVec> {
    let mut centroids = Vec::with_capacity(k);
    let mut rng = rand::thread_rng();

    // Choose the first centroid randomly
    if let Some(first_centroid) = data.choose(&mut rng) {
        centroids.push(first_centroid.clone());
    } else {
        return centroids; 
    }

    // K-Means++
    while centroids.len() < k {
        let distances: Vec<f32> = data
            .iter()
            .map(|pixel| {
                centroids
                    .iter()
                    .map(|centroid| utils::euclidean_distance(pixel, centroid))
                    .min_by(|a, b| a.partial_cmp(b).unwrap())
                    .unwrap()
            })
            .collect();

        let total_distance: f32 = distances.iter().sum();
        let threshold = rng.gen::<f32>() * total_distance;

        let mut cumulative_distance = 0.0;
        for (i, distance) in distances.iter().enumerate() {
            cumulative_distance += distance;
            if cumulative_distance >= threshold {
                let pixel = &data[i];
                centroids.push(pixel.clone());
                break;
            }
        }
    }

    centroids
}

/// A k-means optimized for 3 channel color images
pub fn kmeans_3chan(data: &[ColorVec], k: usize) -> (Vec<Vec<usize>>, Vec<ColorVec>) {
    let mut centroids = initialize_centroids(data, k);
    let mut new_centroids: Vec<ColorVec> = centroids.clone();

    let mut clusters = vec![Vec::new(); k];
    let mut assignments = vec![0; data.len()];

    // Define the convergence criterion percentage (e.g., 2%)
    let mut iterations = 0;
    let mut converged = false;
    while iterations < MAX_ITERATIONS && !converged {
        // Assign points to clusters
        for (i, pixel) in data.iter().enumerate() {
            let closest_centroid = find_closest_centroid(pixel, &centroids);
            if assignments[i] != closest_centroid {
                assignments[i] = closest_centroid;
            }
        }

        clusters.iter_mut().for_each(|cluster| cluster.clear());
        assignments.iter().enumerate().for_each(|(i, &cluster)| {
            clusters[cluster].push(i);
        });

        // Update centroids and check for convergence
        clusters.iter().zip(new_centroids.iter_mut()).for_each(|(cluster, new_centroid)| {
            if cluster.is_empty() {
                return; // centroid can't move if there are no points
            }

            let mut sum_r = 0.0;
            let mut sum_g = 0.0;
            let mut sum_b = 0.0;
            let num_pixels = cluster.len() as f32;


            for &idx in cluster {
                let pixel = &data[idx];
                sum_r += pixel[0];
                sum_g += pixel[1];
                sum_b += pixel[2];
            }

            *new_centroid = [sum_r / num_pixels, sum_g / num_pixels, sum_b / num_pixels];
        });
        converged = check_convergence(&centroids, &new_centroids, TOLERANCE);
        // Swap the centroids and new_centroid. We'll update the new centroids again before
        // we check for convergence.
        std::mem::swap(&mut centroids, &mut new_centroids);
        iterations += 1;
    }
    
    (clusters, centroids)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen, target_feature(enable = "simd128"))]
pub fn reduce_colorspace(
    pixels: &[u8],
    max_colors: usize,
    sample_rate: usize, // 1 = no sampling, 2 = sample every 2 pixels, 3 = sample every 3 pixels, etc
    channels: usize // 3 = RGB, 4 = RGBA
) -> Vec<u8> {
    let image_data: Vec<ColorVec> = pixels
        .chunks_exact(channels)
        .step_by(sample_rate)
        .map(|chunk| [chunk[0] as f32, chunk[1] as f32, chunk[2] as f32])
        .collect();

    if num_distinct_colors(&image_data) <= max_colors {
        return pixels.to_vec();
    }

    let (_, centroids) = kmeans_3chan(&image_data, max_colors);

    let mut new_image = Vec::with_capacity(pixels.len());
    for pixel in pixels.chunks_exact(channels) {
        let px_vec = [pixel[0] as f32, pixel[1] as f32, pixel[2] as f32];
        let closest_centroid = find_closest_centroid(&px_vec, &centroids);
        let new_color = &centroids[closest_centroid];

        if channels == 3 {
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
                pixel[3]
        ]);
            }
    }

    new_image
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;


    #[wasm_bindgen_test]
    fn test_reduce_colorspace() {
        let data = vec![
            255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255
        ];
        let max_colors = 2;
        let sample_rate = 1;
        let channels = 4;

        let result = reduce_colorspace(&data, max_colors, sample_rate, channels);
        assert_eq!(result.len(), data.len());
    }


    #[test]
    fn test_kmeans_basic() {
        let data = vec![
            [255.0, 0.0, 0.0],
            [0.0, 255.0, 0.0],
            [0.0, 0.0, 255.0],
        ];
        let k = 3;
        let (clusters, centroids) = kmeans_3chan(&data, k);

        assert_eq!(clusters.len(), k);
        assert_eq!(centroids.len(), k);
        assert_eq!(clusters.iter().map(|c| c.len()).sum::<usize>(), 3);
    }

    #[test]
    fn test_kmeans_single_color() {
        let data = vec![
            [100.0, 100.0, 100.0],
            [100.0, 100.0, 100.0],
            [100.0, 100.0, 100.0],
        ];
        let k = 2;
        let (clusters, centroids) = kmeans_3chan(&data, k);

        assert_eq!(clusters.len(), k);
        assert_eq!(centroids.len(), k);
        assert_eq!(clusters.iter().filter(|c| !c.is_empty()).count(), 1);
    }

    #[test]
    fn test_kmeans_two_distinct_colors() {
        let data = vec![
            [255.0, 0.0, 0.0],
            [0.0, 0.0, 255.0],
        ];
        let k = 2;
        let (clusters, centroids) = kmeans_3chan(&data, k);

        assert_eq!(clusters.len(), k);
        assert_eq!(centroids.len(), k);
    }

    #[test]
    fn test_kmeans_more_clusters_than_colors() {
        let data = vec![
            [255.0, 0.0, 0.0],
            [0.0, 255.0, 0.0],
        ];
        let k = 3;
        let (clusters, centroids) = kmeans_3chan(&data, k);

        assert_eq!(clusters.len(), k);
        assert_eq!(centroids.len(), k);
        assert_eq!(clusters.iter().filter(|c| !c.is_empty()).count(), 2);
    }
}