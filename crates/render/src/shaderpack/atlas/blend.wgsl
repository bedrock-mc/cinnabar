// Writes animated display sprites at one atlas mip: the current and next frame-store texels
// mixed by the frame blend, in each sampled view's colour space. With LabPBR frame stores one
// pass writes the colour, normal and specular atlases together.

struct Instance {
    // Display square at this mip: x, y, side, mip level.
    @location(0) target_rect: vec4<u32>,
    // Extent of the target mip.
    @location(1) extent: vec2<u32>,
    // Current and next frame origins in the frame store at this mip.
    @location(2) sources: vec4<u32>,
    @location(3) blend: f32,
}

struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) origin_mip: vec3<i32>,
    @location(1) @interpolate(flat) sources: vec4<i32>,
    @location(2) @interpolate(flat) blend: f32,
}

struct Layers {
    @location(0) colour: vec4<f32>,
    @location(1) normal: vec4<f32>,
    @location(2) specular: vec4<f32>,
}

@group(0) @binding(0) var colour_frames: texture_2d<f32>;
@group(0) @binding(1) var normal_frames: texture_2d<f32>;
@group(0) @binding(2) var specular_frames: texture_2d<f32>;

@vertex
fn vertex(@builtin(vertex_index) index: u32, instance: Instance) -> Varyings {
    // Two triangles: (0,0) (1,0) (0,1) / (0,1) (1,0) (1,1).
    let corner = vec2<u32>((0x32u >> index) & 1u, (0x2cu >> index) & 1u);
    let pixel = vec2<f32>(instance.target_rect.xy + corner * instance.target_rect.z);
    let extent = vec2<f32>(instance.extent);
    var out: Varyings;
    out.position = vec4(pixel.x / extent.x * 2.0 - 1.0, 1.0 - pixel.y / extent.y * 2.0, 0.0, 1.0);
    out.origin_mip = vec3<i32>(instance.target_rect.xyw);
    out.sources = vec4<i32>(instance.sources);
    out.blend = instance.blend;
    return out;
}

fn mixed(frames: texture_2d<f32>, in: Varyings) -> vec4<f32> {
    let local = vec2<i32>(floor(in.position.xy)) - in.origin_mip.xy;
    let current = textureLoad(frames, in.sources.xy + local, in.origin_mip.z);
    let next = textureLoad(frames, in.sources.zw + local, in.origin_mip.z);
    return mix(current, next, in.blend);
}

@fragment
fn fragment_colour(in: Varyings) -> @location(0) vec4<f32> {
    return mixed(colour_frames, in);
}

// LabPBR alpha 255 and 0 both mean no emission; interpolate their decoded strength.
fn mixed_specular(in: Varyings) -> vec4<f32> {
    let local = vec2<i32>(floor(in.position.xy)) - in.origin_mip.xy;
    var current = textureLoad(specular_frames, in.sources.xy + local, in.origin_mip.z);
    var next = textureLoad(specular_frames, in.sources.zw + local, in.origin_mip.z);
    current.a = select(current.a, 0.0, current.a == 1.0);
    next.a = select(next.a, 0.0, next.a == 1.0);
    return mix(current, next, in.blend);
}

@fragment
fn fragment_layers(in: Varyings) -> Layers {
    return Layers(mixed(colour_frames, in), mixed(normal_frames, in), mixed_specular(in));
}
