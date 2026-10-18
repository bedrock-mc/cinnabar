import render::lighting::WorldAtmosphere;
import bevy_render::view::View;
import render::lighting::{tint_to_linear};

alias AtmosphereUniform = WorldAtmosphere;

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
    @location(1) distance: f32,
}

/// Initializes cloud varyings by name so new fields have zero defaults in every caller.
fn cloud_vertex_output(position: vec4<f32>, colour: vec4<f32>, distance: f32) -> VertexOutput {
    var out: VertexOutput;
    out.position = position;
    out.colour = colour;
    out.distance = distance;
    return out;
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
    let face = record.face & 7u;
    let lower_y = native_cloud.geometry.y;
    let upper_y = native_cloud.geometry.z;
    var local_position: vec3<f32>;
    if (face == FACE_DOWN) {
        local_position = vec3(cell.x + corner.x, lower_y, cell.y + corner.y);
    } else if (face == FACE_UP) {
        local_position = vec3(cell.x + corner.x, upper_y, cell.y + corner.y);
    } else {
        let y = mix(lower_y, upper_y, corner.y);
        if (face == FACE_NORTH) {
            local_position = vec3(cell.x + corner.x, y, cell.y);
        } else if (face == FACE_SOUTH) {
            local_position = vec3(cell.x + corner.x, y, cell.y + 1.0);
        } else if (face == FACE_WEST) {
            local_position = vec3(cell.x, y, cell.y + corner.x);
        } else {
            local_position = vec3(cell.x + 1.0, y, cell.y + corner.x);
        }
    }
    return vec3(
        local_position.x * native_cloud.geometry.x + atmosphere.fog_end_time.z * native_cloud.geometry.w,
        local_position.y,
        local_position.z * native_cloud.geometry.x - select(0.0, JAVA_CLOUD_Z_OFFSET_VALUE, atmosphere.java_fog.x > 0.0),
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
    if (atmosphere.java_fog.x > 0.0) {
        let face = record.face & 7u;
        var shade = 1.0;
        if (face == FACE_DOWN) { shade = 0.7; }
        else if (face == FACE_NORTH || face == FACE_SOUTH) { shade = 0.8; }
        else if (face == FACE_WEST || face == FACE_EAST) { shade = 0.9; }
        colour = vec4(colour.rgb * shade, native_cloud.colour.a);
    } else {
        colour.a *= distance_fade(distance(world_position, view.world_position));
    }
    return colour;
}

@vertex
fn cloud_vertex(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let quad_index = vertex_index / 6u;
    let corner_index = vertex_index % 6u;
    let record = cloud_records[quad_index];
    var corner = face_corner_uv(record.face, corner_index);
    if (atmosphere.java_fog.x > 0.0) {
        var index = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u)[corner_index];
        if ((record.face & 16u) != 0u) { index = 3u - index; }
        let uv = array<vec2<f32>, 4>(vec2(0.0,0.0),vec2(1.0,0.0),vec2(1.0,1.0),vec2(0.0,1.0))[index];
        let face = record.face & 7u;
        corner = uv.yx;
        if (face == FACE_DOWN || face == FACE_SOUTH || face == FACE_WEST) { corner = vec2(1.0 - uv.y, uv.x); }
    }
    let world_position = reconstruct_world_position(record, corner);
    return cloud_vertex_output(
        view.clip_from_world * vec4(world_position, 1.0),
        native_cloud_vertex_colour(record, world_position),
        distance(world_position, view.world_position),
    );
}

@fragment
fn cloud_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Interpolate encoded vertex colour, then select the active pass's output space.
    var colour = in.colour;
    if (atmosphere.java_fog.x > 0.0) { colour.a *= 1.0 - clamp(in.distance / max(atmosphere.java_sky.z, 0.0001), 0.0, 1.0); }
    @if(JAVA_ENCODED)
    return colour;
    @if(!JAVA_ENCODED)
    return tint_to_linear(colour);
}
