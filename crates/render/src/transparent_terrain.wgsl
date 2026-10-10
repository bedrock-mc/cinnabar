// One pipeline for sorted transparent terrain: water and model draws alternate back to front,
// and every program switch between them costs the GPU as much as the drawing itself. A draw whose
// first instance carries TRANSPARENT_WATER_DRAW_FLAG shades its instances as transparent liquid,
// sorted refs or direct records as `liquid_draw_ref` selects; any other draw shades model draw
// refs. Both paths run the liquid and model programs' own vertex and shading functions on their
// own outputs, so the picture matches drawing each family through its own pipeline.
#import cinnabar::chunk_bindings::TRANSPARENT_WATER_DRAW_FLAG
#import cinnabar::liquid::{VertexOutput as LiquidOutput, liquid_draw_ref, vertex_for_ref, shade_liquid}
#import cinnabar::model::{VertexOutput as ModelOutput, model_vertex, shade_blend}

struct VertexOutput {
    // Invariant like both families' outputs, so shared edges keep identical positions.
    @builtin(position) @invariant clip_position: vec4<f32>,
    @location(0) @interpolate(flat) water: u32,
    // Carried by both families.
    @location(1) @interpolate(flat) current_texture: u32,
    @location(2) @interpolate(flat) next_texture: u32,
    @location(3) @interpolate(flat) frame_blend: f32,
    @location(4) @interpolate(flat) two_sided: u32,
    @location(5) world_position: vec3<f32>,
    @location(6) lighting: vec3<f32>,
    @location(7) normal: vec3<f32>,
#ifdef ENHANCED
    @location(8) sky_light: f32,
    @location(9) ambient_occlusion: f32,
    @location(10) @interpolate(flat) tint_surface: vec4<f32>,
#else
    @location(8) native_light_levels: vec2<f32>,
#endif
    // Transparent liquid.
    @location(11) liquid_uv: vec2<f32>,
    @location(12) water_tint: vec4<f32>,
    @location(13) @interpolate(flat) depth_write_route: u32,
    @location(14) @interpolate(flat) native_face_shade: f32,
    // Transparent models; partially covered MSAA pixels must sample inside the authored quad.
    @location(15) @interpolate(perspective, centroid) model_uv: vec2<f32>,
    @location(16) @interpolate(flat) material_flags: u32,
    @location(17) @interpolate(flat) local_position: vec3<f32>,
    @location(18) @interpolate(flat) biome_record: u32,
    @location(19) @interpolate(flat) visible: u32,
    @location(20) @interpolate(flat) world_origin: vec3<f32>,
#ifndef ENHANCED
    @location(21) native_ao_face: f32,
    @location(22) @interpolate(flat) tint_gamma: vec3<f32>,
#endif
}

@vertex
fn vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    var out: VertexOutput;
    if ((instance_index & TRANSPARENT_WATER_DRAW_FLAG) != 0u) {
        let draw_ref = liquid_draw_ref(vertex_index, instance_index & ~TRANSPARENT_WATER_DRAW_FLAG);
        let liquid = vertex_for_ref(draw_ref, vertex_index);
        out.clip_position = liquid.clip_position;
        out.water = 1u;
        out.current_texture = liquid.current_texture;
        out.next_texture = liquid.next_texture;
        out.frame_blend = liquid.frame_blend;
        out.two_sided = liquid.two_sided;
        out.world_position = liquid.world_position;
        out.lighting = liquid.lighting;
#ifdef ENHANCED
        out.normal = liquid.normal;
        out.sky_light = liquid.sky_light;
        out.ambient_occlusion = liquid.ambient_occlusion;
        out.tint_surface = vec4(1.0, 1.0, 1.0, f32(liquid.surface_class));
#else
        out.native_light_levels = liquid.native_light_levels;
#endif
        out.liquid_uv = liquid.uv;
        out.water_tint = liquid.water_tint;
        out.depth_write_route = liquid.depth_write_route;
        out.native_face_shade = liquid.native_face_shade;
        return out;
    }
    let model = model_vertex(vertex_index, instance_index);
    out.clip_position = model.clip_position;
    out.water = 0u;
    out.current_texture = model.current_texture;
    out.next_texture = model.next_texture;
    out.frame_blend = model.frame_blend;
    out.two_sided = model.two_sided;
    out.world_position = model.world_position;
    out.lighting = model.lighting;
    out.normal = model.normal;
#ifdef ENHANCED
    out.sky_light = model.sky_light;
    out.ambient_occlusion = model.ambient_occlusion;
    out.tint_surface = model.tint_surface;
#else
    out.native_light_levels = model.native_light_levels;
    out.native_ao_face = model.native_ao_face;
    out.tint_gamma = model.tint_gamma;
#endif
    out.model_uv = model.uv;
    out.material_flags = model.material_flags;
    out.local_position = model.local_position;
    out.biome_record = model.biome_record;
    out.visible = model.visible;
    out.world_origin = model.world_origin;
    return out;
}

fn liquid_output(in: VertexOutput) -> LiquidOutput {
    var liquid: LiquidOutput;
    liquid.clip_position = in.clip_position;
    liquid.uv = in.liquid_uv;
    liquid.current_texture = in.current_texture;
    liquid.next_texture = in.next_texture;
    liquid.frame_blend = in.frame_blend;
    liquid.water_tint = in.water_tint;
    liquid.lighting = in.lighting;
#ifdef ENHANCED
    liquid.sky_light = in.sky_light;
    liquid.ambient_occlusion = in.ambient_occlusion;
    liquid.normal = in.normal;
    liquid.surface_class = u32(in.tint_surface.w);
#else
    liquid.native_light_levels = in.native_light_levels;
#endif
    liquid.depth_write_route = in.depth_write_route;
    liquid.world_position = in.world_position;
    liquid.native_face_shade = in.native_face_shade;
    liquid.two_sided = in.two_sided;
    return liquid;
}

fn model_output(in: VertexOutput) -> ModelOutput {
    var model: ModelOutput;
    model.clip_position = in.clip_position;
    model.uv = in.model_uv;
    model.current_texture = in.current_texture;
    model.normal = in.normal;
    model.material_flags = in.material_flags;
    model.local_position = in.local_position;
    model.biome_record = in.biome_record;
    model.next_texture = in.next_texture;
    model.frame_blend = in.frame_blend;
    model.visible = in.visible;
    model.lighting = in.lighting;
#ifdef ENHANCED
    model.sky_light = in.sky_light;
    model.ambient_occlusion = in.ambient_occlusion;
    model.tint_surface = in.tint_surface;
#else
    model.native_light_levels = in.native_light_levels;
    model.native_ao_face = in.native_ao_face;
    model.tint_gamma = in.tint_gamma;
#endif
    model.world_origin = in.world_origin;
    model.two_sided = in.two_sided;
    model.world_position = in.world_position;
    return model;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    // Derivatives stay in uniform control flow; each family only reads its own.
    let liquid_dx = dpdx(in.liquid_uv);
    let liquid_dy = dpdy(in.liquid_uv);
    let model_dx = dpdx(in.model_uv);
    let model_dy = dpdy(in.model_uv);
    if (in.water != 0u) {
        // Packed liquid corners wind clockwise; this pipeline's front face is counter-clockwise.
        return shade_liquid(liquid_output(in), !front_facing, liquid_dx, liquid_dy);
    }
    return shade_blend(model_output(in), front_facing, model_dx, model_dy);
}
