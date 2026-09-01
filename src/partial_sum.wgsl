override workgroup_size: u32;

struct Immediates {
    count_len: u32,
    block_size: u32,
}
var<immediate> immediates: Immediates;

@group(0) @binding(0)
var<storage, read> counts: array<u32>;
@group(0) @binding(1)
var<storage, read_write> offsets: array<u32>;

@compute @workgroup_size(workgroup_size)
fn partial_sum(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let y_offset = immediates.count_len * global_id.y;
    let block_base = immediates.block_size * global_id.x;
    let block_top = min(immediates.block_size + block_base, immediates.count_len);
    let _active = block_base < immediates.count_len;

    if (_active) {
        var block_partial = counts[y_offset + block_base];
        for (
            var i = block_base + 1;
            i < min(block_top, immediates.count_len);
            i++
        ) {
            offsets[y_offset + i] = block_partial;
            block_partial += counts[y_offset + i];
        }

        if (block_top < immediates.count_len) {
            offsets[y_offset + block_top] = block_partial;
        }
    }

    workgroupBarrier();

    // TODO: reduce from workgroup_size to a min of that and #blocks?
    for (var stride = 1u; stride < workgroup_size; stride <<= 1) {
        var value = 0u;
        if (global_id.x >= stride) {
            value = offsets[y_offset + (global_id.x - stride) * immediates.block_size];
        }
        workgroupBarrier();

        if (global_id.x >= stride) {
            offsets[y_offset + global_id.x * immediates.block_size] += value;
        }
        workgroupBarrier();
    }

    if (block_base >= immediates.count_len) {
        return;
    }

    let prefix = offsets[y_offset + block_base];
    for (
        var i = block_base + 1;
        i < min(block_top, immediates.count_len);
        i++
    ) {
        offsets[y_offset + i] += prefix;
    }
}
