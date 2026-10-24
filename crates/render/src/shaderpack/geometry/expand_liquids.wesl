// Expands packed liquid records (water and lava surfaces) into Iris faces.
import render::liquid::{liquid_record_corner, face_normal};
import render::chunk_bindings::{LIQUID_DEPTH_WRITE_BIT, LIQUID_TWO_SIDED_BIT};
import render::material::materials;
import render::biome_tint::blended_biome_tint;
import render::lighting::{face_shade, tint_to_gamma};
import render::shaderpack::geometry::expand_common::{FaceCorners, FaceSource, JobFace, expand_job, expand_job_count, expand_job_index, write_job_faces, EXPAND_WORKGROUP_SIZE};

/// Job: x = liquid record, y = origin slot, z = first output face, w = reserved face flags.
@compute @workgroup_size(EXPAND_WORKGROUP_SIZE)
fn expand_liquids(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let index = expand_job_index(id, groups);
    if (index >= expand_job_count()) {
        return;
    }
    let job = expand_job(index);
    write_job_faces(job, liquid_face(job));
}

/// The liquid surface face of `job`'s record.
fn liquid_face(job: vec4<u32>) -> JobFace {
    // Packed liquid corners wind clockwise; Iris faces wind counter-clockwise.
    var winding = array<u32, 4>(0u, 3u, 2u, 1u);
    var corners: FaceCorners;
    let first = liquid_record_corner(job.x, 0u);
    for (var corner = 0u; corner < 4u; corner += 1u) {
        let decoded = liquid_record_corner(job.x, winding[corner]);
        corners.positions[corner] = decoded.local_position;
        corners.uvs[corner] = decoded.uv;
        corners.light[corner] = vec2(f32(decoded.light_sample & 15u), f32((decoded.light_sample >> 4u) & 15u));
        // Native liquid vertex colours carry no ambient occlusion.
        corners.ao[corner] = 1.0;
    }
    let normal = face_normal(first.face);
    let material = materials(first.selected_material_id);
    var tint = vec4(1.0);
    if ((first.packed_material & LIQUID_DEPTH_WRITE_BIT) == 0u) {
        let water = blended_biome_tint(0x30u, 0u, u32(first.chunk_origin.w), vec3<f32>(first.block_coordinate), vec3<f32>(first.chunk_origin.xyz));
        tint = vec4(tint_to_gamma(vec4(water.rgb, 1.0)).rgb, water.a);
    }
    var source: FaceSource;
    source.normal = normal;
    source.tint = tint;
    source.face_shade = face_shade(normal, (first.light_sample & 2048u) != 0u);
    source.block = vec3<f32>(first.block_coordinate);
    source.chunk_origin = first.chunk_origin.xyz;
    source.material_id = first.selected_material_id;
    source.material = material;
    source.two_sided = (first.packed_material & LIQUID_TWO_SIDED_BIT) != 0u;
    source.fluid = true;
    return JobFace(corners, source, true);
}
