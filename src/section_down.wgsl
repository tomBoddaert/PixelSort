const FLAG: u32 = ~0u;

override workgroup_size: u32;

struct Immediates {
    width: u32,
    block_size: u32,
    threshold: f32,
}
var<immediate> immediates: Immediates;

@group(0) @binding(0)
var<storage, read> image: array<u32>;
@group(0) @binding(1)
var<storage, read_write> tagged_image: array<vec2<u32>>;

@group(0) @binding(2)
var<storage, read> left_workgroup: array<u32>;
@group(0) @binding(3)
var<storage, read> workgroup_right: array<u32>;

var<workgroup> left_block: array<atomic<u32>, workgroup_size>;
var<workgroup> block_right: array<u32, workgroup_size>;

@compute @workgroup_size(workgroup_size)
fn section_down(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let image_y_offset = immediates.width * global_id.y;
    let block_base = immediates.block_size * global_id.x;
    let block_top = min(block_base + immediates.block_size, immediates.width);
    let _active = block_base < immediates.width;

    var right = FLAG;
    var left_block_local: u32;
    var neighbour_right: u32;

    if (_active) {
        if (local_id.x == 0 && workgroup_id.x != 0) {
            let workgroup_offset_y = num_workgroups.x * workgroup_id.y;
            let left_workgroup_local = left_workgroup[workgroup_offset_y + workgroup_id.x - 1];
            neighbour_right = workgroup_right[workgroup_offset_y + left_workgroup_local];
        }

        if (block_base == 0u) {
            right = 0u;
        } else if (local_id.x == 0) {
            right = neighbour_right;
        }

        var i = block_top - 1u;
        var previous = source_thresholded(image_y_offset + i);

        loop {
            if (i < max(block_base, 1u)) {
                break;
            }
            i--;

            let current = source_thresholded(image_y_offset + i);
            if (previous != current) {
                right = i + 1;
                break;
            }
        }

        block_right[local_id.x] = right;

        left_block_local = local_id.x;
        if (right == FLAG) {
            // when left_block_local == 0, right defaults to the workgroup's established left value,
            // so left_block_local != 0 due to above condition
            left_block_local--;
        }
        atomicStore(&left_block[local_id.x], left_block_local);
    }

    workgroupBarrier();

    if (_active) {
        loop {
            let previous = left_block_local;
            left_block_local = atomicLoad(&left_block[left_block_local]);
            if (left_block_local == previous) {
                break;
            }

            atomicStore(&left_block[local_id.x], left_block_local);
        }
    }

    workgroupBarrier();

    if (_active) {
        var left = neighbour_right;
        if (local_id.x > 0) {
            left = block_right[atomicLoad(&left_block[local_id.x - 1])];
        }

        var previous: bool;
        if (block_base > 0u) {
            previous = source_thresholded(image_y_offset + block_base - 1);
        } else {
            previous = source_thresholded(image_y_offset + block_base);
        }
        for (var i = block_base; i < block_top; i++) {
            let pixel = image[image_y_offset + i];
            let value = get_value(pixel);
            let current = value > immediates.threshold;
            if (previous != current) {
                left = i;
            }
            tagged_image[image_y_offset + i] = vec2<u32>(
                // pack4x8unorm places value i at bits 8×i to 8×i+7
                (left << 16) | pack4x8unorm(vec4<f32>(value, 0, 0, 0)),
                pixel,
            );

            previous = current;
        }
    }
}

fn get_value(rgb: u32) -> f32 {
    let sqrt = sqrt(unpack4x8unorm(rgb));
    let sqrt_mean = (sqrt.x + sqrt.y + sqrt.z) / 3;
    return sqrt_mean * sqrt_mean;
}
fn get_thresholded(rgb: u32) -> bool {
    return get_value(rgb) > immediates.threshold;
}
fn source_thresholded(index: u32) -> bool {
    return get_thresholded(image[index]);
}
