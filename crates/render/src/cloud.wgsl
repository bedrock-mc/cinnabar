#import bevy_render::view::View

struct AtmosphereUniform {
    sun_direction_daylight: vec4<f32>,
    moon_direction_phase: vec4<f32>,
    sky_zenith_rain: vec4<f32>,
    sky_horizon_thunder: vec4<f32>,
    fog_color_start: vec4<f32>,
    fog_end_time: vec4<f32>,
}

struct PackedCloudQuad {
    bounds: u32,
    face_and_axis: u32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> atmosphere: AtmosphereUniform;
@group(0) @binding(2) var<storage, read> cloud_records: array<PackedCloudQuad>;

const CLOUD_UNDERSIDE_Y: f32 = 128.0;
const CLOUD_TOP_Y: f32 = 132.0;
const CLOUD_TEXTURE_WORLD_PERIOD: f32 = 256.0;
const FACE_DOWN: u32 = 0u;
const FACE_UP: u32 = 1u;
const FACE_NORTH: u32 = 2u;
const FACE_SOUTH: u32 = 3u;
const FACE_WEST: u32 = 4u;
const CLOUD_DIRECTIONAL_AMBIENT: f32 = 0.55;
const PROVISIONAL_CLOUD_NIGHT_FLOOR: f32 = 0.2;
const RAIN_CLOUD_COLOUR: vec3<f32> = vec3(191.0 / 255.0);
const THUNDER_CLOUD_COLOUR: vec3<f32> = vec3(30.0 / 255.0);
const WEATHER_COLOUR_CONTRIBUTION: f32 = 0.95;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) @interpolate(flat) normal: vec3<f32>,
}

fn face_normal(face: u32) -> vec3<f32> {
    if (face == FACE_DOWN) {
        return vec3(0.0, -1.0, 0.0);
    }
    if (face == FACE_UP) {
        return vec3(0.0, 1.0, 0.0);
    }
    if (face == FACE_NORTH) {
        return vec3(0.0, 0.0, -1.0);
    }
    if (face == FACE_SOUTH) {
        return vec3(0.0, 0.0, 1.0);
    }
    if (face == FACE_WEST) {
        return vec3(-1.0, 0.0, 0.0);
    }
    return vec3(1.0, 0.0, 0.0);
}

fn invalid_cloud_fog_input(value: f32) -> bool {
    return (bitcast<u32>(value) & 0x7f800000u) == 0x7f800000u;
}

fn bounded_cloud_fog(world_distance: f32, fog_start: f32, fog_end: f32) -> f32 {
    if (invalid_cloud_fog_input(world_distance)
        || invalid_cloud_fog_input(fog_start)
        || invalid_cloud_fog_input(fog_end)) {
        return 1.0;
    }
    let bounded_distance = max(world_distance, 0.0);
    let bounded_start = clamp(fog_start, 0.0, CLOUD_TEXTURE_WORLD_PERIOD - 1.0);
    let bounded_end = clamp(fog_end, 0.0, CLOUD_TEXTURE_WORLD_PERIOD - 1.0);
    if (bounded_end <= bounded_start) {
        return select(0.0, 1.0, bounded_distance >= bounded_end);
    }
    let amount = clamp(
        (bounded_distance - bounded_start) / (bounded_end - bounded_start),
        0.0,
        1.0,
    );
    return amount * amount * (3.0 - 2.0 * amount);
}

fn corner_uv(corner_index: u32) -> vec2<f32> {
    return array<vec2<f32>, 6>(
        vec2(0.0, 0.0),
        vec2(1.0, 0.0),
        vec2(1.0, 1.0),
        vec2(0.0, 0.0),
        vec2(1.0, 1.0),
        vec2(0.0, 1.0),
    )[corner_index];
}

fn reconstruct_local_position(record: PackedCloudQuad, corner: vec2<f32>) -> vec3<f32> {
    let axis0_start = f32(record.bounds & 0xffu);
    let axis1_start = f32((record.bounds >> 8u) & 0xffu);
    let axis0_extent = f32(((record.bounds >> 16u) & 0xffu) + 1u);
    let axis1_extent = f32(((record.bounds >> 24u) & 0xffu) + 1u);
    let face = record.face_and_axis & 0x7u;

    let x = mix(axis0_start, axis0_start + axis0_extent, corner.x);
    let z = mix(axis1_start, axis1_start + axis1_extent, corner.y);
    if (face == FACE_DOWN) {
        return vec3(x, CLOUD_UNDERSIDE_Y, z);
    }
    if (face == FACE_UP) {
        return vec3(x, CLOUD_TOP_Y, z);
    }

    let run = mix(axis0_start, axis0_start + axis0_extent, corner.x);
    let y = mix(CLOUD_UNDERSIDE_Y, CLOUD_TOP_Y, corner.y);
    if (face == FACE_NORTH) {
        return vec3(run, y, axis1_start);
    }
    if (face == FACE_SOUTH) {
        return vec3(run, y, axis1_start);
    }
    if (face == FACE_WEST) {
        return vec3(axis1_start, y, run);
    }
    return vec3(axis1_start, y, run);
}

@vertex
fn cloud_vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let quad_index = vertex_index / 6u;
    let corner_index = vertex_index % 6u;
    let record = cloud_records[quad_index];
    let local_position = reconstruct_local_position(record, corner_uv(corner_index));

    let cloud_texture_offset = atmosphere.fog_end_time.z * CLOUD_TEXTURE_WORLD_PERIOD;
    let center_x = floor((view.world_position.x - cloud_texture_offset) / CLOUD_TEXTURE_WORLD_PERIOD)
        * CLOUD_TEXTURE_WORLD_PERIOD + cloud_texture_offset;
    let center_z = floor(view.world_position.z / CLOUD_TEXTURE_WORLD_PERIOD)
        * CLOUD_TEXTURE_WORLD_PERIOD;
    let instance_column = i32(instance_index % 3u) - 1;
    let instance_row = i32(instance_index / 3u) - 1;
    let instance_origin = vec2(
        center_x + f32(instance_column) * CLOUD_TEXTURE_WORLD_PERIOD,
        center_z + f32(instance_row) * CLOUD_TEXTURE_WORLD_PERIOD,
    );
    let world_position = local_position + vec3(instance_origin.x, 0.0, instance_origin.y);

    var out: VertexOutput;
    out.position = view.clip_from_world * vec4(world_position, 1.0);
    out.world_position = world_position;
    out.normal = face_normal(record.face_and_axis & 0x7u);
    return out;
}

@fragment
fn cloud_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let rain = atmosphere.sky_zenith_rain.w;
    let thunder = atmosphere.sky_horizon_thunder.w;
    let rain_colour = mix(
        vec3(1.0),
        RAIN_CLOUD_COLOUR,
        clamp(rain, 0.0, 1.0) * WEATHER_COLOUR_CONTRIBUTION,
    );
    let weather_colour = mix(
        rain_colour,
        THUNDER_CLOUD_COLOUR,
        clamp(thunder, 0.0, 1.0) * WEATHER_COLOUR_CONTRIBUTION,
    );
    let sun_direction = normalize(atmosphere.sun_direction_daylight.xyz);
    let directional = max(dot(in.normal, sun_direction), 0.0);
    let illuminance = max(
        clamp(atmosphere.sun_direction_daylight.w, 0.0, 1.0),
        PROVISIONAL_CLOUD_NIGHT_FLOOR,
    )
        * mix(CLOUD_DIRECTIONAL_AMBIENT, 1.0, directional);
    let cloud_colour = weather_colour * illuminance;

    let world_distance = distance(in.world_position, view.world_position);
    let fog = bounded_cloud_fog(
        world_distance,
        atmosphere.fog_color_start.w,
        atmosphere.fog_end_time.x,
    );
    let fogged_colour = mix(cloud_colour, atmosphere.fog_color_start.rgb, fog);
    let cloud_alpha = clamp(1.0 - fog, 0.0, 1.0);
    return vec4(fogged_colour, cloud_alpha);
}
