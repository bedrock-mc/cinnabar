
// Projects shared world corners through one camera-relative path for identical clip coordinates.
// Integer block subtraction preserves precision far from the origin.

/// Returns local relative to camera, subtracting whole section blocks as integers first.
/// Shared corners remain identical across sections and precise far from the origin.
fn section_camera_offset(section_origin: vec3<i32>, local: vec3<f32>, camera: vec3<f32>) -> vec3<f32> {
    let camera_block = vec3<i32>(floor(camera));
    return vec3<f32>(section_origin - camera_block) + local - (camera - vec3<f32>(camera_block));
}

/// Clip position of a point `offset` from `camera`.
/// Declare position outputs invariant to preserve matching arithmetic across world passes.
fn camera_offset_clip(clip_from_world: mat4x4<f32>, camera: vec3<f32>, offset: vec3<f32>) -> vec4<f32> {
    return clip_from_world * vec4(offset, 0.0) + clip_from_world * vec4(camera, 1.0);
}

/// Projects an absolute world position through its exact difference from a nearby camera.
/// This keeps overlays aligned with camera-relative terrain.
fn world_clip(clip_from_world: mat4x4<f32>, camera: vec3<f32>, world: vec3<f32>) -> vec4<f32> {
    return camera_offset_clip(clip_from_world, camera, world - camera);
}
