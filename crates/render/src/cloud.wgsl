#import bevy_render::view::View
#import cinnabar::lighting::{tint_to_linear}

struct AtmosphereUniform {
    sun_direction_daylight: vec4<f32>,
    moon_direction_phase: vec4<f32>,
    sky_zenith_rain: vec4<f32>,
    sky_horizon_thunder: vec4<f32>,
    fog_color_start: vec4<f32>,
    fog_end_time: vec4<f32>,
    sunrise_band: vec4<f32>,
    sky_extra: vec4<f32>,
}

struct ViewportCloudQuad {
    cell: vec2<i32>,
    face: u32,
    colour: u32,
}

struct NativeCloudUniform {
    // Native CPU colour and tessellator bytes are gamma-space.
    colour: vec4<f32>,
    // Cell blocks, underside Y, top Y, texture world period; Rust owns them.
    geometry: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> atmosphere: AtmosphereUniform;
@group(0) @binding(2) var<storage, read> cloud_records: array<ViewportCloudQuad>;
@group(0) @binding(3) var<uniform> native_cloud: NativeCloudUniform;

const FACE_DOWN: u32 = CLOUD_FACE_DOWN_VALUE;
const FACE_UP: u32 = CLOUD_FACE_UP_VALUE;
const FACE_NORTH: u32 = CLOUD_FACE_NORTH_VALUE;
const FACE_SOUTH: u32 = CLOUD_FACE_SOUTH_VALUE;
const FACE_WEST: u32 = CLOUD_FACE_WEST_VALUE;
const FACE_EAST: u32 = CLOUD_FACE_EAST_VALUE;
const CLOUD_FADE_START: f32 = CLOUD_FADE_START_VALUE;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec4<f32>,
}

fn distance_fade(world_distance: f32) -> f32 {
    let fade_distance = atmosphere.fog_end_time.w;
    if (fade_distance <= 0.0) {
        return 1.0;
    }
    return clamp(1.0 - max(world_distance / fade_distance - CLOUD_FADE_START, 0.0), 0.0, 1.0);
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

fn face_corner_uv(face: u32, corner_index: u32) -> vec2<f32> {
    // GlobalQuadIndexBuffer triangulates native four-vertex quads.
    let quad_vertex = array<u32, 6>(1u, 2u, 0u, 0u, 2u, 3u)[corner_index];
    let corner = array<vec2<f32>, 4>(
        vec2(0.0, 0.0), vec2(1.0, 0.0),
        vec2(1.0, 1.0), vec2(0.0, 1.0),
    )[quad_vertex];
    // Vanilla's exact face sequences preserve both outward
    // winding and the diagonal over which native vertex fade interpolates.
    if (face == FACE_DOWN) {
        return vec2(1.0 - corner.y, corner.x);
    }
    if (face == FACE_UP) {
        return corner.yx;
    }
    if (face == FACE_NORTH || face == FACE_EAST) {
        return vec2(corner.x, 1.0 - corner.y);
    }
    return corner;
}

fn reconstruct_world_position(record: ViewportCloudQuad, corner: vec2<f32>) -> vec3<f32> {
    let cell = vec2<f32>(record.cell);
    let lower_y = native_cloud.geometry.y;
    let upper_y = native_cloud.geometry.z;
    var local_position: vec3<f32>;
    if (record.face == FACE_DOWN) {
        local_position = vec3(cell.x + corner.x, lower_y, cell.y + corner.y);
    } else if (record.face == FACE_UP) {
        local_position = vec3(cell.x + corner.x, upper_y, cell.y + corner.y);
    } else {
        let y = mix(lower_y, upper_y, corner.y);
        if (record.face == FACE_NORTH) {
            local_position = vec3(cell.x + corner.x, y, cell.y);
        } else if (record.face == FACE_SOUTH) {
            local_position = vec3(cell.x + corner.x, y, cell.y + 1.0);
        } else if (record.face == FACE_WEST) {
            local_position = vec3(cell.x, y, cell.y + corner.x);
        } else {
            local_position = vec3(cell.x + 1.0, y, cell.y + corner.x);
        }
    }
    return vec3(
        local_position.x * native_cloud.geometry.x + atmosphere.fog_end_time.z * native_cloud.geometry.w,
        local_position.y,
        local_position.z * native_cloud.geometry.x,
    );
}

fn native_cloud_vertex_colour(record: ViewportCloudQuad, world_position: vec3<f32>) -> vec4<f32> {
    let baked = vec4(
        f32(record.colour & 0xffu),
        f32((record.colour >> 8u) & 0xffu),
        f32((record.colour >> 16u) & 0xffu),
        f32((record.colour >> 24u) & 0xffu),
    ) / 255.0;
    var colour = baked * native_cloud.colour;
    // Current Clouds material fades the per-texel vertices, not the fragment.
    colour.a *= distance_fade(distance(world_position, view.world_position));
    return colour;
}

@vertex
fn cloud_vertex(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let quad_index = vertex_index / 6u;
    let corner_index = vertex_index % 6u;
    let record = cloud_records[quad_index];
    let world_position = reconstruct_world_position(record, face_corner_uv(record.face, corner_index));
    return VertexOutput(
        view.clip_from_world * vec4(world_position, 1.0),
        native_cloud_vertex_colour(record, world_position),
    );
}

@fragment
fn cloud_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Interpolate native gamma vertex colour first; Bevy's sRGB target performs
    // the inverse transfer on write. Render-target blend-space parity is a
    // separate gate, not an invented cloud fog/light pass.
    return tint_to_linear(in.colour);
}
