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
var<storage, read_write> left_workgroup: array<u32>;
@group(0) @binding(2)
var<storage, read_write> workgroup_right: array<u32>;

var<workgroup> left_block: array<atomic<u32>, workgroup_size>;
var<workgroup> block_right: array<u32, workgroup_size>;

@compute @workgroup_size(workgroup_size)
fn section_up(
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

    if (_active) {
        if (block_base == 0u) {
            right = 0u;
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
            if (left_block_local == 0) {
                left_block_local = FLAG;
            } else {
                left_block_local--;
            }
        }
        atomicStore(&left_block[local_id.x], left_block_local);
    }

    workgroupBarrier();

    if (_active) {
        loop {
            if (left_block_local == FLAG) {
                break;
            }

            let previous = left_block_local;
            left_block_local = atomicLoad(&left_block[left_block_local]);
            if (left_block_local == previous) {
                break;
            }

            atomicStore(&left_block[local_id.x], left_block_local);
        }
    }

    let workgroup_top = min(
        immediates.block_size * workgroup_size * (workgroup_id.x + 1),
        immediates.width
    );
    if (!_active || block_top != workgroup_top) {
        return;
    }

    let workgroup_offset_y = num_workgroups.x * workgroup_id.y;

    var workgroup_right_local = FLAG;
    var left_workgroup_local = workgroup_id.x;
    if (left_block_local != FLAG) {
        workgroup_right_local = block_right[left_block_local];
    } else {
        // for workgroup_id.x == 0, left_block_local != FLAG after pointer jumping
        left_workgroup_local--;
    }
    workgroup_right[workgroup_offset_y + workgroup_id.x] = workgroup_right_local;
    left_workgroup[workgroup_offset_y + workgroup_id.x] = left_workgroup_local;
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
