use crate::kmeans::distance::euclidean_distance_squared;
use crate::types::ColorVec;
use rand::seq::SliceRandom;
use rand::Rng;
use rand::SeedableRng;

// Return the index of closest centroid and distance to that centroid
pub fn find_closest_centroid(pixel: &ColorVec, centroids: &[ColorVec]) -> usize {
    debug_assert!(centroids.len() > 0);
    let mut min_distance = euclidean_distance_squared(pixel, &centroids[0]);
    let mut min_index = 0;
    for (i, centroid) in centroids.iter().enumerate() {
        let distance = euclidean_distance_squared(pixel, centroid);
        if distance < min_distance {
            min_distance = distance;
            min_index = i;
        }
    }
    min_index
}

pub fn has_converged(
    initial_centroids: &[ColorVec],
    final_centroids: &[ColorVec],
    tolerance: f32,
) -> bool {
    initial_centroids
        .iter()
        .zip(final_centroids.iter())
        .all(|(a, b)| euclidean_distance_squared(a, b) < (tolerance * tolerance))
}

#[allow(dead_code)]
pub fn calculate_max_centroid_movement(
    initial_centroids: &[ColorVec],
    final_centroids: &[ColorVec],
) -> f32 {
    initial_centroids
        .iter()
        .zip(final_centroids.iter())
        .map(|(a, b)| euclidean_distance_squared(&a, &b))
        .reduce(f32::max)
        .unwrap_or(0.0)
}

#[allow(dead_code)]
pub fn calculate_min_centroid_distance(centroids: &[ColorVec]) -> f32 {
    centroids
        .iter()
        .enumerate()
        .flat_map(|(i, &centroid_a)| {
            centroids[i + 1..]
                .iter()
                .map(move |&centroid_b| euclidean_distance_squared(&centroid_a, &centroid_b))
        })
        .fold(f32::MAX, f32::min)
}

// Ok we're using the K-Means++ initialization
// I think this is right? Seems to work
pub fn initialize_centroids(data: &[ColorVec], k: usize, seed: Option<u64>) -> Vec<ColorVec> {
    let mut centroids = Vec::with_capacity(k);

    // Seed the RNG if provided, otherwise use the current time
    let mut rng = {
        if let Some(seed) = seed {
            rand::rngs::StdRng::seed_from_u64(seed)
        } else {
            rand::rngs::StdRng::from_entropy()
        }
    };

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
                    .map(|centroid| euclidean_distance_squared(pixel, centroid))
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

#[cfg(test)]
mod tests {
    use statrs::assert_almost_eq;

    use super::*;

    #[test]
    fn test_find_closest_centroid() {
        let pixel = [100.0, 100.0, 100.0];
        let centroids = vec![
            [0.0, 0.0, 0.0],
            [100.0, 100.0, 100.0],
            [200.0, 200.0, 200.0],
        ];

        let closest_index = find_closest_centroid(&pixel, &centroids);
        assert_eq!(closest_index, 1);
    }

    #[test]
    fn test_calculate_max_centroid_movement() {
        let initial_centroids = vec![[0.0, 0.0, 0.0], [100.0, 100.0, 100.0]];
        let final_centroids = vec![[10.0, 10.0, 10.0], [90.0, 90.0, 90.0]];

        let max_movement = calculate_max_centroid_movement(&initial_centroids, &final_centroids);
        assert!((max_movement - 300.0).abs() < 0.00001); // sqrt(300) ≈ 17.32051
    }

    #[test]
    fn test_calculate_min_centroid_distance() {
        let centroids = vec![
            [0.0, 0.0, 0.0],
            [100.0, 100.0, 100.0],
            [200.0, 200.0, 200.0],
        ];

        let min_distance = calculate_min_centroid_distance(&centroids) as f64;
        assert_almost_eq!(min_distance, 30000.0, 0.0001); // sqrt(30000) ≈ 173.2051
    }
}
