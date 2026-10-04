struct UiViewport {
    viewport_size: vec2<f32>,
    time_seconds: f32,
    glint_strength: f32,
};

// Vertex style bits (`ui::UI_STYLE_GLINT`, `UI_STYLE_GRAYSCALE`, `UI_STYLE_BILINEAR`).
const STYLE_GLINT: u32 = 2u;
const STYLE_GRAYSCALE: u32 = 4u;
const STYLE_BILINEAR: u32 = 8u;
// Injected from the renderer's single Rust style-bit definition.
const STYLE_ALPHA_TEST: u32 = UI_STYLE_ALPHA_TEST;

@group(0) @binding(0) var<uniform> viewport: UiViewport;
@group(0) @binding(1) var ui_pages: texture_2d_array<f32>;
@group(0) @binding(2) var ui_sampler: sampler;
@group(0) @binding(3) var ui_linear_sampler: sampler;

struct UiVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) texture_page: u32,
    @location(3) @interpolate(flat) style_flags: u32,
    @location(4) @interpolate(flat) alpha_cutoff: f32,
    @location(5) model_light: f32,
};

@vertex
fn ui_vertex(
    @location(0) position: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) style_flags: u32,
    @location(4) alpha_cutoff: f32,
    @location(5) model_light: f32,
    @builtin(instance_index) texture_page: u32,
) -> UiVertexOutput {
    let ndc = vec2<f32>(
        position.x / viewport.viewport_size.x * 2.0 - position.w,
        position.w - position.y / viewport.viewport_size.y * 2.0,
    );
    var output: UiVertexOutput;
    output.clip_position = vec4<f32>(ndc, position.z, position.w);
    output.uv = uv;
    // Pages, vertex colours and the UI layer all stay sRGB-encoded: vanilla UI
    // blends in gamma space, and the layer composites over the scene after.
    output.color = color;
    output.texture_page = texture_page;
    output.style_flags = style_flags;
    output.alpha_cutoff = alpha_cutoff;
    output.model_light = model_light;
    return output;
}

@fragment
fn ui_fragment(input: UiVertexOutput) -> @location(0) vec4<f32> {
    return shade_ui(input, false);
}

@fragment
fn ui_world_fragment(input: UiVertexOutput) -> @location(0) vec4<f32> {
    return shade_ui(input, true);
}

fn srgb_to_linear(srgb: vec3<f32>) -> vec3<f32> {
    let low = srgb / 12.92;
    let high = pow((srgb + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, srgb <= vec3<f32>(0.04045));
}

fn shade_ui(input: UiVertexOutput, direct: bool) -> vec4<f32> {
    let dimensions = vec2<f32>(textureDimensions(ui_pages));
    // Sprite/glyph UVs address texel *edges*: a glyph spans x0..x0+width. Linear
    // interpolation across the quad therefore already lands on texel centres,
    // and adding half a texel here shifted the whole nearest-sampling grid by
    // half a texel. At a 1:1 draw that sampled one texel to the right; at a 2x
    // draw it gave the leading column one pixel, every other column two, and
    // bled a column of the neighbouring glyph in on the right. Model extrusion
    // side faces instead supply native fractional texel centers; preserve those too.
    let normalized_uv = input.uv / dimensions;
    // Level 0 sampling keeps the per-vertex sampler choice legal in non-uniform flow.
    var sample: vec4<f32>;
    if (input.style_flags & STYLE_BILINEAR) != 0u {
        sample = textureSampleLevel(ui_pages, ui_linear_sampler, normalized_uv, i32(input.texture_page), 0.0);
    } else {
        sample = textureSampleLevel(ui_pages, ui_sampler, normalized_uv, i32(input.texture_page), 0.0);
    }
    if (input.style_flags & STYLE_GRAYSCALE) != 0u {
        // Provisional luma weights (Rec. 601); the retail material is not inspected.
        sample = vec4<f32>(vec3<f32>(dot(sample.rgb, vec3<f32>(0.299, 0.587, 0.114))), sample.a);
    }
    // Native alpha-tested name-tag glyphs threshold the texture, not the faded vertex alpha.
    if input.alpha_cutoff >= 0.0 {
        if sample.a < input.alpha_cutoff { discard; }
    } else if (input.style_flags & STYLE_ALPHA_TEST) != 0u && sample.a < 0.5 {
        discard;
    }
    var straight_color = input.color;
    if direct {
        sample = vec4<f32>(srgb_to_linear(sample.rgb), sample.a);
        straight_color = vec4<f32>(srgb_to_linear(straight_color.rgb), straight_color.a);
    }
    let alpha = sample.a * straight_color.a;
    var premultiplied_rgb = sample.rgb * sample.a * straight_color.rgb * straight_color.a * input.model_light;
    if (input.style_flags & STYLE_GLINT) != 0u {
        // Vanilla scales glint RGB without changing alpha.
        premultiplied_rgb += glint(input.clip_position.xy) * viewport.glint_strength * alpha;
    }
    return vec4<f32>(premultiplied_rgb, alpha);
}

// Two diagonal purple bands scrolling at different rates, added over opaque texels like the
// item glint layers. Provisional: procedural, not the retail glint texture.
fn glint(pixel: vec2<f32>) -> vec3<f32> {
    let t = viewport.time_seconds;
    let a = fract((pixel.x * 0.96 + pixel.y * 0.28) / 96.0 - t * 0.33);
    let b = fract((pixel.x * 0.5 - pixel.y * 0.87) / 80.0 + t * 0.21);
    return vec3<f32>(0.5, 0.25, 0.8) * (glint_band(a) + glint_band(b)) * 0.55;
}

fn glint_band(x: f32) -> f32 {
    return smoothstep(0.0, 0.18, x) * (1.0 - smoothstep(0.18, 0.42, x));
}
