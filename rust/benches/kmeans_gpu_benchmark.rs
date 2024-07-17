use colorcrunch::kmeans::gpu::{GpuAlgorithm, KMeansGpu};
use colorcrunch::kmeans::{Initializer, KMeansConfig};
use colorcrunch::types::Vec4u;
use criterion::async_executor::FuturesExecutor;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use futures::executor::block_on;
use rand::prelude::*;

fn benchmark_kmeans_gpu(c: &mut Criterion) {
    let algorithms = vec![
        GpuAlgorithm::LloydAssignmentsAndCentroids,
        GpuAlgorithm::LloydAssignmentsOnly,
        GpuAlgorithm::LloydAssignmentsAndCentroidInfo,
    ];

    let mut rng = thread_rng();
    let image_size = 2000;
    let pixels: Vec<Vec4u> = (0..image_size * image_size)
        .map(|_| {
            [
                (rng.gen::<f32>() * 255.0) as u32,
                (rng.gen::<f32>() * 255.0) as u32,
                (rng.gen::<f32>() * 255.0) as u32,
                (rng.gen::<f32>() * 255.0) as u32,
            ]
        })
        .collect();

    let mut group = c.benchmark_group("kmeans_gpu");

    for algorithm in algorithms {
        let config = KMeansConfig {
            k: 10,
            max_iterations: 10,
            tolerance: 0.001,
            algorithm: algorithm.into(),
            initializer: Initializer::Random,
            seed: Some(42),
        };
        let kmeans = block_on(KMeansGpu::new(config));

        group.bench_with_input(BenchmarkId::from_parameter(format!("{:?}", algorithm)), &kmeans, |b, kmeans| {
            b.to_async(FuturesExecutor)
                .iter_with_large_drop(|| async { kmeans.run_async(black_box(&pixels)).await.expect("Error running kmeans"); });
        });
    }

    group.finish();
}

criterion_group!(benches, benchmark_kmeans_gpu);
criterion_main!(benches);