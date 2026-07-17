struct UiViewport {
    viewport_size: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> viewport: UiViewport;
@group(0) @binding(1) var ui_pages: texture_2d_array<f32>;
@group(0) @binding(2) var ui_sampler: sampler;

struct UiVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) texture_page: u32,
    @location(3) @interpolate(flat) style_flags: u32,
};

@vertex
fn ui_vertex(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<u32>,
    @location(2) color: vec4<f32>,
    @location(3) style_flags: u32,
    @builtin(instance_index) texture_page: u32,
) -> UiVertexOutput {
    let ndc = vec2<f32>(
        position.x / viewport.viewport_size.x * 2.0 - 1.0,
        1.0 - position.y / viewport.viewport_size.y * 2.0,
    );
    var output: UiVertexOutput;
    output.clip_position = vec4<f32>(ndc, 0.0, 1.0);
    output.uv = vec2<f32>(uv);
    output.color = color;
    output.texture_page = texture_page;
    output.style_flags = style_flags;
    return output;
}

@fragment
fn ui_fragment(input: UiVertexOutput) -> @location(0) vec4<f32> {
    let dimensions = vec2<f32>(textureDimensions(ui_pages));
    let normalized_uv = (input.uv + vec2<f32>(0.5)) / dimensions;
    let sample = textureSample(
        ui_pages,
        ui_sampler,
        normalized_uv,
        i32(input.texture_page),
    );
    let straight_color = input.color;
    let alpha = sample.a * straight_color.a;
    let premultiplied_rgb = sample.rgb * sample.a * straight_color.rgb * straight_color.a;
    return vec4<f32>(premultiplied_rgb, alpha);
}
