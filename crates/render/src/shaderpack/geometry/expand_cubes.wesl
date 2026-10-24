// Expands packed greedy cube quads into one Iris face per covered block, as Java tessellates
// block faces: the atlas cannot repeat a sprite across a greedy quad.
import render::chunk::{cube_quad, cube_light_sample, quad_corner, face_normal, quad_axes, quad_position, greedy_uv};
import render::material::{materials, material_uv_flags, TWO_SIDED};
import render::biome_tint::{uniform_biome_tint_gamma, blended_biome_tint_gamma};
import render::lighting::{light_ao_factor, material_ambient_occlusion, material_face_shade};
import render::shaderpack::geometry::expand_common::{FaceCorners, FaceSource, JobFace, expand_job, expand_job_count, expand_job_index, write_job_faces, EXPAND_WORKGROUP_SIZE};

/// Gamma tint of the block at section-local corner `block`, as the cube renderer evaluates it.
fn cube_tint(flags: u32, record: u32, block: vec3<f32>, chunk_origin: vec3<i32>) -> vec3<f32> {
    let tint_kind = flags & 0x30u;
    if (tint_kind == 0u) {
        return vec3(1.0);
    }
    let uniform_tint = uniform_biome_tint_gamma(tint_kind, flags, record);
    if (uniform_tint.a != 0.0) {
        return uniform_tint.rgb;
    }
    return blended_biome_tint_gamma(tint_kind, flags, record, block + vec3(0.5), vec3<f32>(chunk_origin)).rgb;
}

/// Job: x = packed quad index, y = origin slot, z = first output face, w = covered block
/// (width step in bits 0-7, height step in bits 8-15) and reserved face flags.
@compute @workgroup_size(EXPAND_WORKGROUP_SIZE)
fn expand_cubes(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = expand_job_index(id, groups);
    if (index >= expand_job_count()) {
        return;
    }
    let job = expand_job(index);
    write_job_faces(job, cube_face(job));
}

/// The face of the block `job` covers within its packed greedy cube quad.
fn cube_face(job: vec4<u32>) -> JobFace {
    let cube = cube_quad(job.x, job.y);
    let material = materials(cube.selected_material_id);
    let extent = vec2(cube.width, cube.height);
    let step = vec2<f32>(f32(job.w & 0xffu), f32((job.w >> 8u) & 0xffu));
    let axes = quad_axes(cube.face);
    let block = cube.local_origin + axes[0] * step.x + axes[1] * step.y;
    let uv_flags = material_uv_flags(material.flags, cube.chunk_origin.xyz + vec3<i32>(cube.local_origin));
    let tile = floor(greedy_uv(cube.face, step + vec2(0.5), cube.width, cube.height, uv_flags));

    // The quad's four light samples, bilinearly spread over the blocks it covers.
    var samples: array<u32, 4>;
    var sample_at: array<vec2<f32>, 4>;
    for (var corner = 0u; corner < 4u; corner += 1u) {
        samples[corner] = cube_light_sample(cube.lighting_record_index, corner);
        let at = quad_corner(cube.face, corner, cube.local_origin, cube.width, cube.height) - cube.local_origin;
        sample_at[corner] = quad_position(cube.face, at) / extent;
    }
    var corners: FaceCorners;
    for (var corner = 0u; corner < 4u; corner += 1u) {
        let position = quad_corner(cube.face, corner, block, 1.0, 1.0);
        let in_quad = quad_position(cube.face, position - cube.local_origin);
        corners.positions[corner] = position;
        corners.uvs[corner] = greedy_uv(cube.face, in_quad, cube.width, cube.height, uv_flags) - tile;
        let at = in_quad / extent;
        var light = vec2(0.0);
        var ao = 0.0;
        for (var sample = 0u; sample < 4u; sample += 1u) {
            let weights = mix(vec2(1.0) - at, at, sample_at[sample]);
            let weight = weights.x * weights.y;
            let value = samples[sample];
            light += weight * vec2(f32(value & 15u), f32((value >> 4u) & 15u));
            ao += weight * material_ambient_occlusion(light_ao_factor((value >> 8u) & 7u), material.flags);
        }
        corners.light[corner] = light;
        corners.ao[corner] = ao;
    }
    let normal = face_normal(cube.face);
    var source: FaceSource;
    source.normal = normal;
    source.tint = vec4(cube_tint(material.flags, u32(cube.chunk_origin.w), block, cube.chunk_origin.xyz), 1.0);
    source.face_shade = material_face_shade(normal, (samples[0] & 2048u) != 0u, material.flags);
    source.block = block;
    source.chunk_origin = cube.chunk_origin.xyz;
    source.material_id = cube.selected_material_id;
    source.material = material;
    source.two_sided = (material.flags & TWO_SIDED) != 0u;
    return JobFace(corners, source, true);
}
