// Composites the gamma-space UI layer over the linear scene the way vanilla
// blends UI: in sRGB-encoded values, not linear light.

@group(0) @binding(0) var ui_layer: texture_2d<f32>;
@group(0) @binding(1) var scene: texture_2d<f32>;

@vertex
fn composite_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

fn srgb_to_linear(srgb: vec3<f32>) -> vec3<f32> {
    let low = srgb / 12.92;
    let high = pow((srgb + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, srgb <= vec3<f32>(0.04045));
}

fn linear_to_srgb(linear: vec3<f32>) -> vec3<f32> {
    let clamped = clamp(linear, vec3<f32>(0.0), vec3<f32>(1.0));
    let low = clamped * 12.92;
    let high = 1.055 * pow(clamped, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, clamped <= vec3<f32>(0.0031308));
}

@fragment
fn composite_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let ui = textureLoad(ui_layer, pixel, 0);
    let under = textureLoad(scene, pixel, 0);
    let gamma = ui.rgb + linear_to_srgb(under.rgb) * (1.0 - ui.a);
    return vec4<f32>(srgb_to_linear(gamma), ui.a + under.a * (1.0 - ui.a));
}

// The damage rectangle is selected by the render pass scissor.
@fragment
fn clear_fragment() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}
