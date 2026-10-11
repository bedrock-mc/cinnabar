// One pipeline for sorted transparent terrain: water and model draws alternate back to front,
// and every program switch between them costs the GPU as much as the drawing itself. A draw whose
// first instance carries TRANSPARENT_WATER_DRAW_FLAG shades its instances as transparent liquid,
// sorted refs or direct records as `liquid_draw_ref` selects; any other draw shades model draw
// refs. Both paths run the liquid and model programs' own vertex and shading functions on their
// own outputs, so the picture matches drawing each family through its own pipeline.
import render::chunk_bindings::TRANSPARENT_WATER_DRAW_FLAG;
import render::liquid::{VertexOutput as LiquidOutput, liquid_draw_ref, vertex_for_ref, shade_liquid};
import render::model::{VertexOutput as ModelOutput, model_vertex, shade_blend};

struct VertexOutput {
    // Invariant like both families' outputs, so shared edges keep identical positions.
    @builtin(position) @invariant clip_position: vec4<f32>,
    @location(0) @interpolate(flat) routing: vec4<u32>,
    @location(1) @interpolate(flat) textures: vec2<u32>,
    @location(2) @interpolate(flat) material_biome: vec2<u32>,
    @location(3) @interpolate(flat) frame_face: vec2<f32>,
    @location(4) @interpolate(flat) local_position: vec3<f32>,
    @location(5) @interpolate(flat) world_origin: vec3<f32>,
    @location(6) world_position: vec3<f32>,
    @location(7) lighting: vec3<f32>,
    @location(8) normal: vec3<f32>,
    @if(ENHANCED)
    @location(9) sky_ao: vec2<f32>,
    @if(ENHANCED)
    @location(10) @interpolate(flat) surface_class: u32,
    @if(!ENHANCED)
    @location(9) native_light_ao: vec3<f32>,
    @if(!ENHANCED)
    @location(10) @interpolate(flat) tint_gamma: vec3<f32>,
    @location(11) liquid_uv: vec2<f32>,
    @location(12) water_tint: vec4<f32>,
    // Centroid sampling stays separate from the liquid's ordinary UV interpolation.
    @location(13) @interpolate(perspective, centroid) model_uv: vec2<f32>,
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
        out.routing.x = 1u;
        out.textures.x = liquid.current_texture;
        out.textures.y = liquid.next_texture;
        out.frame_face.x = liquid.frame_blend;
        out.routing.y = liquid.two_sided;
        out.world_position = liquid.world_position;
        out.lighting = liquid.lighting;
        @if(ENHANCED)
        out.normal = liquid.normal;
        @if(ENHANCED)
        out.sky_ao.x = liquid.sky_light;
        @if(ENHANCED)
        out.sky_ao.y = liquid.ambient_occlusion;
        @if(ENHANCED)
        out.surface_class = liquid.surface_class;
        @if(!ENHANCED)
        out.native_light_ao = vec3(liquid.native_light_levels, 0.0);
        out.liquid_uv = liquid.uv;
        out.water_tint = liquid.water_tint;
        out.routing.z = liquid.depth_write_route;
        out.frame_face.y = liquid.native_face_shade;
        return out;
    }
    let model = model_vertex(vertex_index, instance_index);
    out.clip_position = model.clip_position;
    out.routing.x = 0u;
    out.textures.x = model.current_texture;
    out.textures.y = model.next_texture;
    out.frame_face.x = model.frame_blend;
    out.routing.y = model.visibility.y;
    out.world_position = model.world_position;
    out.lighting = model.lighting;
    out.normal = model.normal;
    @if(ENHANCED)
    out.sky_ao.x = model.sky_light;
    @if(ENHANCED)
    out.sky_ao.y = model.ambient_occlusion;
    @if(ENHANCED)
    out.surface_class = model.surface_class;
    @if(!ENHANCED)
    out.native_light_ao = vec3(model.native_light_levels, model.native_ao_face);
    @if(!ENHANCED)
    out.tint_gamma = model.tint_gamma;
    out.model_uv = model.uv;
    out.material_biome.x = model.material_flags;
    out.local_position = model.local_position;
    out.material_biome.y = model.biome_record;
    out.routing.w = model.visibility.x;
    out.world_origin = model.world_origin;
    return out;
}

fn liquid_output(in: VertexOutput) -> LiquidOutput {
    var liquid: LiquidOutput;
    liquid.clip_position = in.clip_position;
    liquid.uv = in.liquid_uv;
    liquid.current_texture = in.textures.x;
    liquid.next_texture = in.textures.y;
    liquid.frame_blend = in.frame_face.x;
    liquid.water_tint = in.water_tint;
    liquid.lighting = in.lighting;
    @if(ENHANCED)
    liquid.sky_light = in.sky_ao.x;
    @if(ENHANCED)
    liquid.ambient_occlusion = in.sky_ao.y;
    @if(ENHANCED)
    liquid.normal = in.normal;
    @if(ENHANCED)
    liquid.surface_class = in.surface_class;
    @if(!ENHANCED)
    liquid.native_light_levels = in.native_light_ao.xy;
    liquid.depth_write_route = in.routing.z;
    liquid.world_position = in.world_position;
    liquid.native_face_shade = in.frame_face.y;
    liquid.two_sided = in.routing.y;
    return liquid;
}

fn model_output(in: VertexOutput) -> ModelOutput {
    var model: ModelOutput;
    model.clip_position = in.clip_position;
    model.uv = in.model_uv;
    model.current_texture = in.textures.x;
    model.normal = in.normal;
    model.material_flags = in.material_biome.x;
    model.local_position = in.local_position;
    model.biome_record = in.material_biome.y;
    model.next_texture = in.textures.y;
    model.frame_blend = in.frame_face.x;
    model.visibility.x = in.routing.w;
    model.lighting = in.lighting;
    @if(ENHANCED)
    model.sky_light = in.sky_ao.x;
    @if(ENHANCED)
    model.ambient_occlusion = in.sky_ao.y;
    @if(ENHANCED)
    model.surface_class = in.surface_class;
    @if(!ENHANCED)
    model.native_light_levels = in.native_light_ao.xy;
    @if(!ENHANCED)
    model.native_ao_face = in.native_light_ao.z;
    @if(!ENHANCED)
    model.tint_gamma = in.tint_gamma;
    model.world_origin = in.world_origin;
    model.visibility.y = in.routing.y;
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
    if (in.routing.x != 0u) {
        // Packed liquid corners wind clockwise; this pipeline's front face is counter-clockwise.
        return shade_liquid(liquid_output(in), !front_facing, liquid_dx, liquid_dy);
    }
    return shade_blend(model_output(in), front_facing, model_dx, model_dy);
}
