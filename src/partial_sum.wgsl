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
fn partial_sum(@builtin(local_invocation_id) local_id: vec3<u32>) {
    let block_base = local_id.x * immediates.block_size;
    let block_top = block_base + immediates.block_size;

    if (block_base < immediates.count_len) {
        var block_partial = counts[block_base];
        for (
            var i = block_base + 1;
            i < min(block_top, immediates.count_len);
            i++
        ) {
            offsets[i] = block_partial;
            block_partial += counts[i];
        }

        if (block_top < immediates.count_len) {
            offsets[block_top] = block_partial;
        } else {
            offsets[0] = 0;
        }
    }

    workgroupBarrier();

    // TODO: reduce from workgroup_size to a min of that and #blocks?
    for (var stride = 1u; stride < workgroup_size; stride += stride) {
        var value = 0u;
        if (local_id.x >= stride) {
            value = offsets[(local_id.x - stride) * immediates.block_size];
        }
        workgroupBarrier();

        if (local_id.x >= stride) {
            offsets[local_id.x * immediates.block_size] += value;
        }
        workgroupBarrier();
    }

    if (block_base >= immediates.count_len) {
        return;
    }

    let prefix = offsets[block_base];
    for (
        var i = block_base + 1;
        i < min(block_top, immediates.count_len);
        i++
    ) {
        offsets[i] += prefix;
    }
}
