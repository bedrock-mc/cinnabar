// Skins actor rig instances (the world's actors and the first-person hand) and converts
// block-entity vertices into the Iris vertex stream shader-pack gbuffers programs read.
// Skinning is the actor pass's own (`render::actor_rig`); this kernel only changes where the
// posed vertices go: camera-relative world space (or hand view space), one Iris vertex each,
// with an explicit back face per two-sided triangle so pack pipelines cull like Java.
import render::actor_rig::{BoneMatrix, posed_point, posed_normal, placed_point, placed_normal};

struct GeometrySpan {
    first_vertex: u32,
    vertex_count: u32,
}

// One instance to skin: its triangles land at `first_output` (front faces) and
// `first_output + vertex_count` (back faces, degenerate where the face is one-sided).
struct SkinJob {
    instance: u32,
    first_output: u32,
    vertex_count: u32,
    flags: u32,
    // Block light in bits 0..4, sky light in bits 4..8.
    light: u32,
    // Gamma RGBA8 vertex colour.
    colour: u32,
    // `mc_Entity.x`.
    block_id: i32,
    first_output_high: u32,
}

struct SkinParams {
    // World-space jobs are written relative to this camera position.
    camera: vec4<f32>,
    // Rig dispatches: x = jobs. Block-entity dispatches: x = vertices to convert, y = their
    // first output vertex, z = their mc_Entity bits.
    counts: vec4<u32>,
}

const SKIN_WORKGROUP_SIZE: u32 = 64u;
// The job's positions are already in view space (the first-person hand).
const JOB_VIEW_SPACE: u32 = 1u;
// The instance's material culls back faces even where the rig authors them.
const JOB_ONE_SIDED: u32 = 2u;
// Glint jobs have separate vertex colours and sprite-local UVs.
const JOB_GLINT: u32 = JOB_GLINT_FLAG;

@group(0) @binding(0) var<storage, read> instance_words: array<u32>;
@group(0) @binding(1) var<storage, read> vertex_words: array<u32>;
@group(0) @binding(2) var<storage, read> geometry_spans: array<GeometrySpan>;
@group(0) @binding(3) var<storage, read> previous_bones: array<BoneMatrix>;
@group(0) @binding(4) var<storage, read> current_bones: array<BoneMatrix>;
@group(0) @binding(5) var<storage, read> block_entity_words: array<u32>;

@group(1) @binding(0) var<storage, read> jobs: array<SkinJob>;
@group(1) @binding(1) var<storage, read_write> vertices: array<u32>;
@group(1) @binding(2) var<uniform> params: SkinParams;

struct Corner {
    position: vec3<f32>,
    normal: vec3<f32>,
    uv: vec2<f32>,
    back_uv: vec2<f32>,
}

fn word_f32(index: u32) -> f32 {
    return bitcast<f32>(instance_words[index]);
}

fn instance_row(base: u32, row: u32) -> vec4<f32> {
    let offset = base + row * 4u;
    return vec4(word_f32(offset), word_f32(offset + 1u), word_f32(offset + 2u), word_f32(offset + 3u));
}

fn rig_f32(index: u32) -> f32 {
    return bitcast<f32>(vertex_words[index]);
}

// One rig vertex posed as the actor pass poses it, with the instance's uv_anim applied.
fn skin_corner(job: SkinJob, vertex: u32) -> Corner {
    let instance_base = job.instance * ACTOR_GPU_INSTANCE_WORDS;
    let previous_bone_base = instance_words[instance_base + 12u];
    let current_bone_base = instance_words[instance_base + 13u];
    let span = geometry_spans[instance_words[instance_base + 14u]];
    let partial_tick = clamp(word_f32(instance_base + 16u), 0.0, 1.0);
    let uv_offset = vec2(word_f32(instance_base + 20u), word_f32(instance_base + 21u));
    let uv_scale = vec2(word_f32(instance_base + 22u), word_f32(instance_base + 23u));
    let base = (span.first_vertex + vertex) * ACTOR_RIG_VERTEX_WORDS;
    let local = vec3(rig_f32(base), rig_f32(base + 1u), rig_f32(base + 2u));
    let local_normal = vec3(rig_f32(base + 3u), rig_f32(base + 4u), rig_f32(base + 5u));
    let bone_index = vertex_words[base + 10u];
    let previous_bone = previous_bones[previous_bone_base + bone_index];
    let current_bone = current_bones[current_bone_base + bone_index];
    let placement = BoneMatrix(
        instance_row(instance_base, 0u),
        instance_row(instance_base, 1u),
        instance_row(instance_base, 2u),
    );
    var corner: Corner;
    corner.position = placed_point(placement, posed_point(previous_bone, current_bone, local, partial_tick));
    if ((job.flags & JOB_VIEW_SPACE) == 0u) {
        corner.position -= params.camera.xyz;
    }
    corner.normal = placed_normal(placement, posed_normal(previous_bone, current_bone, local_normal, partial_tick));
    corner.uv = uv_offset + vec2(rig_f32(base + 6u), rig_f32(base + 7u)) * uv_scale;
    let raw_back_uv = vec2(rig_f32(base + 8u), rig_f32(base + 9u));
    // A one-sided plane's back keeps its sentinel.
    corner.back_uv = select(uv_offset + raw_back_uv * uv_scale, raw_back_uv, raw_back_uv.x < -1.0e8);
    let glint_base = instance_base + ACTOR_GLINT_WORD;
    let item_extent = word_f32(glint_base + 5u);
    if ((job.flags & JOB_GLINT) != 0u && item_extent > 0.0) {
        let origin = uv_offset + vec2(
            rig_f32(base + ACTOR_RIG_VERTEX_WORDS - 2u),
            rig_f32(base + ACTOR_RIG_VERTEX_WORDS - 1u),
        ) * uv_scale;
        let glint_scale = vec2(word_f32(glint_base + 3u), word_f32(glint_base + 4u)) / item_extent;
        corner.uv = (corner.uv - origin) * glint_scale;
        if (corner.back_uv.x >= -1.0e8) {
            corner.back_uv = (corner.back_uv - origin) * glint_scale;
        }
    }
    return corner;
}

// Tangent with handedness in `w`, from the triangle's position and texture gradients.
fn triangle_tangent(p: array<vec3<f32>, 3>, uv: array<vec2<f32>, 3>, normal: vec3<f32>) -> vec4<f32> {
    let edge_a = p[1] - p[0];
    let edge_b = p[2] - p[0];
    let delta_a = uv[1] - uv[0];
    let delta_b = uv[2] - uv[0];
    let determinant = delta_a.x * delta_b.y - delta_b.x * delta_a.y;
    if (abs(determinant) < 1e-12) {
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

fn write_vertex(
    index: u32,
    position: vec3<f32>,
    colour: u32,
    uv: vec2<f32>,
    mid_uv: vec2<f32>,
    light: vec2<f32>,
    normal: vec3<f32>,
    tangent: vec4<f32>,
    block_id: i32,
) {
    // Unsigned subtraction also handles pages that cross the low address word's wrap.
    let local = index - params.counts.w;
    if (local >= arrayLength(&vertices) / ACTOR_VERTEX_WORDS) { return; }
    let base = local * ACTOR_VERTEX_WORDS;
    let raw_light = clamp(light, vec2(0.0), vec2(15.0)) * 16.0;
    vertices[base + IRIS_WORD_POSITION] = bitcast<u32>(position.x);
    vertices[base + IRIS_WORD_POSITION + 1u] = bitcast<u32>(position.y);
    vertices[base + IRIS_WORD_POSITION + 2u] = bitcast<u32>(position.z);
    vertices[base + IRIS_WORD_COLOR] = colour;
    vertices[base + IRIS_WORD_UV0] = bitcast<u32>(uv.x);
    vertices[base + IRIS_WORD_UV0 + 1u] = bitcast<u32>(uv.y);
    vertices[base + ACTOR_MID_UV_WORD] = bitcast<u32>(mid_uv.x);
    vertices[base + ACTOR_MID_UV_WORD + 1u] = bitcast<u32>(mid_uv.y);
    vertices[base + IRIS_WORD_LIGHTMAP] = pack2x16float(raw_light);
    vertices[base + IRIS_WORD_NORMAL] = pack4x8snorm(vec4(normal, 0.0));
    vertices[base + IRIS_WORD_TANGENT] = pack4x8snorm(tangent);
    // Signed shorts, as Iris stores `mc_Entity`, with Iris's render type for non-fluid
    // geometry (-1) in `y`; `at_midBlock` is zero.
    vertices[base + IRIS_WORD_BLOCK_ID] = (bitcast<u32>(block_id) & 0xffffu) | 0xffff0000u;
    vertices[base + IRIS_WORD_MID_BLOCK] = 0u;
}

// Rig vertex x of the dispatch's job y (y continues in z past the dispatch limit).
@compute @workgroup_size(SKIN_WORKGROUP_SIZE)
fn skin_rig(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let x_blocks = (params.counts.z + groups.x * SKIN_WORKGROUP_SIZE - 1u) / (groups.x * SKIN_WORKGROUP_SIZE);
    let job_index = id.y + (id.z / x_blocks) * groups.y;
    if (job_index >= params.counts.x) {
        return;
    }
    let job = jobs[params.counts.y + job_index];
    let vertex = id.x + (id.z % x_blocks) * groups.x * SKIN_WORKGROUP_SIZE;
    if (vertex >= job.vertex_count) {
        return;
    }
    let first = vertex - vertex % 3u;
    let corner_index = vertex - first;
    let corners = array(skin_corner(job, first), skin_corner(job, first + 1u), skin_corner(job, first + 2u));
    let corner = corners[corner_index];
    let positions = array(corners[0].position, corners[1].position, corners[2].position);
    let uvs = array(corners[0].uv, corners[1].uv, corners[2].uv);
    let mid_uv = (uvs[0] + uvs[1] + uvs[2]) / 3.0;
    let light = vec2(f32(job.light & 15u), f32((job.light >> 4u) & 15u));
    write_vertex(
        job.first_output + vertex,
        corner.position,
        job.colour,
        corner.uv,
        mid_uv,
        light,
        corner.normal,
        triangle_tangent(positions, uvs, corner.normal),
        job.block_id,
    );
    // The back face reverses the winding (corners 0, 2, 1) and samples the back UV map.
    let back_slot = select(3u - corner_index, 0u, corner_index == 0u);
    let back_index = job.first_output + job.vertex_count + first + back_slot;
    let two_sided = (job.flags & JOB_ONE_SIDED) == 0u
        && corners[0].back_uv.x >= -1.0e8 && corners[1].back_uv.x >= -1.0e8 && corners[2].back_uv.x >= -1.0e8;
    if (!two_sided) {
        write_vertex(back_index, positions[0], 0u, vec2(0.0), vec2(0.0), vec2(0.0), vec3(0.0), vec4(0.0), job.block_id);
        return;
    }
    let back_uvs = array(corners[0].back_uv, corners[2].back_uv, corners[1].back_uv);
    let back_positions = array(positions[0], positions[2], positions[1]);
    let back_mid = (back_uvs[0] + back_uvs[1] + back_uvs[2]) / 3.0;
    write_vertex(
        back_index,
        corner.position,
        job.colour,
        corner.back_uv,
        back_mid,
        light,
        -corner.normal,
        triangle_tangent(back_positions, back_uvs, -corner.normal),
        job.block_id,
    );
}

fn block_f32(index: u32) -> f32 {
    return bitcast<f32>(block_entity_words[index]);
}

struct BlockCorner {
    position: vec3<f32>,
    uv: vec2<f32>,
}

fn block_corner(vertex: u32) -> BlockCorner {
    let base = vertex * BLOCK_ENTITY_VERTEX_WORDS;
    return BlockCorner(
        vec3(block_f32(base), block_f32(base + 1u), block_f32(base + 2u)) - params.camera.xyz,
        vec2(block_f32(base + 3u), block_f32(base + 4u)),
    );
}

// Block-entity vertex x (continuing in y past the dispatch limit): world-space triangles
// with atlas UVs, a gamma tint and packed actor light.
@compute @workgroup_size(SKIN_WORKGROUP_SIZE)
fn convert_block_entities(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(num_workgroups) groups: vec3<u32>,
) {
    let vertex = id.x + id.y * groups.x * SKIN_WORKGROUP_SIZE;
    if (vertex >= params.counts.x) {
        return;
    }
    let first = vertex - vertex % 3u;
    let corners = array(block_corner(first), block_corner(first + 1u), block_corner(first + 2u));
    let corner = corners[vertex - first];
    let base = vertex * BLOCK_ENTITY_VERTEX_WORDS;
    let colour = vec4(block_f32(base + 5u), block_f32(base + 6u), block_f32(base + 7u), block_f32(base + 8u));
    let actor_light = block_entity_words[base + 12u];
    // The beacon beam carries its tile in the normal; it is unlit, as Java's is full-bright.
    let beam = actor_light == 0xffffffffu;
    var normal = vec3(block_f32(base + 9u), block_f32(base + 10u), block_f32(base + 11u));
    let positions = array(corners[0].position, corners[1].position, corners[2].position);
    let uvs = array(corners[0].uv, corners[1].uv, corners[2].uv);
    let face = cross(positions[2] - positions[0], positions[1] - positions[0]);
    if (beam || dot(normal, normal) < 1e-6) {
        normal = select(vec3(0.0, 1.0, 0.0), normalize(face), dot(face, face) > 1e-12);
    }
    normal = normalize(normal);
    // Without the world flag the light is baked into the tint: full lightmap.
    let lit = !beam && (actor_light & ACTOR_LIGHT_WORLD_FLAG) != 0u;
    let light = select(vec2(15.0), vec2(f32(actor_light & 15u), f32((actor_light >> 4u) & 15u)), lit);
    write_vertex(
        params.counts.y + vertex,
        corner.position,
        pack4x8unorm(clamp(colour, vec4(0.0), vec4(1.0))),
        corner.uv,
        (uvs[0] + uvs[1] + uvs[2]) / 3.0,
        light,
        normal,
        triangle_tangent(positions, uvs, normal),
        bitcast<i32>(params.counts.z),
    );
}
