#import bevy_render::view::View

struct PackedQuad {
    geometry: u32,
    material_id: u32,
}

struct ChunkOrigin {
    value: vec4<i32>,
}

struct MaterialGpu {
    layer: u32,
    flags: u32,
}

struct BiomeTintGpu {
    grass: u32,
    foliage: u32,
    water: u32,
    flags: u32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> quads: array<PackedQuad>;
@group(0) @binding(2) var<storage, read> chunk_origins: array<ChunkOrigin>;
@group(0) @binding(3) var<storage, read> materials: array<MaterialGpu>;
@group(0) @binding(4) var block_textures: texture_2d_array<f32>;
@group(0) @binding(5) var block_sampler: sampler;
@group(0) @binding(6) var<storage, read> biome_records: array<u32>;
@group(0) @binding(7) var<storage, read> biome_tints: array<BiomeTintGpu>;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: u32,
    @location(2) normal: vec3<f32>,
    @location(3) @interpolate(flat) material_flags: u32,
    @location(4) local_position: vec3<f32>,
    @location(5) @interpolate(flat) biome_record: u32,
}

fn quad_corner(face: u32, corner: u32, origin: vec3<f32>, width: f32, height: f32) -> vec3<f32> {
    var corners = array<vec3<f32>, 4>(origin, origin, origin, origin);
    switch face {
        case 0u: {
            corners = array<vec3<f32>, 4>(
                origin,
                origin + vec3(0.0, 0.0, width),
                origin + vec3(0.0, height, width),
                origin + vec3(0.0, height, 0.0),
            );
        }
        case 1u: {
            let base = origin + vec3(1.0, 0.0, 0.0);
            corners = array<vec3<f32>, 4>(
                base,
                base + vec3(0.0, height, 0.0),
                base + vec3(0.0, height, width),
                base + vec3(0.0, 0.0, width),
            );
        }
        case 2u: {
            corners = array<vec3<f32>, 4>(
                origin,
                origin + vec3(width, 0.0, 0.0),
                origin + vec3(width, 0.0, height),
                origin + vec3(0.0, 0.0, height),
            );
        }
        case 3u: {
            let base = origin + vec3(0.0, 1.0, 0.0);
            corners = array<vec3<f32>, 4>(
                base,
                base + vec3(0.0, 0.0, height),
                base + vec3(width, 0.0, height),
                base + vec3(width, 0.0, 0.0),
            );
        }
        case 4u: {
            corners = array<vec3<f32>, 4>(
                origin,
                origin + vec3(0.0, height, 0.0),
                origin + vec3(width, height, 0.0),
                origin + vec3(width, 0.0, 0.0),
            );
        }
        default: {
            let base = origin + vec3(0.0, 0.0, 1.0);
            corners = array<vec3<f32>, 4>(
                base,
                base + vec3(width, 0.0, 0.0),
                base + vec3(width, height, 0.0),
                base + vec3(0.0, height, 0.0),
            );
        }
    }
    return corners[corner];
}

fn face_normal(face: u32) -> vec3<f32> {
    switch face {
        case 0u: { return vec3(-1.0, 0.0, 0.0); }
        case 1u: { return vec3(1.0, 0.0, 0.0); }
        case 2u: { return vec3(0.0, -1.0, 0.0); }
        case 3u: { return vec3(0.0, 1.0, 0.0); }
        case 4u: { return vec3(0.0, 0.0, -1.0); }
        default: { return vec3(0.0, 0.0, 1.0); }
    }
}

fn greedy_uv(face: u32, corner: u32, width: f32, height: f32, flags: u32) -> vec2<f32> {
    let horizontal_standard = array<vec2<f32>, 4>(
        vec2(0.0, 0.0), vec2(width, 0.0),
        vec2(width, height), vec2(0.0, height),
    );
    let horizontal_transposed = array<vec2<f32>, 4>(
        vec2(0.0, 0.0), vec2(0.0, height),
        vec2(width, height), vec2(width, 0.0),
    );
    let vertical_standard = array<vec2<f32>, 4>(
        vec2(0.0, height), vec2(width, height),
        vec2(width, 0.0), vec2(0.0, 0.0),
    );
    let vertical_transposed = array<vec2<f32>, 4>(
        vec2(0.0, height), vec2(0.0, 0.0),
        vec2(width, 0.0), vec2(width, height),
    );
    var uv = horizontal_standard[corner];
    switch face {
        case 0u, 5u: { uv = vertical_standard[corner]; }
        case 1u, 4u: { uv = vertical_transposed[corner]; }
        case 3u: { uv = horizontal_transposed[corner]; }
        default: {}
    }

    var extents = vec2(width, height);
    switch flags & 3u {
        case 1u: {
            uv = vec2(uv.y, width - uv.x);
            extents = vec2(height, width);
        }
        case 2u: {
            uv = vec2(width - uv.x, height - uv.y);
        }
        case 3u: {
            uv = vec2(height - uv.y, uv.x);
            extents = vec2(height, width);
        }
        default: {}
    }
    if ((flags & 4u) != 0u) {
        uv.x = extents.x - uv.x;
    }
    if ((flags & 8u) != 0u) {
        uv.y = extents.y - uv.y;
    }
    return uv;
}

@vertex
fn vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let quad = quads[instance_index];
    let geometry = quad.geometry;
    let local_origin = vec3<f32>(
        f32(geometry & 0x1fu),
        f32((geometry >> 5u) & 0x1fu),
        f32((geometry >> 10u) & 0x1fu),
    );
    let face = (geometry >> 15u) & 0x7u;
    let width = f32(((geometry >> 18u) & 0xfu) + 1u);
    let height = f32(((geometry >> 22u) & 0xfu) + 1u);
    let metadata_index = vertex_index / 4u;
    let corner = vertex_index & 3u;
    let chunk_origin = chunk_origins[metadata_index];
    let local_position = quad_corner(face, corner, local_origin, width, height);
    let world_position = vec3<f32>(chunk_origin.value.xyz) + local_position;
    let material = materials[quad.material_id];

    var out: VertexOutput;
    out.clip_position = view.clip_from_world * vec4(world_position, 1.0);
    out.uv = greedy_uv(face, corner, width, height, material.flags);
    out.layer = material.layer;
    out.normal = face_normal(face);
    out.material_flags = material.flags;
    out.local_position = local_position;
    out.biome_record = u32(chunk_origin.value.w);
    return out;
}

fn unpack_linear_rgb10(packed: u32) -> vec3<f32> {
    return vec3<f32>(
        f32(packed & 0x3ffu),
        f32((packed >> 10u) & 0x3ffu),
        f32((packed >> 20u) & 0x3ffu),
    ) / 1023.0;
}

fn packed_biome_tint_index(record: u32, coordinate: vec3<u32>) -> u32 {
    let header = biome_records[record];
    let bits = header & 0xffu;
    let palette_len = (header >> 8u) & 0x1fffu;
    if (palette_len == 0u) {
        return 0u;
    }

    var packed_word_count = 0u;
    var palette_index = 0u;
    if (bits != 0u) {
        let values_per_word = 32u / bits;
        packed_word_count = (4096u + values_per_word - 1u) / values_per_word;
        let linear = (coordinate.x << 8u) | (coordinate.z << 4u) | coordinate.y;
        let word = biome_records[record + 1u + linear / values_per_word];
        let shift = (linear % values_per_word) * bits;
        let mask = (1u << bits) - 1u;
        palette_index = (word >> shift) & mask;
    }
    if (palette_index >= palette_len) {
        return 0u;
    }
    return biome_records[record + 1u + packed_word_count + palette_index];
}

fn biome_tint(
    tint_kind: u32,
    record: u32,
    local_position: vec3<f32>,
    normal: vec3<f32>,
) -> vec3<f32> {
    let inward_position = floor(local_position - normal * 0.001);
    let coordinate = vec3<u32>(clamp(inward_position, vec3(0.0), vec3(15.0)));
    let requested = packed_biome_tint_index(record, coordinate);
    let tint_count = arrayLength(&biome_tints);
    let tint_index = select(0u, requested, requested < tint_count);
    let tint = biome_tints[tint_index];
    if (tint_kind == 0x10u) {
        return unpack_linear_rgb10(tint.grass);
    }
    if (tint_kind == 0x20u) {
        return unpack_linear_rgb10(tint.foliage);
    }
    return unpack_linear_rgb10(tint.water);
}

fn apply_material_tint(
    sampled: vec4<f32>,
    material_flags: u32,
    biome_record: u32,
    local_position: vec3<f32>,
    normal: vec3<f32>,
) -> vec4<f32> {
    let tint_kind = material_flags & 0x30u;
    if (tint_kind != 0u) {
        let tinted = sampled.rgb * biome_tint(tint_kind, biome_record, local_position, normal);
        if ((material_flags & (1u << 6u)) != 0u) {
            // Grass-side alpha is an overlay weight, not transparency. Its
            // alpha-zero RGB contains the opaque dirt base.
            return vec4(mix(sampled.rgb, tinted, sampled.a), 1.0);
        }
        return vec4(tinted, 1.0);
    }
    return vec4(sampled.rgb, 1.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(block_textures, block_sampler, in.uv, i32(in.layer));
    if ((in.material_flags & (1u << 8u)) != 0u && sampled.a < 0.5) {
        discard;
    }
    return apply_material_tint(
        sampled,
        in.material_flags,
        in.biome_record,
        in.local_position,
        in.normal,
    );
}
