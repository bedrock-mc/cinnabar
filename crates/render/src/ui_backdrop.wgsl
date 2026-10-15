// Mod-surface backdrops: replaces the scene under each backdrop shape with the scene blurred,
// saturated and brightened by its vertex paint, sampled from one shared blur pyramid. Element
// filters reuse the same sampling over an offscreen UI group.

struct UiViewport {
    viewport_size: vec2<f32>,
    time_seconds: f32,
    glint_strength: f32,
};

@group(0) @binding(0) var<uniform> viewport: UiViewport;
// The scene before backdrops, and its blur pyramid: mip m holds level m + 1, the scene
// downsampled m + 1 times.
@group(1) @binding(0) var scene: texture_2d<f32>;
@group(1) @binding(1) var pyramid: texture_2d<f32>;
@group(1) @binding(2) var pyramid_sampler: sampler;

struct BackdropOutput {
    @builtin(position) clip_position: vec4<f32>,
    // Area coverage times opacity.
    @location(0) coverage: f32,
    // Blur in physical pixels, saturation and brightness; constant across a shape.
    @location(1) @interpolate(flat) paint: vec3<f32>,
};

// Matches `ui_vertex` in ui.wgsl: positions are physical pixels times W.
@vertex
fn backdrop_vertex(
    @location(0) position: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(6) overlay_color: vec4<f32>,
) -> BackdropOutput {
    let ndc = vec2<f32>(
        position.x / viewport.viewport_size.x * 2.0 - position.w,
        position.w - position.y / viewport.viewport_size.y * 2.0,
    );
    var output: BackdropOutput;
    output.clip_position = vec4<f32>(ndc, position.z, position.w);
    output.coverage = uv.y * color.a;
    output.paint = overlay_color.xyz;
    return output;
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

// Pyramid level whose reconstruction has standard deviation `sigma` scene pixels: L downsamples
// contribute 0.25 * (4^L - 1) px^2 and the four-tap bilinear reconstruction 5/12 * 4^L px^2.
fn level_for(sigma: f32) -> f32 {
    return 0.5 * log2((sigma * sigma + 0.25) * 1.5);
}

// The source at `pixel` blurred with a Gaussian-like kernel of standard deviation `sigma`.
fn blurred(pixel: vec2<f32>, sigma: f32) -> vec4<f32> {
    let exact = textureLoad(scene, vec2<i32>(pixel), 0);
    if sigma <= 0.0 {
        return exact;
    }
    let size = vec2<f32>(textureDimensions(scene));
    let level = clamp(level_for(sigma), 1.0, f32(textureNumLevels(pyramid)));
    let uv = pixel / size;
    let offset = 0.5 * exp2(level) / size;
    let lod = level - 1.0;
    var sum = textureSampleLevel(pyramid, pyramid_sampler, uv - offset, lod);
    sum += textureSampleLevel(pyramid, pyramid_sampler, uv + offset, lod);
    sum += textureSampleLevel(pyramid, pyramid_sampler, uv + vec2<f32>(offset.x, -offset.y), lod);
    sum += textureSampleLevel(pyramid, pyramid_sampler, uv - vec2<f32>(offset.x, -offset.y), lod);
    let soft = sum * 0.25;
    // Below the first level, fade in from the exact source.
    let first = sqrt(2.0 / 3.0 * 4.0 - 0.25);
    return mix(exact, soft, clamp(sigma / first, 0.0, 1.0));
}

@fragment
fn backdrop_fragment(input: BackdropOutput) -> @location(0) vec4<f32> {
    let alpha = clamp(input.coverage, 0.0, 1.0);
    if alpha <= 0.0 {
        discard;
    }
    // CSS filters act on sRGB-encoded values; Rec. 709 luma weights match `saturate()`.
    var srgb = linear_to_srgb(blurred(input.clip_position.xy, input.paint.x).rgb);
    let luma = dot(srgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    srgb = (vec3<f32>(luma) + (srgb - vec3<f32>(luma)) * input.paint.y) * input.paint.z;
    let linear = srgb_to_linear(clamp(srgb, vec3<f32>(0.0), vec3<f32>(1.0)));
    return vec4<f32>(linear * alpha, alpha);
}

// Composites a filter group's premultiplied gamma-space drawing, blurred, into its UI layer.
@fragment
fn filter_fragment(input: BackdropOutput) -> @location(0) vec4<f32> {
    let alpha = clamp(input.coverage, 0.0, 1.0);
    return blurred(input.clip_position.xy, input.paint.x) * alpha;
}
