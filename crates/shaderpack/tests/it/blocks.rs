//! GLSL that OpenGL drivers accept and naga's parser needs rewritten: interface blocks
//! between stages, bools in buffer blocks, integer 1D samplers, returns that convert,
//! literal shifts into the sign bit, `coherent`/`volatile` resources and implicit-LOD
//! shadow lookups in compute shaders.

use crate::{program, reflection};
use shaderpack::{PackSource, SampleType, ShaderPack, TextureDimension, TranslateConfig};

fn translate(files: &[(&str, &str)]) -> shaderpack::PackTranslation {
    ShaderPack::new(PackSource::from_files(files.iter().copied()))
        .translate(&TranslateConfig::default())
}

#[test]
fn interface_blocks_link_by_block_and_member_name() {
    let t = translate(&[
        (
            "composite.vsh",
            "#version 430 compatibility\n\
             out DATA { vec4 color; flat int id; mat3 m; } data_out;\n\
             void main() {\n\
                 gl_Position = ftransform();\n\
                 data_out.color = gl_Color;\n\
                 data_out.id = 3;\n\
                 data_out.m = mat3(2.0);\n\
             }\n",
        ),
        (
            "composite.fsh",
            "#version 430\n\
             in DATA { vec4 color; flat int id; mat3 m; } data_in;\n\
             /* RENDERTARGETS: 0 */\n\
             layout(location = 0) out vec4 colour;\n\
             void main() { colour = data_in.color * float(data_in.id) * data_in.m[0].x; }\n",
        ),
    ]);
    let pass = program(&t, "composite");
    reflection(pass);
    // The vertex stage writes and the fragment stage reads the same locations.
    for stage in &pass.stages {
        let glsl = stage.result.as_ref().map(|c| c.glsl.as_str()).unwrap_or("");
        assert!(glsl.contains("iris_b_DATA_color_0"), "{glsl}");
        assert!(glsl.contains("flat"), "integer members are flat: {glsl}");
    }
}

#[test]
fn buffer_block_bools_are_integers_read_and_written_through_conversions() {
    let t = translate(&[(
        "begin.csh",
        "#version 430\n\
         layout(local_size_x = 1) in;\n\
         const ivec3 workGroups = ivec3(1, 1, 1);\n\
         layout(std430, binding = 4) buffer State { vec3 v; bool idle; bool check; bvec2 pair; };\n\
         void main() {\n\
             idle = check;\n\
             check = v.x > 0.0 && !idle;\n\
             if (pair.x) { pair = bvec2(idle, true); }\n\
         }\n",
    )]);
    let r = reflection(program(&t, "begin"));
    // std430: vec3 at 0, the bools as 4-byte integers at 12 and 16, the pair at 24.
    assert_eq!(r.storage_buffers[0].min_size, 32);
}

#[test]
fn signed_literal_shifts_into_the_sign_bit_wrap() {
    // Rethinking Voxels marks texels with `uvec4(1<<31)`; naga's constant evaluator rejects
    // the overflow, GLSL wraps it.
    let t = translate(&[(
        "begin.csh",
        "#version 430\n\
         layout(local_size_x = 1) in;\n\
         const ivec3 workGroups = ivec3(1, 1, 1);\n\
         layout(r32ui) uniform uimage2D marks;\n\
         void main() { imageStore(marks, ivec2(0), uvec4(1<<31)); }\n",
    )]);
    let begin = program(&t, "begin");
    reflection(begin);
    let glsl = &begin.stages[0].result.as_ref().expect("compiles").glsl;
    assert!(glsl.contains("0x80000000"), "{glsl}");
}

#[test]
fn integer_1d_samplers_and_returns_that_convert_translate() {
    let t = translate(&[
        (
            "shaders.properties",
            "image.blocks = texBlockData RG_INTEGER RG32UI UNSIGNED_INT false false 64\n",
        ),
        (
            "composite.vsh",
            "#version 430 compatibility\nvoid main() { gl_Position = ftransform(); }\n",
        ),
        (
            "composite.fsh",
            "#version 430\n\
             uniform usampler1D texBlockData;\n\
             uint block(int i) { if (i < 0) return 0; return texelFetch(texBlockData, i, 0).r; }\n\
             float half_of(int i) { return i / 2; }\n\
             /* RENDERTARGETS: 0 */\n\
             layout(location = 0) out vec4 colour;\n\
             void main() { colour = vec4(float(block(1)), half_of(3), 0.0, 1.0); }\n",
        ),
    ]);
    let r = reflection(program(&t, "composite"));
    let blocks = r.textures.iter().find(|t| t.name == "texBlockData");
    assert_eq!(
        blocks.map(|t| (t.dimension, t.sample_type)),
        Some((TextureDimension::D1, SampleType::Uint))
    );
}

#[test]
fn coherent_and_volatile_resources_translate_without_the_qualifiers() {
    let t = translate(&[(
        "begin.csh",
        "#version 430\n\
         layout(local_size_x = 1) in;\n\
         const ivec3 workGroups = ivec3(1, 1, 1);\n\
         layout(std430, binding = 4) coherent buffer State { float total; };\n\
         layout(r32f) coherent volatile uniform image2D scratch;\n\
         void main() {\n\
             total += imageLoad(scratch, ivec2(0)).r;\n\
             memoryBarrierBuffer();\n\
             imageStore(scratch, ivec2(0), vec4(total));\n\
         }\n",
    )]);
    let begin = program(&t, "begin");
    let r = reflection(begin);
    assert_eq!(r.storage_buffers.len(), 1);
    assert_eq!(r.storage_images.len(), 1);
    let glsl = &begin.stages[0].result.as_ref().expect("compiles").glsl;
    assert!(!glsl.contains("coherent"), "{glsl}");
    assert!(!glsl.contains("volatile"), "{glsl}");
}

#[test]
fn compute_shadow_and_texture_lookups_sample_lod_zero() {
    let t = translate(&[(
        "deferred1.csh",
        "#version 430\n\
         layout(local_size_x = 16, local_size_y = 16) in;\n\
         const vec2 workGroupsRender = vec2(0.5, 0.5);\n\
         uniform sampler2DShadow shadowtex0;\n\
         uniform sampler2D noisetex;\n\
         layout(rgba16f) uniform image2D colorimg10;\n\
         void main() {\n\
             vec2 uv = vec2(gl_GlobalInvocationID.xy) / 128.0;\n\
             float shadow = shadow2D(shadowtex0, vec3(uv, 0.5)).x;\n\
             float noise = texture2D(noisetex, uv).r;\n\
             imageStore(colorimg10, ivec2(gl_GlobalInvocationID.xy), vec4(shadow, noise, 0.0, 1.0));\n\
         }\n",
    )]);
    let r = reflection(program(&t, "deferred1"));
    let mut sampled: Vec<&str> = r.textures.iter().map(|t| t.name.as_str()).collect();
    sampled.sort_unstable();
    assert_eq!(sampled, ["noisetex", "shadowtex0"]);
}
