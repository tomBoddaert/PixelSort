const bit_length = 4u;
const base = u32(exp2(f32(bit_length)));
override workgroup_size: u32;

struct Immediates {
    width: u32,
    block_size: u32,
    bit_offset: u32,
}
var<immediate> immediates: Immediates;

@group(0) @binding(0)
var<storage, read> tagged_image: array<vec2<u32>>;

@group(0) @binding(1)
var<storage, read_write> counts: array<u32>;

var<workgroup> wg_counts: array<atomic<u32>, base>;
var<private> p_counts: array<u32, base>;

@compute @workgroup_size(workgroup_size)
fn count(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
    @builtin(subgroup_invocation_id) subgroup_id: u32,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let image_y_offset = immediates.width * global_id.y;
    let block_base = immediates.block_size * global_id.x;
    let block_top = min(block_base + immediates.block_size, immediates.width);

    for (var i = block_base; i < block_top; i++) {
        let value = tagged_image[image_y_offset + i].x; // TODO: cache in workgroup?
        let cmp = extractBits(value, immediates.bit_offset, bit_length);
        p_counts[cmp] += 1;
    }

    for (var i = 0u; i < base; i++) {
        let subgroup_sum = subgroupAdd(p_counts[i]);
        if (subgroup_id == 0) {
            atomicAdd(&wg_counts[i], subgroup_sum);
        }
    }
    workgroupBarrier();

    if (local_id.x != 0) {
        return;
    }

    let counts_y_offset = base * num_workgroups.x * workgroup_id.y;

    var j = workgroup_id.x;
    for (var i = 0u; i < base; i++) {
        counts[counts_y_offset + j] = atomicLoad(&wg_counts[i]);
        j += num_workgroups.x;
    }
}
