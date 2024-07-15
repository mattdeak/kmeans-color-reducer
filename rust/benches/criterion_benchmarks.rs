#[cfg(not(target_arch = "wasm32"))]
use criterion::{black_box, criterion_group, criterion_main, Criterion};

use colorcrunch::{
    kmeans::{KMeans, KMeansAlgorithm},
    types::Vec3,
};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

#[cfg(not(target_arch = "wasm32"))]
fn generate_random_pixels(count: usize, seed: u64) -> Vec<Vec3> {
    let mut rng = StdRng::seed_from_u64(seed ^ (count as u64));
    (0..count)
        .map(|_| {
            [
                rng.gen::<f32>() * 255.0,
                rng.gen::<f32>() * 255.0,
                rng.gen::<f32>() * 255.0,
            ]
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn benchmark_kmeans_comparison(c: &mut Criterion) {
    let k_values = [2, 4, 8, 16];
    let data_sizes = [1000, 10000, 100000, 500000];
    let seed = 42; // Fixed seed for reproducibility

    for &size in &data_sizes {
        let data = generate_random_pixels(size, seed);

        for &k in &k_values {
            let mut group = c.benchmark_group(format!("kmeans_size_{}_k_{}", size, k));

            group.bench_function("Hamerly", |b| {
                b.iter(|| {
                    black_box(
                        KMeans::new(black_box(k))
                            .with_algorithm(KMeansAlgorithm::Hamerly)
                            .run(black_box(&data)),
                    )
                })
            });

            group.bench_function("Lloyd", |b| {
                b.iter(|| {
                    black_box(
                        KMeans::new(black_box(k))
                            .with_algorithm(KMeansAlgorithm::Lloyd)
                            .run(black_box(&data)),
                    )
                })
            });

            group.finish();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn benchmark_euclidean_distance(c: &mut Criterion) {
    use colorcrunch::kmeans;

    let mut rng = rand::thread_rng();
    let a: Vec3 = [rng.gen(), rng.gen(), rng.gen()];
    let b: Vec3 = [rng.gen(), rng.gen(), rng.gen()];
    let mut group = c.benchmark_group("euclidean_distance");

    group.bench_function("euclidean_distance_arr", |bencher| {
        bencher.iter(|| kmeans::distance::euclidean_distance_squared(black_box(&a), black_box(&b)))
    });

    group.finish();
}

#[cfg(not(target_arch = "wasm32"))]
fn benchmark_find_closest_centroid(c: &mut Criterion) {
    use colorcrunch::kmeans;
    let mut rng = rand::thread_rng();
    let pixel = [rng.gen(), rng.gen(), rng.gen()];
    let centroids: Vec<Vec3> = (0..100)
        .map(|_| [rng.gen(), rng.gen(), rng.gen()])
        .collect();

    c.bench_function("find_closest_centroid", |bencher| {
        bencher.iter(|| kmeans::find_closest_centroid(black_box(&pixel), black_box(&centroids)))
    });
}

#[cfg(not(target_arch = "wasm32"))]
criterion_group!(
    benches,
    benchmark_kmeans_comparison,
    benchmark_euclidean_distance,
    benchmark_find_closest_centroid
);

#[cfg(not(target_arch = "wasm32"))]
criterion_main!(benches);

#[cfg(target_arch = "wasm32")]
pub fn main() {
    println!("This benchmark is only supported on non-wasm32 targets.");
}
