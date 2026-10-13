// Far terrain: blocky tiles beyond near terrain, pulled from packed quads and lit like a
// sky-lit terrain face.

import bevy_render::view::View;
import render::lighting::{actor_distance_fog, face_shade, terrain_light_colour, tint_to_linear};
import render::world_projection::{camera_offset_clip, section_camera_offset};

struct FarTile {
    origin_x: i32,
    origin_z: i32,
    level: u32,
    padding: u32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> quads: array<vec2<u32>>;
@group(0) @binding(2) var<storage, read> tiles: array<FarTile>;
@group(0) @binding(3) var colours: texture_2d_array<f32>;
// Origin x, origin z and side in columns, one unused word, then one bit per near column.
@group(0) @binding(4) var<storage, read> near_columns: array<u32>;

struct VertexOutput {
    @builtin(position) @invariant position: vec4<f32>,
    @location(0) offset: vec3<f32>,
    @location(1) cell: vec2<f32>,
    @location(2) @interpolate(flat) light: vec3<f32>,
    @location(3) @interpolate(flat) layer: u32,
    @location(4) @interpolate(flat) shade: f32,
}

fn near_column(column: vec2<i32>) -> bool {
    let origin = vec2(bitcast<i32>(near_columns[0]), bitcast<i32>(near_columns[1]));
    let side = i32(near_columns[2]);
    let local = column - origin;
    if (any(local < vec2(0)) || any(local >= vec2(side))) {
        return false;
    }
    let index = u32(local.y * side + local.x);
    return (near_columns[4u + index / 32u] & (1u << (index % 32u))) != 0u;
}

@vertex
fn far_vertex(@builtin(vertex_index) vertex: u32, @builtin(instance_index) slot: u32) -> VertexOutput {
    let quad = quads[vertex / 6u];
    var triangles = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
    let corner = triangles[vertex % 6u];
    let tile = tiles[slot];
    let x = f32(quad.x & 127u);
    let z = f32((quad.x >> 7u) & 127u);
    let w = f32((quad.x >> 14u) & 127u);
    let l = f32((quad.x >> 21u) & 127u);
    let face = quad.x >> 28u;
    let top = f32(bitcast<i32>(quad.y << 16u) >> 16u);
    let bottom = f32(bitcast<i32>(quad.y) >> 16u);

    // Corners run counter-clockwise seen from outside; xz in cells, y in blocks.
    var corners: array<vec3<f32>, 4>;
    var normal = vec3(0.0, 1.0, 0.0);
    var owner = vec2(x, z);
    switch face {
        case FAR_TERRAIN_FACE_POSITIVE_X: {
            corners = array(vec3(x, bottom, z + l), vec3(x, bottom, z), vec3(x, top, z), vec3(x, top, z + l));
            normal = vec3(1.0, 0.0, 0.0);
            owner = vec2(x - 1.0, z);
        }
        case FAR_TERRAIN_FACE_NEGATIVE_X: {
            corners = array(vec3(x, bottom, z), vec3(x, bottom, z + l), vec3(x, top, z + l), vec3(x, top, z));
            normal = vec3(-1.0, 0.0, 0.0);
        }
        case FAR_TERRAIN_FACE_POSITIVE_Z: {
            corners = array(vec3(x, bottom, z), vec3(x + w, bottom, z), vec3(x + w, top, z), vec3(x, top, z));
            normal = vec3(0.0, 0.0, 1.0);
            owner = vec2(x, z - 1.0);
        }
        case FAR_TERRAIN_FACE_NEGATIVE_Z: {
            corners = array(vec3(x + w, bottom, z), vec3(x, bottom, z), vec3(x, top, z), vec3(x + w, top, z));
            normal = vec3(0.0, 0.0, -1.0);
        }
        default: {
            corners = array(vec3(x, top, z), vec3(x, top, z + l), vec3(x + w, top, z + l), vec3(x + w, top, z));
        }
    }
    let local = corners[corner];
    let cell_blocks = f32(1u << tile.level);
    let origin = vec2(tile.origin_x, tile.origin_z);

    var out: VertexOutput;
    // Only finest tiles border near terrain; their quads never cross a column.
    let owner_block = origin + vec2<i32>(owner * cell_blocks);
    if (tile.level == 0u && near_column(vec2(owner_block.x >> 4u, owner_block.y >> 4u))) {
        out.position = vec4(0.0, 0.0, 2.0, 1.0);
        return out;
    }
    let block = vec3(local.x * cell_blocks, local.y, local.z * cell_blocks);
    out.offset = section_camera_offset(vec3(origin.x, 0, origin.y), block, view.world_position);
    out.position = camera_offset_clip(view.clip_from_world, view.world_position, out.offset);
    // Sample the cell that owns the face: walls belong to the higher cell behind them.
    out.cell = local.xz - normal.xz * 0.5;
    out.light = terrain_light_colour(vec2(0.0, 15.0));
    out.layer = slot;
    out.shade = face_shade(normal, false);
    return out;
}

@fragment
fn far_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let last = i32(FAR_TERRAIN_TILE_CELLS) - 1;
    let cell = clamp(vec2<i32>(floor(in.cell)), vec2(0), vec2(last));
    let colour = textureLoad(colours, cell, i32(in.layer), 0).rgb;
    let lit = colour * in.shade * in.light;
    let fogged = actor_distance_fog(lit, in.offset, vec3(0.0));
    return tint_to_linear(vec4(fogged, 1.0));
}
