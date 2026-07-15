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
const SIDE_LIGHT: f32 = 0.78;
const UNDERSIDE_LIGHT: f32 = 0.58;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) @interpolate(flat) face: u32,
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
    out.face = record.face_and_axis & 0x7u;
    return out;
}

@fragment
fn cloud_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let rain = atmosphere.sky_zenith_rain.w;
    let thunder = atmosphere.sky_horizon_thunder.w;
    let storm = clamp(rain * 0.7 + thunder * 0.3, 0.0, 1.0);
    let bounded_storm_colour = mix(
        atmosphere.sky_zenith_rain.rgb,
        atmosphere.sky_horizon_thunder.rgb,
        thunder,
    );
    let top_colour = mix(vec3(1.0), bounded_storm_colour, storm);
    let face_light = select(
        select(SIDE_LIGHT, 1.0, in.face == FACE_UP),
        UNDERSIDE_LIGHT,
        in.face == FACE_DOWN,
    );
    let cloud_colour = top_colour * face_light;

    let world_distance = distance(in.world_position, view.world_position);
    let bounded_fog_end = min(atmosphere.fog_end_time.x, CLOUD_TEXTURE_WORLD_PERIOD - 1.0);
    let fog = smoothstep(atmosphere.fog_color_start.w, bounded_fog_end, world_distance);
    let fogged_colour = mix(cloud_colour, atmosphere.fog_color_start.rgb, fog);
    return vec4(fogged_colour, 1.0);
}
