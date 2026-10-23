// Expands placed block-model quads (plants, stairs, slabs, translucent cubes ...) into Iris faces.
import render::model::{model_quad_at, model_corner_position, model_corner_uv, model_light_sample, model_quad_normal, stem_colour};
import render::material::materials;
import render::biome_tint::blended_biome_tint_gamma;
import render::lighting::{light_ao_factor, material_ambient_occlusion, material_face_shade};
import render::shaderpack::geometry::expand_common::{FaceCorners, FaceSource, JobFace, expand_job, expand_job_count, expand_job_index, write_job_faces, EXPAND_WORKGROUP_SIZE};

/// Job: x = model-ref record, y = origin slot, z = first output face, w = quad index and face flags.
@compute @workgroup_size(EXPAND_WORKGROUP_SIZE)
fn expand_models(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = expand_job_index(id, groups);
    if (index >= expand_job_count()) {
        return;
    }
    let job = expand_job(index);
    write_job_faces(job, model_face(job));
}

/// The face of the template quad `job` places; invalid when the model's addresses
/// fail validation.
fn model_face(job: vec4<u32>) -> JobFace {
    let quad = model_quad_at(job.x, job.w & JOB_MODEL_QUAD_MASK, job.y);
    if (!quad.valid) {
        var invalid: JobFace;
        return invalid;
    }
    let material = materials(quad.selected_material_id);
    var corners: FaceCorners;
    var raw_uvs: array<vec2<f32>, 4>;
    var lowest = vec2(1e9);
    var samples: array<u32, 4>;
    for (var corner = 0u; corner < 4u; corner += 1u) {
        corners.positions[corner] = quad.block_position + model_corner_position(quad, corner);
        raw_uvs[corner] = model_corner_uv(quad, material, corner);
        lowest = min(lowest, raw_uvs[corner]);
        let sample = model_light_sample(quad, corner);
        samples[corner] = sample;
        corners.light[corner] = vec2(f32(sample & 15u), f32((sample >> 4u) & 15u));
        corners.ao[corner] = material_ambient_occlusion(light_ao_factor((sample >> 8u) & 7u), material.flags);
    }
    // Wrapped template UVs repeat the sprite; an atlas sprite cannot, so shift the quad's
    // rectangle to its first repeat and clamp what extends past it.
    let wrap = floor(lowest);
    for (var corner = 0u; corner < 4u; corner += 1u) {
        corners.uvs[corner] = raw_uvs[corner] - wrap;
    }
    let geometric = cross(corners.positions[1] - corners.positions[0], corners.positions[2] - corners.positions[0]);
    let authored = model_quad_normal(quad);
    let normal = select(authored, normalize(geometric), dot(geometric, geometric) > 1e-12);
    var tint = stem_colour(material.flags);
    let tint_kind = material.flags & 0x30u;
    if (tint_kind != 0u) {
        tint = blended_biome_tint_gamma(tint_kind, material.flags, u32(quad.chunk_origin.w), quad.block_position, vec3<f32>(quad.chunk_origin.xyz)).rgb;
    }
    if ((material.flags & MATERIAL_CAULDRON_WATER_FLAG) != 0u) {
        tint *= (CAULDRON_WATER_COLOUR).rgb;
    }
    var source: FaceSource;
    source.normal = normal;
    source.tint = vec4(tint, 1.0);
    source.face_shade = material_face_shade(authored, (samples[0] & 2048u) != 0u, material.flags);
    source.block = quad.block_position;
    source.chunk_origin = quad.chunk_origin.xyz;
    source.material_id = quad.selected_material_id;
    source.material = material;
    // The chunk renderer's model fragment gate keeps back faces only of two-sided template quads.
    source.two_sided = (quad.quad_flags & MODEL_QUAD_TWO_SIDED_FLAG) != 0u;
    return JobFace(corners, source, true);
}
