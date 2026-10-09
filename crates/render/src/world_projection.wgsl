#define_import_path cinnabar::world_projection

// World passes that share corners or depth with terrain project through
// these functions with their view's `clip_from_world` and camera position.
// Equal world positions then reach bit-identical clip coordinates in every
// pass, without the large-coordinate rounding that moves each vertex alone.

/// Position of `local` in the section at `section_origin`, relative to
/// `camera`. Whole blocks are subtracted as integers first, so a world corner
/// is bit-identical from every section and stays precise far from the origin.
fn section_camera_offset(section_origin: vec3<i32>, local: vec3<f32>, camera: vec3<f32>) -> vec3<f32> {
    let camera_block = vec3<i32>(floor(camera));
    return vec3<f32>(section_origin - camera_block) + local - (camera - vec3<f32>(camera_block));
}

/// Clip position of a point `offset` from `camera`.
fn camera_offset_clip(clip_from_world: mat4x4<f32>, camera: vec3<f32>, offset: vec3<f32>) -> vec4<f32> {
    return clip_from_world * vec4(offset, 0.0) + clip_from_world * vec4(camera, 1.0);
}

/// Clip position of an absolute world position. Its difference from a
/// nearby camera is exact, so geometry given absolute positions, such as
/// overlays, stays as precise as the terrain under it.
fn world_clip(clip_from_world: mat4x4<f32>, camera: vec3<f32>, world: vec3<f32>) -> vec4<f32> {
    return camera_offset_clip(clip_from_world, camera, world - camera);
}
