#define_import_path cinnabar::enhanced_caster

#import cinnabar::enhanced_common::{FEATURE_WAVING, wave_offset}

// Mirrors `CasterUniformGpu` in enhanced/gpu.rs; one 256-byte slot per cascade.
struct CasterUniform {
    clip_from_world: mat4x4<f32>,
    // x wrapped seconds, y rain level.
    params: vec4<f32>,
    // x feature bits.
    flags: vec4<u32>,
    previous_clip_from_world: mat4x4<f32>,
    // x previous wrapped seconds, y previous near, w submitted history is usable.
    previous_params: vec4<f32>,
    // xyz local emitter centre, w this is a local shadow capture.
    local_light: vec4<f32>,
}

// Shadow-caster bind group appended as group 2 on depth-only pipelines.
@group(2) @binding(0) var<uniform> caster: CasterUniform;
@group(2) @binding(1) var caster_material_classes: texture_2d<u32>;

// Projects a caster after applying the same material wave as the lit pass.
// Apply the same foliage motion before the light projection.
fn caster_clip(world: vec3<f32>, material_id: u32, weight: f32) -> vec4<f32> {
    var position = world;
    let dimensions = textureDimensions(caster_material_classes);
    if ((caster.flags.x & FEATURE_WAVING) != 0u
        && material_id < dimensions.x * dimensions.y) {
        position += wave_offset(
            world,
            textureLoad(caster_material_classes, vec2<i32>(i32(material_id % dimensions.x), i32(material_id / dimensions.x)), 0).r,
            weight,
            caster.params.x,
            caster.params.y,
        );
    }
    return caster.clip_from_world * vec4(position, 1.0);
}

fn caster_previous_clip(world: vec3<f32>, material_id: u32, weight: f32) -> vec4<f32> {
    var position = world;
    let dimensions = textureDimensions(caster_material_classes);
    if ((caster.flags.x & FEATURE_WAVING) != 0u && material_id < dimensions.x * dimensions.y) {
        position += wave_offset(world,
            textureLoad(caster_material_classes, vec2<i32>(i32(material_id % dimensions.x), i32(material_id / dimensions.x)), 0).r,
            weight, caster.previous_params.x, caster.params.y);
    }
    return caster.previous_clip_from_world * vec4(position, 1.0);
}

fn caster_history_valid() -> bool {
    return caster.previous_params.w > 0.5;
}

fn caster_excludes_emitter(world: vec3<f32>) -> bool {
    return caster.local_light.w > 0.5 && all(abs(world - caster.local_light.xyz) < vec3(0.501));
}
