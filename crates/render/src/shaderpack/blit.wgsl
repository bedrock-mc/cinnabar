// Host passes around a shader pack's programs. Pack buffers are stored bottom-up like
// OpenGL images and hold gamma-encoded colour like Minecraft's framebuffer, so copies into
// and out of them flip Y and convert between linear and sRGB encoding.

struct DepthParameters {
    // Near plane of the scene's infinite reverse-Z projection.
    scene_near: f32,
    // Near and far planes of the pack's OpenGL projection.
    gl_near: f32,
    gl_far: f32,
    // Eye distance within which the pack's terrain is fully expanded; beyond it Vanilla
    // terrain the pack has not drawn must not show through the overlay.
    overlay_radius: f32,
    // 1 / the projection's x and y scales, to turn view depth into eye distance.
    inverse_focal: vec2<f32>,
    _padding: vec2<f32>,
    viewport: vec4<f32>,
}

@group(0) @binding(0) var source_colour: texture_2d<f32>;
@group(0) @binding(1) var source_depth: texture_depth_2d;
@group(0) @binding(2) var<uniform> depth_parameters: DepthParameters;
@group(0) @binding(3) var source_sampler: sampler;
// The pack's geometry depth (OpenGL window depth, bottom-up like the pack's buffers) as the
// depth attachment, and its opaque depth as the `depthtex1` float copy.
@group(0) @binding(4) var pack_depth: texture_depth_2d;
@group(0) @binding(5) var pack_opaque_depth: texture_2d<f32>;

struct Fullscreen {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn fullscreen(@builtin(vertex_index) index: u32) -> Fullscreen {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Fullscreen;
    out.position = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3<f32>(0.0031308));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((max(c, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

// The texel of a `size`d source under top-down coordinate `uv`, read bottom-up.
fn flipped_texel(uv: vec2<f32>, size: vec2<u32>) -> vec2<i32> {
    let flipped = vec2<f32>(uv.x, 1.0 - uv.y);
    return vec2<i32>(clamp(flipped * vec2<f32>(size), vec2<f32>(0.0), vec2<f32>(size) - 1.0));
}

struct SceneColourOut {
    @location(0) colour: vec4<f32>,
}

// The scene (linear values) into colortex0, gamma-encoded and bottom-up.
@fragment
fn scene_colour(in: Fullscreen) -> SceneColourOut {
    let texel = vec2<i32>(depth_parameters.viewport.xy) + flipped_texel(in.uv, vec2<u32>(depth_parameters.viewport.zw));
    let colour = textureLoad(source_colour, texel, 0);
    var out: SceneColourOut;
    out.colour = vec4<f32>(linear_to_srgb(colour.rgb), colour.a);
    return out;
}

// OpenGL window depth of the scene sample under this bottom-up pixel; the sky stays at 1.
fn gl_depth(in: Fullscreen) -> f32 {
    let texel = vec2<i32>(depth_parameters.viewport.xy) + flipped_texel(in.uv, vec2<u32>(depth_parameters.viewport.zw));
    let reverse = textureLoad(source_depth, texel, 0);
    if (reverse <= 0.0) {
        return 1.0;
    }
    let distance = depth_parameters.scene_near / reverse;
    let n = depth_parameters.gl_near;
    let f = depth_parameters.gl_far;
    let ndc = (f + n) / (f - n) - 2.0 * f * n / ((f - n) * distance);
    return clamp(ndc * 0.5 + 0.5, 0.0, 1.0);
}

struct SceneDepthOut {
    @location(0) depth0: vec4<f32>,
    @location(1) depth1: vec4<f32>,
    @location(2) depth2: vec4<f32>,
}

@fragment
fn scene_depth(in: Fullscreen) -> SceneDepthOut {
    let depth = vec4<f32>(gl_depth(in), 0.0, 0.0, 1.0);
    return SceneDepthOut(depth, depth, depth);
}

// The scene depth into a depth texture: a comparison `depthtex` or the geometry depth.
@fragment
fn scene_depth_attachment(in: Fullscreen) -> @builtin(frag_depth) f32 {
    return gl_depth(in);
}

// The pack's bottom-up, gamma-encoded screen into the view's linear colour target.
@fragment
fn present(in: Fullscreen) -> @location(0) vec4<f32> {
    let size = textureDimensions(source_colour);
    let texel = flipped_texel(in.uv, size);
    let colour = textureLoad(source_colour, texel, 0);
    return vec4<f32>(srgb_to_linear(colour.rgb), 1.0);
}

// One mip level from the level above it.
@fragment
fn downsample(in: Fullscreen) -> @location(0) vec4<f32> {
    return textureSampleLevel(source_colour, source_sampler, in.uv, 0.0);
}

// Keeps the scene's linear colour outside the camera viewport unchanged.
@fragment
fn preserve(in: Fullscreen) -> @location(0) vec4<f32> {
    return textureLoad(source_colour, vec2<i32>(in.position.xy), 0);
}

// The geometry depth attachment into a float `depthtex`/`shadowtex`, texel for texel.
@fragment
fn depth_copy(in: Fullscreen) -> @location(0) vec4<f32> {
    let depth = textureLoad(pack_depth, vec2<i32>(in.position.xy), 0);
    return vec4<f32>(depth, 0.0, 0.0, 1.0);
}

// Eye distance of an OpenGL window depth under the pack's projection.
fn gl_distance(depth: f32) -> f32 {
    let n = depth_parameters.gl_near;
    let f = depth_parameters.gl_far;
    let ndc = depth * 2.0 - 1.0;
    return 2.0 * f * n / ((f + n) - ndc * (f - n));
}

struct OverlayOut {
    @location(0) colour: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

// What the Vanilla scene drew clearly in front of the pack's opaque geometry (entities,
// particles, terrain not yet re-expanded), written into `colortex0` and the depth attachment
// before translucent geometry draws over it, as Iris draws entities. Nothing comes through
// beyond the fully expanded radius: there the pack's sky or fog stands where its terrain
// waits, rather than Vanilla terrain without gbuffer data.
@fragment
fn vanilla_overlay(in: Fullscreen) -> OverlayOut {
    let texel = vec2<i32>(depth_parameters.viewport.xy) + flipped_texel(in.uv, vec2<u32>(depth_parameters.viewport.zw));
    let reverse = textureLoad(source_depth, texel, 0);
    if (reverse <= 0.0) {
        discard;
    }
    let vanilla = depth_parameters.scene_near / reverse;
    let ray = length(vec3<f32>((in.uv * 2.0 - 1.0) * depth_parameters.inverse_focal, 1.0));
    if (vanilla * ray > depth_parameters.overlay_radius) {
        discard;
    }
    let pack = gl_distance(textureLoad(pack_opaque_depth, vec2<i32>(in.position.xy), 0).r);
    // Both renderers draw the same terrain: only clearly nearer samples come through.
    if (vanilla > pack * 0.995 - 0.05) {
        discard;
    }
    let colour = textureLoad(source_colour, texel, 0);
    var out: OverlayOut;
    out.colour = vec4<f32>(linear_to_srgb(colour.rgb), 1.0);
    out.depth = gl_depth(in);
    return out;
}
