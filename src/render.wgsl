struct Immediates {
    size: vec2<u32>,
    image_size: vec2<u32>,
}
var<immediate> immediates: Immediates;

@group(0) @binding(0)
var<storage, read> source_image: array<u32>;
@group(0) @binding(1)
var<storage, read> sections: array<u32>;

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
    let scale_factors = vec2<f32>(immediates.size) / vec2<f32>(immediates.image_size);
    let scale_factor = min(scale_factors.x, scale_factors.y);

    let coord = vec2<u32>(coord_in.xy / scale_factor);
    if (coord.x > immediates.image_size.x || coord.y > immediates.image_size.y) {
        discard;
    }

    let i = coord.y * immediates.image_size.x + coord.x;
    var rgba = unpack4x8unorm(source_image[i]) * 0.2;
    rgba += unpack4x8unorm(sections[i]);
    // rgba *= 100;
    rgba.w = 1.0;

    return rgba;
}
