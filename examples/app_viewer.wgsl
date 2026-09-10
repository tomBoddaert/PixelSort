struct Immediates {
    viewport_top_left: vec2<f32>,
    viewport_size: vec2<f32>,
    image_size: vec2<u32>,
}
var<immediate> immediates: Immediates;

@group(0) @binding(0)
var<storage, read> image: array<u32>;

@vertex
fn vertex(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    const pos = array(
        vec4<f32>(-1.0, -1.0, 0.0, 1.0),
        vec4<f32>(-1.0, 1.0, 0.0, 1.0),
        vec4<f32>(1.0, 1.0, 0.0, 1.0),
        vec4<f32>(1.0, 1.0, 0.0, 1.0),
        vec4<f32>(1.0, -1.0, 0.0, 1.0),
        vec4<f32>(-1.0, -1.0, 0.0, 1.0),
    );
    return pos[vertex_index];
}

@fragment
fn fragment(@builtin(position) coord_in: vec4<f32>) -> @location(0) vec4<f32> {
    var pos = coord_in.xy - immediates.viewport_top_left;
    if (pos.x >= immediates.viewport_size.x || pos.y >= immediates.viewport_size.y) {
        discard;
    }

    let scale_factors = immediates.viewport_size / vec2<f32>(immediates.image_size);
    let scale_factor = min(scale_factors.x, scale_factors.y);

    let coord = vec2<u32>(pos / scale_factor);
    if (coord.x >= immediates.image_size.x || coord.y >= immediates.image_size.y) {
        discard;
    }

    let i = coord.y * immediates.image_size.x + coord.x;
    let rgb = unpack4x8unorm(image[i]).xyz;

    return vec4(rgb, 1);
}
