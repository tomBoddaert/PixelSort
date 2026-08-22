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
var<storage, read_write> output: array<u32>;

@group(0) @binding(2)
var<storage, read> offsets: array<u32>;

var<workgroup> wg_offsets: array<u32, workgroup_size * base>;
var<private> p_buffer: array<u32, base>;

@compute @workgroup_size(workgroup_size)
fn reorder(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    if (local_id.x == 0) {
        var j = workgroup_id.x;
        for (var i = 0u; i < base * workgroup_size; i += workgroup_size) {
            wg_offsets[i] = offsets[j];
            j += num_workgroups.x;
        }
    }
    workgroupBarrier();

    let input_offset = global_id.x * immediates.block_size;
    for (
        var i = input_offset;
        i < min(input_offset + immediates.block_size, immediates.buffer_len);
        i++
    ) {
        let value = input[i]; // TODO: cache in workgroup?
        let cmp = extractBits(value, immediates.bit_offset, bit_length);
        p_buffer[cmp] += 1;
    }

    if (local_id.x != workgroup_size - 1) {
        var j = local_id.x + 1;
        for (var cmp = 0u; cmp < base; cmp++) {
            wg_offsets[j] += p_buffer[cmp];
            j += workgroup_size;
        }
    }
    workgroupBarrier();

    partial_sum(local_id);

    var j = local_id.x;
    for (var cmp = 0u; cmp < base; cmp++) {
        p_buffer[cmp] = wg_offsets[j];
        j += workgroup_size;
    }

    for (
        var i = input_offset;
        i < min(input_offset + immediates.block_size, immediates.buffer_len);
        i++
    ) {
        let value = input[i]; // TODO: cache in workgroup?
        let cmp = extractBits(value, immediates.bit_offset, bit_length);
        let j = p_buffer[cmp];
        p_buffer[cmp]++;
        output[j] = value;
    }
}

fn partial_sum(local_id: vec3<u32>) {
    for (var stride = 1u; stride < workgroup_size; stride += stride) {
        if (local_id.x >= stride) {
            var j = local_id.x - stride;
            for (var cmp = 0u; cmp < base; cmp++) {
                p_buffer[cmp] = wg_offsets[j];
                j += workgroup_size;
            }
        }
        workgroupBarrier();

        if (local_id.x >= stride) {
            var j = local_id.x;
            for (var cmp = 0u; cmp < base; cmp++) {
                wg_offsets[j] += p_buffer[cmp];
                j += workgroup_size;
            }
        }
        workgroupBarrier();
    }
}
