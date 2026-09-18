const bit_length = 4u;
const base = u32(exp2(f32(bit_length)));
override workgroup_size: u32;

struct Immediates {
    width: u32,
    block_size: u32,
    vertical: u32,
}
var<immediate> immediates: Immediates;

struct Config {
    boundaries: vec4<u32>,
    sort: u32,
}

@group(0) @binding(0)
var<storage, read> image: array<u32>;
@group(0) @binding(1)
var<uniform> config: Config;
@group(0) @binding(2)
var<storage, read_write> tagged_image: array<vec2<u32>>;
var<private> tagged_image_shift = 0u;
@group(0) @binding(3)
var<storage, read_write> output: array<u32>;

@compute @workgroup_size(workgroup_size)
fn main(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let y_offset = immediates.width * id.y;

    let region_count = count_regions(id.xy, num_workgroups.y);
    let region_offset = wg_partial_sum(id.x, region_count);
    label_regions(id.xy, num_workgroups.y, y_offset, region_offset);

    sort(id.x, y_offset);

    write_image(id.xy, num_workgroups.y, y_offset);
}

fn count_regions(id: vec2<u32>, height: u32) -> u32 {
    if (
        id.x == 0u
        || id.x >= ceil_div(immediates.width, immediates.block_size)
    ) {
        return 0u;
    }

    let block_offset = (id.x - 1u) * immediates.block_size;
    let y_offset = id.y * select(immediates.width, 1u, bool(immediates.vertical));
    let stride = select(1u, height, bool(immediates.vertical));

    var i = y_offset + block_offset * stride;

    let block_base = y_offset + block_offset * stride;
    // Does not include the last block, so a integer multiple of the block size
    let block_top = block_base + immediates.block_size * stride;

    var region_count = 0u;
    var previous = source_boundary_id(block_base);

    for (var i = block_base + stride; i <= block_top; i += stride) {
        let current = source_boundary_id(i);
        region_count += u32(previous != current);
        previous = current;
    }

    return region_count;
}

var<workgroup> wg_buffer: array<u32, workgroup_size * 2u>;
fn wg_partial_sum(id: u32, count: u32) -> u32 {
    wg_buffer[id << 1u] = count;

    var shift = 0u;
    var value: u32;

    for (var stride = 1u; stride < workgroup_size; stride <<= 1u) {
        let id_shifted = id << 1u | shift;
        workgroupBarrier();
        if (id >= stride) {
            value = wg_buffer[id_shifted - (stride << 1u)]
                + wg_buffer[id_shifted];
        }

        wg_buffer[id_shifted ^ 1u] = value;
        shift ^= 1u;
    }

    return value;
}

fn label_regions(
    id: vec2<u32>,
    height: u32,
    y_offset: u32,
    region_offset: u32,
) {
    if (id.x >= ceil_div(immediates.width, immediates.block_size)) {
        return;
    }

    let rotated_iter = image_iter(id, height);

    let block_base = immediates.block_size * id.x;
    let block_top = min(block_base + immediates.block_size, immediates.width);

    var region_counter = region_offset;

    let rgb = image[rotated_iter.block_base];
    let value = get_value(rgb);
    let current = find_boundary_id(value);
    write_tagged(y_offset + block_base, rgb, region_counter, current, value);

    var previous = current;

    var j = rotated_iter.block_base + rotated_iter.stride;
    for (var i = block_base + 1u; i < block_top; i++) {
        let rgb = image[j];
        let value = get_value(rgb);
        let current = find_boundary_id(value);
        region_counter += u32(current != previous);

        write_tagged(y_offset + i, rgb, region_counter, current, value);

        previous = current;
        j += rotated_iter.stride;
    }
}

fn sort(id: u32, y_offset: u32) {
    for (var bit_offset = 0u; bit_offset < 8u; bit_offset += bit_length) {
        partial_sort(id, y_offset, bit_offset);
    }
    for (var bit_offset = 16u; bit_offset < 32u; bit_offset += bit_length) {
        partial_sort(id, y_offset, bit_offset);
    }
}

fn partial_sort(
    id: u32,
    y_offset: u32,
    bit_offset: u32,
) {
    let p_counts = p_count(id, y_offset, bit_offset);
    wg_partial_sum_counts(id, p_counts);
    reorder(id, y_offset, bit_offset);
}

fn p_count(
    id: u32,
    y_offset: u32,
    bit_offset: u32,
) -> array<u32, base> {
    var p_counts: array<u32, base>;

    let block_base = immediates.block_size * id;
    let block_top = min(block_base + immediates.block_size, immediates.width);

    for (var i = block_base; i < block_top; i++) {
        let value = read_tagged(y_offset + i).x;
        let cmp = extractBits(value, bit_offset, bit_length);
        p_counts[cmp]++;
    }

    return p_counts;
}

var<workgroup> wg_counts: array<u32, workgroup_size * base>;
fn wg_partial_sum_counts(id: u32, p_counts: array<u32, base>) {
    if (id == 0u) {
        wg_counts[0u] = 0u;
    }

    var j = id + 1u;
    for (
        var cmp = 0u;
        cmp < base - u32(id == workgroup_size - 1u);
        cmp++
    ) {
        wg_counts[j] = p_counts[cmp];
        j += workgroup_size;
    }

    workgroupBarrier();

    var pre_sum = 0u;
    if (id != 0u) {
        let pre_block_base = base * (id - 1u);
        let pre_block_top = pre_block_base + base;

        for (var i = pre_block_base; i < pre_block_top; i++) {
            pre_sum += wg_counts[i];
        }
    }
    pre_sum = wg_partial_sum(id, pre_sum);

    let block_base = base * id;
    let block_top = block_base + base;
    for (var i = block_base; i < block_top; i++) {
        pre_sum += wg_counts[i];
        wg_counts[i] = pre_sum;
    }

    workgroupBarrier();
}

fn reorder(
    id: u32,
    y_offset: u32,
    bit_offset: u32,
) {
    var offsets: array<u32, base>;
    var j = id;
    for (var cmp = 0u; cmp < base; cmp++) {
        offsets[cmp] = wg_counts[j];
        j += workgroup_size;
    }

    let block_base = immediates.block_size * id;
    let block_top = min(block_base + immediates.block_size, immediates.width);
    for (var i = block_base; i < block_top; i++) {
        let pixel = read_tagged(y_offset + i);
        let cmp = extractBits(pixel.x, bit_offset, bit_length);
        let pos = offsets[cmp];
        offsets[cmp] = pos + 1u;

        tagged_image[(y_offset + pos) << 1u | tagged_image_shift ^ 1u] = pixel;
    }

    tagged_image_shift ^= 1u;
    storageBarrier();
}

fn write_image(id: vec2<u32>, height: u32, y_offset: u32) {
    let rotated_iter = image_iter(id, height);

    let block_base = immediates.block_size * id.x;
    let block_top = min(block_base + immediates.block_size, immediates.width);

    var j = rotated_iter.block_base;
    for (var i = block_base; i < block_top; i++) {
        let rgb = read_tagged(y_offset + i).y;
        output[j] = rgb;

        j += rotated_iter.stride;
    }
}

fn ceil_div(lhs: u32, rhs: u32) -> u32 {
    return (lhs + rhs - 1u) / rhs;
}

struct ImageIter {
    block_base: u32,
    block_top: u32,
    stride: u32,
}
fn image_iter(id: vec2<u32>, height: u32) -> ImageIter {
    let block_offset = id.x * immediates.block_size;
    let y_offset = id.y * select(immediates.width, 1u, bool(immediates.vertical));
    let stride = select(1u, height, bool(immediates.vertical));

    return ImageIter (
        y_offset + block_offset * stride,
        y_offset + min(block_offset + immediates.block_size, immediates.width) * stride,
        stride,
    );
}

fn get_value(rgb: u32) -> f32 {
    let sqrt = sqrt(unpack4x8unorm(rgb));
    let sqrt_mean = (sqrt.x + sqrt.y + sqrt.z) / 3f;
    return sqrt_mean * sqrt_mean;
}
fn find_boundary_id(value: f32) -> u32 {
    var i = 0u;
    var boundaries4: vec4<f32>;

    loop {
        if (i % 4u == 0) {
            boundaries4 = unpack4x8unorm(config.boundaries[i / 4u]);
        }
        let boundary = boundaries4[i % 4u];

        if (boundary == 0 || boundary > value) {
            break;
        }
        i++;
    }

    return i;
}
fn get_boundary_id(rgb: u32) -> u32 {
    return find_boundary_id(get_value(rgb));
}
fn source_boundary_id(index: u32) -> u32 {
    return get_boundary_id(image[index]);
}
fn sorted(boundary_id: u32) -> u32 {
    return extractBits(config.sort, boundary_id << 1u, 2u);
}

fn write_tagged(
    index: u32,
    rgb: u32,
    change_count: u32,
    boundary_id: u32,
    value: f32,
) {
    var value_written: f32;
    switch sorted(boundary_id) {
        case 2: {
            value_written = value;
        }
        case 3: {
            value_written = 1f - value;
        }
        default: {}
    }
    tagged_image[index << 1u | tagged_image_shift] = vec2<u32>(
        // pack4x8unorm places value i at bits 8×i to 8×i+7
        (change_count << 16u) | pack4x8unorm(vec4<f32>(value_written, 0f, 0f, 0f)),
        rgb,
    );
}
fn read_tagged(index: u32) -> vec2<u32> {
    return tagged_image[index << 1u | tagged_image_shift];
}

// == TESTING ==

@group(1) @binding(0)
var<storage, read_write> test_buffer: array<u32>;

@compute @workgroup_size(workgroup_size)
fn test_region_count(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let change_count = count_regions(id.xy, num_workgroups.y);

    test_buffer[workgroup_size * id.y + id.x] = change_count;
}

@compute @workgroup_size(workgroup_size)
fn test_wg_partial_sum(@builtin(global_invocation_id) id: vec3<u32>) {
    let region_count = test_buffer[workgroup_size * id.y + id.x];

    let offset = wg_partial_sum(id.x, region_count);

    test_buffer[workgroup_size * id.y + id.x] = offset;
}

@compute @workgroup_size(workgroup_size)
fn test_label_regions(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(num_workgroups) num_workgroups: vec3<u32>,
) {
    let offset = test_buffer[workgroup_size * id.y + id.x];
    let y_offset = immediates.width * id.y;

    label_regions(id.xy, num_workgroups.y, y_offset, offset);
}

@compute @workgroup_size(workgroup_size)
fn test_p_count(@builtin(global_invocation_id) id: vec3<u32>) {
    let y_offset = immediates.width * id.y;

    let p_counts = p_count(id.x, y_offset, 16u);

    for (var cmp = 0u; cmp < base; cmp++) {
        test_buffer[base * (workgroup_size * id.y + id.x) + cmp]
            = p_counts[cmp];
    }
}

@compute @workgroup_size(workgroup_size)
fn test_wg_partial_sum_counts(@builtin(global_invocation_id) id: vec3<u32>) {
    var p_counts: array<u32, base>;
    for (var cmp = 0u; cmp < base; cmp++) {
        let buffer_offset = base * (workgroup_size * id.y + id.x) + cmp;
        p_counts[cmp] = test_buffer[buffer_offset];
        test_buffer[buffer_offset] = 0u;
    }

    wg_partial_sum_counts(id.x, p_counts);

    for (var cmp = 0u; cmp < base; cmp++) {
        test_buffer[base * (workgroup_size * id.y + id.x) + cmp]
            = wg_counts[cmp * workgroup_size + id.x];
    }
}

@compute @workgroup_size(workgroup_size)
fn test_reorder(@builtin(global_invocation_id) id: vec3<u32>) {
    let y_offset = immediates.width * id.y;

    for (var cmp = 0u; cmp < base; cmp++) {
        let buffer_offset = base * (workgroup_size * id.y + id.x) + cmp;
        wg_counts[cmp * workgroup_size + id.x] = test_buffer[buffer_offset];
    }

    reorder(id.x, y_offset, 0u);
}

@compute @workgroup_size(workgroup_size)
fn test_partial_sort(@builtin(global_invocation_id) id: vec3<u32>) {
    let y_offset = immediates.width * id.y;

    partial_sort(id.x, y_offset, 0u);
}

@compute @workgroup_size(workgroup_size)
fn test_partial_sort2(@builtin(global_invocation_id) id: vec3<u32>) {
    let y_offset = immediates.width * id.y;

    partial_sort(id.x, y_offset, 0u);
    partial_sort(id.x, y_offset, 16u);

    let block_base = immediates.block_size * id.x;
    let block_top = min(block_base + immediates.block_size, immediates.width);
    for (var i = block_base; i < block_top; i++) {
        let pixel = read_tagged(y_offset + i);
        test_buffer[(y_offset + i) << 1u] = pixel.x;
        test_buffer[(y_offset + i) << 1u | 1u] = pixel.y;
    }
}

@compute @workgroup_size(workgroup_size)
fn test_sort(@builtin(global_invocation_id) id: vec3<u32>) {
    let y_offset = immediates.width * id.y;

    sort(id.x, y_offset);

    let block_base = immediates.block_size * id.x;
    let block_top = min(block_base + immediates.block_size, immediates.width);
    for (var i = block_base; i < block_top; i++) {
        let pixel = read_tagged(y_offset + i);
        test_buffer[(y_offset + i) << 1u] = pixel.x;
        test_buffer[(y_offset + i) << 1u | 1u] = pixel.y;
    }
}
