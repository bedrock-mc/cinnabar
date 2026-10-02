#define_import_path cinnabar::lighting

@group(1) @binding(0) var<uniform> world_lightmap: array<vec4<f32>, 256>;

// Both nibbles address the same environment table in every ordinary world pass.
fn light_colour(sample: u32) -> vec3<f32> {
    return world_lightmap[sample & 255u].rgb;
}

// Kept separate from the lightmap: AO shades geometry, not light coordinates.
fn light_ao_factor(level: u32) -> f32 {
    return 1.0 - f32(min(level, 4u)) * 0.2;
}

// Vertex colors interpolate the lightmap result rather than nonlinear light levels.
fn lit_colour(colour: vec3<f32>, lighting: vec3<f32>) -> vec3<f32> {
    return colour * lighting;
}

// Lens 1.26.50.26 0x6a07d80: ordinary and emitting face coefficients.
fn face_shade(normal: vec3<f32>, emitting: bool) -> f32 {
    if normal.y < -0.5 { return select(0.5, 0.875, emitting); }
    if abs(normal.x) > 0.5 { return select(0.6, 0.9, emitting); }
    if abs(normal.z) > 0.5 { return select(0.8, 0.95, emitting); }
    return 1.0;
}

struct WorldAtmosphere {
    sun_direction_daylight: vec4<f32>, moon_direction_phase: vec4<f32>,
    sky_zenith_rain: vec4<f32>, sky_horizon_thunder: vec4<f32>,
    fog_color_start: vec4<f32>, fog_end_time: vec4<f32>,
    sunrise_band: vec4<f32>, sky_extra: vec4<f32>,
}
@group(1) @binding(1) var<uniform> world_atmosphere: WorldAtmosphere;

// Uses the same distance fade as the terrain passes after lighting and overlays.
fn world_distance_fog(colour: vec3<f32>, position: vec3<f32>, camera: vec3<f32>) -> vec3<f32> {
    let start = world_atmosphere.fog_color_start.w;
    let end = world_atmosphere.fog_end_time.x;
    let fog = clamp((distance(position, camera) - start) / max(end - start, 0.0001), 0.0, 1.0);
    return mix(colour, world_atmosphere.fog_color_start.rgb, fog);
}
