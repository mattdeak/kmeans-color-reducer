@group(0) @binding(0) var<storage, read> image: array<vec3<u32>>;
@group(0) @binding(1) var<storage, read_write> centers: array<vec3<f32>>;
@group(0) @binding(2) var<storage, read_write> assignments: array<u32>;
@group(0) @binding(3) var<storage, read_write> global_assignment_counts: array<atomic<u32>>;
@group(0) @binding(4) var<storage, read_write> global_assignment_sums: array<array<atomic<u32>, 3>>;


const MAX_CLUSTERS: u32 = 64;

var<workgroup> assignment_counts: array<atomic<u32>, MAX_CLUSTERS>;
var<workgroup> assignment_sums: array<array<atomic<u32>, 3>, MAX_CLUSTERS>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>, @builtin(local_invocation_id) local_id: vec3<u32>) {
    var num_clusters = arrayLength(&centers);
    var num_pixels = arrayLength(&image);
    var idx = global_id.x;


    if (idx < num_pixels) {
        let pixel = image[idx];
        var min_dist = distance(vec3<f32>(pixel), centers[0]);
        var min_center = 0u;

        for (var i = 1u; i < arrayLength(&centers); i++) {
            let dist = distance(vec3<f32>(pixel), centers[i]);
            if (dist < min_dist) {
                min_dist = dist;
                min_center = i;
            }
        }

        assignments[idx] = min_center;
        atomicAdd(&assignment_counts[min_center], 1u);
        atomicAdd(&assignment_sums[min_center][0], pixel.x);
        atomicAdd(&assignment_sums[min_center][1], pixel.y);
        atomicAdd(&assignment_sums[min_center][2], pixel.z);
    }

    workgroupBarrier();

    if local_id.x < num_clusters {
        atomicAdd(&global_assignment_counts[local_id.x], assignment_counts[local_id.x]);
        atomicAdd(&global_assignment_sums[local_id.x][0], assignment_sums[local_id.x][0]);
        atomicAdd(&global_assignment_sums[local_id.x][1], assignment_sums[local_id.x][1]);
        atomicAdd(&global_assignment_sums[local_id.x][2], assignment_sums[local_id.x][2]);
    }

    workgroupBarrier();

    if global_id.x < num_clusters {
        let count = f32(atomicLoad(&global_assignment_counts[global_id.x]));
        if count > 0.0 {
            let x = f32(atomicLoad(&global_assignment_sums[global_id.x][0])) / count;
            let y = f32(atomicLoad(&global_assignment_sums[global_id.x][1])) / count;
            let z = f32(atomicLoad(&global_assignment_sums[global_id.x][2])) / count;
            centers[global_id.x] = vec3<f32>(x, y, z);
        }
    }
}


fn distance(a: vec3<f32>, b: vec3<f32>) -> f32 {
    let diff = a - b;
    return dot(diff, diff);
}