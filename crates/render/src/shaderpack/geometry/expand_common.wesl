// Iris vertex stream writer shared by the cube, model and liquid expansion kernels. Each job
// writes one or more four-vertex faces; `geometry/vertex.rs` owns the word layout
// (`TerrainLayout`, passed in `ExpandParams::words`), `geometry/jobs.rs` the job encoding,
// `atlas.rs` the sprite slots.
import render::material::{MaterialGpu, texture_layer, texture_page, material_uses_overlay_mask};

struct ExpandParams {
    /// Block position that written vertex positions are relative to.
    anchor: vec4<i32>,
    /// x: AO goes to colour alpha (Iris `separateAo`); y: bake face shading (`oldLighting`);
    /// z: first job word in `job_words`; w: materials in the attribute table before it.
    flags: vec4<u32>,
    /// `TerrainLayout::expand_words`: the vertex stride and each field's word, a byte each,
    /// `LAYOUT_ABSENT` for fields the layout leaves out. x: stride, float position, packed
    /// position, colour; y: uv, mid uv, lightmap, normal; z: tangent, block, mid block.
    words: vec4<u32>,
    /// x: jobs of this dispatch, from word `flags.z` on.
    jobs: vec4<u32>,
}

// The material attribute table (two words per material) followed by every dispatch's
// four-word jobs; one buffer keeps every kernel within the baseline storage-buffer count
// per stage.
@group(1) @binding(0) var<storage, read> job_words: array<u32>;
@group(1) @binding(1) var<storage, read_write> vertices: array<u32>;
@group(1) @binding(2) var<storage, read> atlas_remap: array<vec2<u32>>;
@group(1) @binding(3) var<uniform> params: ExpandParams;

const EXPAND_WORKGROUP_SIZE: u32 = GENERATED_EXPAND_WORKGROUP_SIZE;
const NO_ANIMATION: u32 = 0xffffffffu;
const LAYOUT_ABSENT: u32 = 0xffu;
/// Packed positions: steps per block, and blocks below the section origin of step zero
/// (`vertex.rs`'s `PACKED_POSITION_STEPS` and `PACKED_POSITION_BIAS`).
const PACKED_POSITION_STEPS: f32 = 2048.0;
const PACKED_POSITION_BIAS: f32 = 8.0;
/// Iris's `mc_Entity.y` (`ExtendedDataHelper`): fluids and every other block.
const FLUID_RENDER_TYPE: i32 = 1;
const BLOCK_RENDER_TYPE: i32 = -1;

/// The four corners of one face, in counter-clockwise front-face order.
struct FaceCorners {
    /// Section-local positions.
    positions: array<vec3<f32>, 4>,
    /// Sprite-local texture coordinates in [0, 1].
    uvs: array<vec2<f32>, 4>,
    /// Block and sky light levels, 0 to 15.
    light: array<vec2<f32>, 4>,
    /// Ambient-occlusion brightness, 1 when unoccluded.
    ao: array<f32, 4>,
}

/// What every face of one job shares.
struct FaceSource {
    normal: vec3<f32>,
    /// Gamma-space vertex tint; alpha is the vertex opacity.
    tint: vec4<f32>,
    /// Vanilla directional face coefficient, applied when `params.flags.y` is set.
    face_shade: f32,
    /// Section-local corner of the block the face belongs to.
    block: vec3<f32>,
    chunk_origin: vec3<i32>,
    /// Material after positional variation; selects the sprite and block attributes.
    material_id: u32,
    material: MaterialGpu,
    two_sided: bool,
    /// Liquid surfaces: Iris's fluid render type in `mc_Entity.y`. Kernels leave it at the
    /// zero value, false, for blocks.
    fluid: bool,
}

/// One job's decoded face: its corners and what they share. `valid` is false for a record
/// that failed validation, whose slots stay degenerate.
struct JobFace {
    corners: FaceCorners,
    source: FaceSource,
    valid: bool,
}

/// One Iris vertex as the expanded stream stores it, before packing: `light` is Java's raw
/// lightmap coordinate (level × 16), `block` is `mc_Entity.xy` and `mid_block` `at_midBlock`.
/// `position` is relative to the anchor and `local` to the section origin; packed layouts
/// store `local`.
struct IrisVertex {
    position: vec3<f32>,
    local: vec3<f32>,
    colour: vec4<f32>,
    uv: vec2<f32>,
    mid_uv: vec2<f32>,
    light: vec2<f32>,
    normal: vec3<f32>,
    tangent: vec4<f32>,
    block: vec2<i32>,
    mid_block: vec4<f32>,
}

/// Linear job index of a dispatch that may span a second dimension.
fn expand_job_index(id: vec3<u32>, groups: vec3<u32>) -> u32 {
    return id.x + id.y * groups.x * EXPAND_WORKGROUP_SIZE;
}

fn expand_job_count() -> u32 {
    return params.jobs.x;
}

fn expand_job(index: u32) -> vec4<u32> {
    let word = params.flags.z + index * 4u;
    return vec4(job_words[word], job_words[word + 1u], job_words[word + 2u], job_words[word + 3u]);
}

/// Remap slot of a material's sprite; `opaque` selects the alpha-free overlay base.
fn sprite_slot(material: MaterialGpu, opaque: bool) -> u32 {
    var slot = ATLAS_HEADER_SLOTS;
    if (material.animation != NO_ANIMATION) {
        slot = ATLAS_ANIMATION_SLOT_BASE + material.animation;
    } else {
        let base = select(ATLAS_STATIC_SLOT_BASE, ATLAS_OPAQUE_SLOT_BASE, opaque);
        slot = base + texture_page(material.texture) * ATLAS_PAGE_SLOTS + u32(texture_layer(material.texture));
    }
    return select(ATLAS_HEADER_SLOTS, slot, slot < arrayLength(&atlas_remap));
}

/// Atlas texture coordinates of sprite-local `local` in remap slot `slot`.
fn atlas_uv(slot: u32, local: vec2<f32>) -> vec2<f32> {
    let size = vec2<f32>(atlas_remap[0]);
    let rect = atlas_remap[slot];
    let origin = vec2<f32>(f32(rect.x & 0xffffu), f32(rect.x >> 16u));
    let extent = vec2<f32>(f32(rect.y & 0xffffu), f32(rect.y >> 16u));
    return (origin + clamp(local, vec2(0.0), vec2(1.0)) * extent) / max(size, vec2(1.0));
}

/// `(block id, emission)` of a material: the Java `block.properties` id (-1 when unmapped)
/// and the block's light emission 0 to 15.
fn block_attributes(material_id: u32) -> vec2<i32> {
    if (material_id >= params.flags.w) {
        return vec2(-1, 0);
    }
    return bitcast<vec2<i32>>(vec2(job_words[material_id * 2u], job_words[material_id * 2u + 1u]));
}

/// Tangent with handedness in `w`, from the face's position and texture gradients.
fn face_tangent(positions: array<vec3<f32>, 4>, uvs: array<vec2<f32>, 4>, normal: vec3<f32>) -> vec4<f32> {
    let edge_a = positions[1] - positions[0];
    let edge_b = positions[2] - positions[0];
    let delta_a = uvs[1] - uvs[0];
    let delta_b = uvs[2] - uvs[0];
    let determinant = delta_a.x * delta_b.y - delta_b.x * delta_a.y;
    if (abs(determinant) < 1e-8) {
        let reference = select(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), abs(normal.x) > 0.9);
        return vec4(normalize(cross(reference, normal)), 1.0);
    }
    let tangent = (edge_a * delta_b.y - edge_b * delta_a.y) / determinant;
    let bitangent = (edge_b * delta_a.x - edge_a * delta_b.x) / determinant;
    let orthogonal = tangent - normal * dot(normal, tangent);
    let length_squared = dot(orthogonal, orthogonal);
    let direction = select(normalize(edge_a), orthogonal * inverseSqrt(length_squared), length_squared > 1e-12);
    return vec4(direction, select(1.0, -1.0, dot(cross(normal, direction), bitangent) < 0.0));
}

/// Byte `index` of a `TerrainLayout::expand_words` word.
fn layout_byte(word: u32, index: u32) -> u32 {
    return (word >> (index * 8u)) & 0xffu;
}

/// Writes `value` at `word` of the vertex at `base`, unless the layout leaves it out.
fn put_word(base: u32, word: u32, value: u32) {
    if (word != LAYOUT_ABSENT) {
        vertices[base + word] = value;
    }
}

fn write_vertex(index: u32, vertex: IrisVertex) {
    let words = params.words;
    let base = index * layout_byte(words.x, 0u);
    let position = layout_byte(words.x, 1u);
    if (position != LAYOUT_ABSENT) {
        vertices[base + position] = bitcast<u32>(vertex.position.x);
        vertices[base + position + 1u] = bitcast<u32>(vertex.position.y);
        vertices[base + position + 2u] = bitcast<u32>(vertex.position.z);
    }
    let packed_position = layout_byte(words.x, 2u);
    if (packed_position != LAYOUT_ABSENT) {
        let steps = clamp(
            round((vertex.local + vec3(PACKED_POSITION_BIAS)) * PACKED_POSITION_STEPS),
            vec3(0.0),
            vec3(65535.0),
        );
        let packed = vec3<u32>(steps);
        // The lightmap coordinates ride in `w`: block then sky, a byte each.
        let light = vec2<u32>(clamp(round(vertex.light), vec2(0.0), vec2(255.0)));
        vertices[base + packed_position] = packed.x | (packed.y << 16u);
        vertices[base + packed_position + 1u] = packed.z | (light.x << 16u) | (light.y << 24u);
    }
    put_word(base, layout_byte(words.x, 3u), pack4x8unorm(vertex.colour));
    put_word(base, layout_byte(words.y, 0u), pack2x16unorm(vertex.uv));
    put_word(base, layout_byte(words.y, 1u), pack2x16unorm(vertex.mid_uv));
    put_word(base, layout_byte(words.y, 2u), pack2x16float(vertex.light));
    put_word(base, layout_byte(words.y, 3u), pack4x8snorm(vec4(vertex.normal, 0.0)));
    put_word(base, layout_byte(words.z, 0u), pack4x8snorm(vertex.tangent));
    // Signed shorts and bytes, as Iris stores `mc_Entity` and `at_midBlock`; the mid-block
    // offset truncates towards zero like Iris's `(int)` cast.
    put_word(
        base,
        layout_byte(words.z, 1u),
        (bitcast<u32>(vertex.block.x) & 0xffffu) | (bitcast<u32>(vertex.block.y) << 16u),
    );
    let mid = bitcast<vec4<u32>>(vec4<i32>(vertex.mid_block)) & vec4(0xffu);
    put_word(base, layout_byte(words.z, 2u), mid.x | (mid.y << 8u) | (mid.z << 16u) | (mid.w << 24u));
}

/// A vertex of a face slot that rasterizes nothing: all four corners at `position`
/// (section-local `local`).
fn degenerate_vertex(position: vec3<f32>, local: vec3<f32>) -> IrisVertex {
    var vertex: IrisVertex;
    vertex.position = position;
    vertex.local = local;
    vertex.block = vec2(-1, BLOCK_RENDER_TYPE);
    return vertex;
}

/// Corner `corner` of one face with `tint` and the sprite in remap slot `slot`; `reversed`
/// gives the back side of a two-sided face, with its corners in the opposite order.
fn face_vertex(source_corners: FaceCorners, source: FaceSource, tint: vec4<f32>, slot: u32, reversed: bool, corner: u32) -> IrisVertex {
    var corners = source_corners;
    var order = array<u32, 4>(0u, 1u, 2u, 3u);
    if (reversed) {
        order = array<u32, 4>(0u, 3u, 2u, 1u);
    }
    let normal = select(source.normal, -source.normal, reversed);
    var positions: array<vec3<f32>, 4>;
    var local_uvs: array<vec2<f32>, 4>;
    var mid_uv = vec2(0.0);
    for (var index = 0u; index < 4u; index += 1u) {
        positions[index] = corners.positions[order[index]];
        local_uvs[index] = corners.uvs[order[index]];
        mid_uv += atlas_uv(slot, local_uvs[index]) * 0.25;
    }
    let attributes = block_attributes(source.material_id);
    let separate_ao = params.flags.x != 0u;
    let shade = select(1.0, source.face_shade, params.flags.y != 0u);
    let ao = corners.ao[order[corner]];
    let position = positions[corner];
    var vertex: IrisVertex;
    vertex.position = vec3<f32>(source.chunk_origin - params.anchor.xyz) + position;
    vertex.local = position;
    vertex.colour = vec4(
        tint.rgb * shade * select(ao, 1.0, separate_ao),
        select(tint.a, ao, separate_ao),
    );
    vertex.uv = atlas_uv(slot, local_uvs[corner]);
    vertex.mid_uv = mid_uv;
    vertex.light = clamp(corners.light[order[corner]], vec2(0.0), vec2(15.0)) * 16.0;
    vertex.normal = normal;
    vertex.tangent = face_tangent(positions, local_uvs, normal);
    vertex.block = vec2(attributes.x, select(BLOCK_RENDER_TYPE, FLUID_RENDER_TYPE, source.fluid));
    vertex.mid_block = vec4((source.block + vec3(0.5) - position) * 64.0, f32(attributes.y));
    return vertex;
}

/// Corner `corner` of slot `slot` of the face slots `job` reserved: the face, then an overlay
/// slot when `JOB_OVERLAY_FACE` is set and a back side when `JOB_REVERSE_FACE` is set; a
/// `JOB_BACK_ONLY` job's one slot is the back side alone.
/// Grass-side overlay materials draw an untinted opaque base under a tinted cutout overlay, as
/// Java's overlay quads do; slots the material does not need, and every slot of an invalid
/// record, are degenerate.
fn job_slot_vertex(job: vec4<u32>, slot: u32, face: JobFace, corner: u32) -> IrisVertex {
    let corners = face.corners;
    let source = face.source;
    if (!face.valid) {
        return degenerate_vertex(vec3(0.0), vec3(0.0));
    }
    let degenerate = degenerate_vertex(
        vec3<f32>(source.chunk_origin - params.anchor.xyz) + corners.positions[0],
        corners.positions[0],
    );
    let overlay_slot = (job.w & JOB_OVERLAY_FACE) != 0u;
    let overlay = material_uses_overlay_mask(source.material.flags) && overlay_slot;
    let front = sprite_slot(source.material, false);
    if ((job.w & JOB_BACK_ONLY) != 0u) {
        if (source.two_sided) {
            return face_vertex(corners, source, source.tint, front, true, corner);
        }
        return degenerate;
    }
    if (slot == 0u) {
        if (overlay) {
            return face_vertex(corners, source, vec4(vec3(1.0), source.tint.a), sprite_slot(source.material, true), false, corner);
        }
        return face_vertex(corners, source, source.tint, front, false, corner);
    }
    if (slot == 1u && overlay_slot) {
        if (overlay) {
            return face_vertex(corners, source, source.tint, front, false, corner);
        }
        return degenerate;
    }
    if ((job.w & JOB_REVERSE_FACE) != 0u && source.two_sided) {
        return face_vertex(corners, source, source.tint, front, true, corner);
    }
    return degenerate;
}

/// Writes every face slot `job` reserved, from its first output face `job.z`.
fn write_job_faces(job: vec4<u32>, face: JobFace) {
    let slots = 1u + select(0u, 1u, (job.w & JOB_OVERLAY_FACE) != 0u) + select(0u, 1u, (job.w & JOB_REVERSE_FACE) != 0u);
    for (var slot = 0u; slot < slots; slot += 1u) {
        for (var corner = 0u; corner < 4u; corner += 1u) {
            write_vertex((job.z + slot) * 4u + corner, job_slot_vertex(job, slot, face, corner));
        }
    }
}
