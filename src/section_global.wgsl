const FLAG = ~0u;

override workgroup_size: u32;

struct Immediates {
    workgroups: u32,
    block_size: u32,
}
var<immediate> immediates: Immediates;

@group(0) @binding(0)
var<storage, read_write> left_workgroup: array<u32>;

var<workgroup> left_block: array<atomic<u32>, workgroup_size>;
var<workgroup> block_right: array<u32, workgroup_size>;

@compute @workgroup_size(workgroup_size)
fn section_global(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let y_offset = immediates.workgroups * global_id.y;
    let block_base = immediates.block_size * global_id.x;
    let block_top = min(block_base + immediates.block_size, immediates.workgroups);
    let _active = block_base < immediates.workgroups;

    var right = FLAG;
    var left_block_local: u32;

    if (_active) {
        if (block_base == 0) {
            right = 0;
        }

        var i = block_top - 1u;
        for (var i = block_top - 1u; i >= block_base; i--) {
            let current = left_workgroup[y_offset + i];
            if (current == i) {
                right = i;
                break;
            }
        }

        block_right[global_id.x] = right;

        left_block_local = global_id.x;
        if (right == FLAG) {
            // when left_block_local == 0, right defaults to 0,
            // so left_block_local != 0 due to above condition
            left_block_local--;
        }
        atomicStore(&left_block[global_id.x], left_block_local);
    }

    workgroupBarrier();

    if (_active) {
        loop {
            let previous = left_block_local;
            left_block_local = atomicLoad(&left_block[left_block_local]);
            if (left_block_local == previous) {
                break;
            }

            atomicStore(&left_block[global_id.x], left_block_local);
        }
    }

    workgroupBarrier();

    if (_active) {
        var left = block_right[atomicLoad(&left_block[global_id.x - 1])];
        for (var i = block_base; i < block_top; i++) {
            let j = y_offset + i;
            if (left_workgroup[j] == i) {
                left = i;
            } else {
                left_workgroup[j] = left;
            }
        }
    }
}
