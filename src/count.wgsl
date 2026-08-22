const bit_length = 4u;
const base = u32(exp2(f32(bit_length)));
override workgroup_size: u32;

struct Immediates {
    buffer_len: u32,
    block_size: u32,
    bit_offset: u32,
}
var<immediate> immediates: Immediates;

@group(0) @binding(0)
var<storage, read> input: array<u32>;

@group(0) @binding(1)
var<storage, read_write> counts: array<u32>;

var<workgroup> wg_counts: array<atomic<u32>, base>;
var<private> p_counts: array<u32, base>;

@compute @workgroup_size(workgroup_size)
fn count(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let input_offset = global_id.x * immediates.block_size;
    for (
        var i = input_offset;
        i < min(input_offset + immediates.block_size, immediates.buffer_len);
        i++
    ) {
        let value = input[i]; // TODO: cache in workgroup?
        let cmp = extractBits(value, immediates.bit_offset, bit_length);
        p_counts[cmp] += 1;
    }

    for (var i = 0u; i < base; i++) {
        atomicAdd(&wg_counts[i], p_counts[i]);
    }
    workgroupBarrier();

    if (local_id.x != 0) {
        return;
    }

    var j = workgroup_id.x;
    for (var i = 0u; i < base; i++) {
        counts[j] = atomicLoad(&wg_counts[i]);
        j += num_workgroups.x;
    }
}
