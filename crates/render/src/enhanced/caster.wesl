
import render::enhanced::common::{FEATURE_WAVING, wave_offset};

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

// The lit pass's foliage wave at `seconds`, or zero when waving is off or the class is unknown.
fn caster_wave(world: vec3<f32>, material_id: u32, weight: f32, seconds: f32) -> vec3<f32> {
    let dimensions = textureDimensions(caster_material_classes);
    if ((caster.flags.x & FEATURE_WAVING) == 0u || material_id >= dimensions.x * dimensions.y) {
        return vec3(0.0);
    }
    return wave_offset(
        world,
        textureLoad(caster_material_classes, vec2<i32>(i32(material_id % dimensions.x), i32(material_id / dimensions.x)), 0).r,
        weight,
        seconds,
        caster.params.y,
    );
}

// Projects a caster after applying the same material wave as the lit pass.
fn caster_clip(world: vec3<f32>, material_id: u32, weight: f32) -> vec4<f32> {
    return caster.clip_from_world * vec4(world + caster_wave(world, material_id, weight, caster.params.x), 1.0);
}

// Camera-prepass projection of `offset` from `camera`, matching the main pass's
// camera-relative precision far from the origin.
fn caster_camera_clip(world: vec3<f32>, camera: vec3<f32>, offset: vec3<f32>, material_id: u32, weight: f32) -> vec4<f32> {
    let wave = caster_wave(world, material_id, weight, caster.params.x);
    return caster.clip_from_world * vec4(offset + wave, 0.0) + caster.clip_from_world * vec4(camera, 1.0);
}

// The previous-frame endpoint of `caster_camera_clip` for motion vectors.
fn caster_previous_camera_clip(world: vec3<f32>, camera: vec3<f32>, offset: vec3<f32>, material_id: u32, weight: f32) -> vec4<f32> {
    let wave = caster_wave(world, material_id, weight, caster.previous_params.x);
    return caster.previous_clip_from_world * vec4(offset + wave, 0.0)
        + caster.previous_clip_from_world * vec4(camera, 1.0);
}

fn caster_history_valid() -> bool {
    return caster.previous_params.w > 0.5;
}

fn caster_excludes_emitter(world: vec3<f32>) -> bool {
    return caster.local_light.w > 0.5 && all(abs(world - caster.local_light.xyz) < vec3(0.501));
}
