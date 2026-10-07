#define_import_path cinnabar::enhanced_radiance
#import cinnabar::enhanced_common::EnhancedFrame

// Absent authored material maps never imply self-emission.
fn untextured_material_mer() -> vec3<f32> {
    return vec3(0.0, 0.0, 0.8);
}

// Block propagation supplies local light independently of time and display gamma.
fn block_illumination(sample: u32) -> vec3<f32> {
    let level = f32(sample & 15u) / 15.0;
    return vec3(1.0, 0.68, 0.38) * level * level * level * 0.42;
}

fn diffuse_indirect(frame: EnhancedFrame, normal: vec3<f32>, sky: f32, block: vec3<f32>) -> vec3<f32> {
    let upward = clamp(normal.y * 0.5 + 0.5, 0.0, 1.0);
    let hemisphere = mix(vec3(0.35, 0.29, 0.23), frame.ambient_colour.rgb, upward);
    let sky_radiance = hemisphere * frame.light_colour.w * clamp(sky, 0.0, 1.0);
    return sky_radiance + max(block, vec3(0.0));
}

// Occlusion affects incident light; emitted radiance never receives AO or contact shadows.
fn compose_surface_lighting(indirect: vec3<f32>, direct: vec3<f32>, emission: vec3<f32>, visibility: vec2<f32>) -> vec3<f32> {
    let visible = clamp(visibility, vec2(0.0), vec2(1.0));
    return max(indirect, vec3(0.0)) * visible.x + max(direct, vec3(0.0)) * visible.y + max(emission, vec3(0.0));
}

// Converts raw voxel skylight to the shared finite-grid sky access bound.
fn voxel_sky_access(value:f32)->f32 {
    let sky=clamp(value,0.0,1.0);
    return sky/(4.0-3.0*sky);
}
