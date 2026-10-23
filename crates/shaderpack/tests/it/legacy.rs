//! Compatibility-profile packs: legacy built-ins, attributes, varyings and samplers.

use crate::{open_fixture, program, reflection, texture_names, uniform_names};
use shaderpack::{
    CompiledStage, FIRST_TEXTURE_BINDING, LOOSE_UNIFORM_BINDING, LOOSE_UNIFORM_BLOCK, ProgramKind,
    SHADOW_SAMPLER_BINDING, SampleType, SamplerClass, ScalarType, StageKind, StageMask,
    TEXTURE_GROUP, TranslateConfig, UNIFORM_GROUP, ValueType, VertexAttribute,
};

const VEC4: ValueType = ValueType::Vector {
    size: 4,
    scalar: ScalarType::Float,
};

/// (member name, byte offset) of each loose-uniform block member in a compiled stage's naga
/// module, and the block's byte size.
fn loose_block_layout(stage: &CompiledStage) -> (Vec<(String, u32)>, u32) {
    let module = &stage.module;
    let (_, var) = module
        .global_variables
        .iter()
        .find(|(_, v)| {
            v.binding
                == Some(naga::ResourceBinding {
                    group: UNIFORM_GROUP,
                    binding: LOOSE_UNIFORM_BINDING,
                })
        })
        .unwrap_or_else(|| panic!("{:?} stage has no loose-uniform block", stage.stage));
    match &module.types[var.ty].inner {
        naga::TypeInner::Struct { members, span } => (
            members
                .iter()
                .map(|m| (m.name.clone().unwrap_or_default(), m.offset))
                .collect(),
            *span,
        ),
        other => panic!("loose-uniform block is not a struct: {other:?}"),
    }
}

#[test]
fn version_120_terrain_maps_legacy_inputs_uniforms_and_samplers_to_fixed_bindings() {
    let Some(pack) = open_fixture("legacy") else {
        return;
    };
    let t = pack.translate(&TranslateConfig::default());
    let terrain = program(&t, "gbuffers_terrain");
    let r = reflection(terrain);
    assert_eq!(r.kind, ProgramKind::Render);
    assert_eq!(
        r.entry_points.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        [StageKind::Vertex, StageKind::Fragment]
    );

    // Every legacy and Iris attribute the vertex stage reads, at its fixed location, and the
    // raw position parts `gl_Vertex` is unpacked from, at the first free locations.
    let attributes: Vec<(&str, VertexAttribute)> = r
        .vertex_inputs
        .iter()
        .map(|i| (i.name.as_str(), i.attribute.clone()))
        .collect();
    assert_eq!(
        attributes,
        [
            ("iris_Vertex", VertexAttribute::Position),
            ("iris_Color", VertexAttribute::Color),
            ("iris_Normal", VertexAttribute::Normal),
            ("iris_MultiTexCoord0", VertexAttribute::TexCoord),
            ("iris_MultiTexCoord1", VertexAttribute::Lightmap),
            ("iris_PackedPosition", VertexAttribute::PackedPosition),
            ("iris_DrawOrigin", VertexAttribute::DrawOrigin),
            ("mc_Entity", VertexAttribute::BlockId),
            ("mc_midTexCoord", VertexAttribute::MidTexCoord),
            ("at_tangent", VertexAttribute::Tangent),
        ]
    );
    for input in &r.vertex_inputs {
        let fixed = input.attribute.location();
        assert!(
            fixed.is_none_or(|fixed| fixed == input.location),
            "{} location",
            input.name
        );
        assert_eq!(
            VertexAttribute::from_glsl_name(&input.name),
            input.attribute
        );
    }
    let location = |attribute: VertexAttribute| {
        r.vertex_inputs
            .iter()
            .find(|i| i.attribute == attribute)
            .map(|i| i.location)
    };
    assert_eq!(location(VertexAttribute::PackedPosition), Some(5));
    assert_eq!(location(VertexAttribute::DrawOrigin), Some(6));
    // The stage reads float positions as three components and builds `gl_Vertex` from them.
    assert_eq!(
        r.vertex_inputs[0].ty,
        ValueType::Vector {
            size: 3,
            scalar: ScalarType::Float
        }
    );
    let mid = r
        .vertex_inputs
        .iter()
        .find(|i| i.name == "mc_midTexCoord")
        .unwrap();
    assert_eq!(
        mid.ty,
        ValueType::Vector {
            size: 2,
            scalar: ScalarType::Float
        }
    );
    let entity = r
        .vertex_inputs
        .iter()
        .find(|i| i.name == "mc_Entity")
        .unwrap();
    // Stored as Iris's shorts, so the stage reads integers and converts them.
    assert_eq!(
        entity.ty,
        ValueType::Vector {
            size: 4,
            scalar: ScalarType::Int
        }
    );

    // Fixed-function matrices and the pack's own uniforms share one std140 block.
    let ubo = r.uniforms.as_ref().expect("loose uniform block");
    assert_eq!(ubo.name, LOOSE_UNIFORM_BLOCK);
    assert_eq!(
        (ubo.group, ubo.binding),
        (UNIFORM_GROUP, LOOSE_UNIFORM_BINDING)
    );
    let members: Vec<(&str, u32, ValueType)> = ubo
        .members
        .iter()
        .map(|m| (m.name.as_str(), m.offset, m.ty))
        .collect();
    let mat = |n: u8| ValueType::Matrix {
        columns: n,
        rows: n,
        scalar: ScalarType::Float,
    };
    assert_eq!(
        members,
        [
            ("iris_ModelViewMat", 0, mat(4)),
            ("iris_ProjMat", 64, mat(4)),
            // std140 mat3: three vec4-aligned columns, 48 bytes.
            ("iris_NormalMat", 128, mat(3)),
            (
                "frameTimeCounter",
                176,
                ValueType::Scalar(ScalarType::Float)
            ),
            // `uniform bool` is stored as a 4-byte int.
            ("isRaining", 180, ValueType::Scalar(ScalarType::Int)),
        ]
    );
    assert_eq!(ubo.size, 192, "block size rounds up to 16 bytes");
    assert!(ubo.members.iter().all(|m| m.array.is_none()));
    assert_eq!(
        ubo.stages,
        StageMask {
            vertex: true,
            fragment: true,
            compute: false
        }
    );

    // Both stages declare the identical block, matching the reflection.
    let compiled: Vec<&CompiledStage> = terrain
        .stages
        .iter()
        .map(|s| s.result.as_ref().expect("stage compiles"))
        .collect();
    let vertex = loose_block_layout(compiled[0]);
    let fragment = loose_block_layout(compiled[1]);
    assert_eq!(vertex, fragment);
    let reflected: Vec<(String, u32)> = ubo
        .members
        .iter()
        .map(|m| (m.name.clone(), m.offset))
        .collect();
    assert_eq!(vertex.0, reflected);
    assert_eq!(vertex.1, ubo.size);

    // Separate textures after the shared samplers; the shadow sampler is a depth texture
    // compared with the shared comparison sampler.
    assert_eq!(texture_names(r), ["gtexture", "lightmap", "shadowtex0"]);
    for (i, tex) in r.textures.iter().enumerate() {
        assert_eq!(tex.group, TEXTURE_GROUP);
        assert_eq!(tex.binding, FIRST_TEXTURE_BINDING + i as u32);
        assert!(!tex.multisampled);
        assert_eq!(
            tex.stages,
            StageMask {
                vertex: false,
                fragment: true,
                compute: false
            }
        );
    }
    let sample_types: Vec<SampleType> = r.textures.iter().map(|t| t.sample_type).collect();
    assert_eq!(
        sample_types,
        [SampleType::Float, SampleType::Float, SampleType::Depth]
    );
    // The atlas repeats without filtering and the lightmap clamps, as in Java.
    let mut samplers: Vec<(u32, u32, Option<SamplerClass>)> = r
        .samplers
        .iter()
        .map(|s| (s.group, s.binding, s.class()))
        .collect();
    samplers.sort_by_key(|s| s.1);
    assert_eq!(
        samplers,
        [
            (TEXTURE_GROUP, SHADOW_SAMPLER_BINDING, None),
            (
                TEXTURE_GROUP,
                SamplerClass::ClampLinear.binding(),
                Some(SamplerClass::ClampLinear)
            ),
            (
                TEXTURE_GROUP,
                SamplerClass::RepeatNearest.binding(),
                Some(SamplerClass::RepeatNearest)
            ),
        ]
    );

    // gl_FragData[n] become outputs at location n, routed by DRAWBUFFERS.
    let outputs: Vec<(u32, ValueType, Option<u32>)> = r
        .color_outputs
        .iter()
        .map(|o| (o.location, o.ty, o.buffer))
        .collect();
    assert_eq!(outputs, [(0, VEC4, Some(0)), (1, VEC4, Some(1))]);
    assert!(!r.writes_depth);
    assert!(r.compute.is_none());
}

#[test]
fn version_400_compatibility_ftransform_needs_only_the_position_matrices() {
    let Some(pack) = open_fixture("legacy") else {
        return;
    };
    let t = pack.translate(&TranslateConfig::default());
    let basic = program(&t, "gbuffers_basic");
    let r = reflection(basic);
    let attributes: Vec<VertexAttribute> = r
        .vertex_inputs
        .iter()
        .map(|i| i.attribute.clone())
        .collect();
    assert_eq!(
        attributes,
        [
            VertexAttribute::Position,
            VertexAttribute::Color,
            VertexAttribute::PackedPosition,
            VertexAttribute::DrawOrigin
        ]
    );
    assert_eq!(uniform_names(r), ["iris_ModelViewMat", "iris_ProjMat"]);
    let ubo = r.uniforms.as_ref().unwrap();
    assert_eq!(
        ubo.stages,
        StageMask {
            vertex: true,
            fragment: false,
            compute: false
        }
    );
    assert!(r.textures.is_empty());
    assert!(r.samplers.is_empty());
    let outputs: Vec<(u32, Option<u32>)> = r
        .color_outputs
        .iter()
        .map(|o| (o.location, o.buffer))
        .collect();
    assert_eq!(
        outputs,
        [(0, Some(0))],
        "without a directive gl_FragData[0] lands in colortex0"
    );
}

fn inline_pack(files: &[(&str, &str)]) -> shaderpack::ShaderPack {
    shaderpack::ShaderPack::new(shaderpack::PackSource::from_files(
        files
            .iter()
            .map(|(k, v)| (*k, (*v).to_owned()))
            .collect::<Vec<_>>(),
    ))
}

#[test]
fn line_programs_get_the_core_matrices_and_attributes_iris_supplies() {
    // Complementary's `gbuffers_line` uses the core names under `#version 130` without
    // declaring them; Iris compiles line programs with its core-profile transformer.
    let vertex = "#version 130\n\
        void main() {\n\
            vec4 a = projectionMatrix * modelViewMatrix * vec4(vaPosition, 1.0);\n\
            vec4 b = projectionMatrix * modelViewMatrix * vec4(vaPosition + vaNormal, 1.0);\n\
            gl_Position = gl_VertexID % 2 == 0 ? a : b;\n\
        }\n";
    let fragment = "#version 130\nvoid main() { gl_FragData[0] = vec4(1.0); }\n";
    let pack = inline_pack(&[
        ("gbuffers_line.vsh", vertex),
        ("gbuffers_line.fsh", fragment),
    ]);
    let t = pack.translate(&TranslateConfig::default());
    let r = reflection(program(&t, "gbuffers_line"));
    let attributes: Vec<VertexAttribute> = r
        .vertex_inputs
        .iter()
        .map(|i| i.attribute.clone())
        .collect();
    assert_eq!(
        attributes,
        [
            VertexAttribute::Position,
            VertexAttribute::PackedPosition,
            VertexAttribute::Normal,
            VertexAttribute::DrawOrigin
        ]
    );
    // Read as the compatibility matrices, as Iris's core transformer renames them.
    assert_eq!(uniform_names(r), ["iris_ModelViewMat", "iris_ProjMat"]);
}

#[test]
fn a_texture_sampler_is_gtexture_in_every_version_and_const_in_parameters_compile() {
    // SEUS Renewed: `#version 130` gbuffers declare `uniform sampler2D texture` and take
    // `const in int` parameters.
    let vertex = "#version 130\nvarying vec2 uv;\nvoid main() { gl_Position = ftransform(); uv = gl_MultiTexCoord0.st; }\n";
    let fragment = "#version 130\n\
        uniform sampler2D texture;\n\
        varying vec2 uv;\n\
        float mask(in float id, const in int wanted) { return id == float(wanted) ? 1.0 : 0.0; }\n\
        void main() { gl_FragData[0] = texture2D(texture, uv) * mask(1.0, 1); }\n";
    let pack = inline_pack(&[
        ("gbuffers_textured.vsh", vertex),
        ("gbuffers_textured.fsh", fragment),
    ]);
    let t = pack.translate(&TranslateConfig::default());
    let r = reflection(program(&t, "gbuffers_textured"));
    assert_eq!(texture_names(r), ["gtexture"]);
}

#[test]
fn live_fragment_inputs_receive_missing_vertex_outputs() {
    let vertex = "#version 120\n\
        varying vec4 color;\n\
        varying vec2 waves;\n\
        varying mat3 tbnMatrix;\n\
        varying vec4 texcoord;\n\
        void main() {\n\
            color = gl_Color; waves = vec2(0.5); texcoord = gl_MultiTexCoord0;\n\
            tbnMatrix = mat3(1.0);\n\
            gl_Position = ftransform();\n\
        }\n";
    let fragment = "#version 120\n\
        varying vec4 texcoord;\n\
        varying vec3 worldNormal;\n\
        varying mat3 tbnMatrix;\n\
        varying vec4 color;\n\
        varying float unwritten[2];\n\
        void main() {\n\
            gl_FragData[0] = color * texcoord + vec4(worldNormal + tbnMatrix[2], unwritten[1]);\n\
            gl_FragData[1] = gl_Color;\n\
        }\n";
    let pack = inline_pack(&[
        ("gbuffers_entities.vsh", vertex),
        ("gbuffers_entities.fsh", fragment),
    ]);
    let t = pack.translate(&TranslateConfig::default());
    let entities = program(&t, "gbuffers_entities");
    reflection(entities);
    let module = &entities.stages[0].result.as_ref().unwrap().module;
    let function = &module.entry_points[0].function;
    let result = function.result.as_ref().unwrap();
    let naga::TypeInner::Struct { members, .. } = &module.types[result.ty].inner else {
        panic!("vertex outputs form an interface structure");
    };
    assert!(
        members
            .iter()
            .any(|member| member.name.as_deref() == Some("worldNormal"))
    );
    assert_eq!(
        function
            .expressions
            .iter()
            .filter(|(_, expression)| matches!(expression, naga::Expression::ZeroValue(_)))
            .count(),
        4
    );
}

#[test]
fn an_integer_lightmap_declaration_reads_the_float_lightmap_and_converts() {
    // Core-profile packs declare `in ivec2 vaUV2`; the vertex stream stores the lightmap
    // once, as floats, so the stage reads a float input and rounds it.
    let vertex = "#version 330\n\
        in vec3 vaPosition;\n\
        in ivec2 vaUV2;\n\
        flat out ivec2 light;\n\
        void main() { light = vaUV2; gl_Position = vec4(vaPosition, 1.0); }\n";
    let fragment = "#version 330\n\
        flat in ivec2 light;\n\
        layout(location = 0) out vec4 color;\n\
        void main() { color = vec4(vec2(light) / 240.0, 0.0, 1.0); }\n";
    let pack = inline_pack(&[
        ("gbuffers_terrain.vsh", vertex),
        ("gbuffers_terrain.fsh", fragment),
    ]);
    let t = pack.translate(&TranslateConfig::default());
    let terrain = program(&t, "gbuffers_terrain");
    let r = reflection(terrain);
    let lightmap = r
        .vertex_inputs
        .iter()
        .find(|i| i.attribute == VertexAttribute::Lightmap)
        .expect("vaUV2 is a vertex input");
    assert_eq!(lightmap.name, "vaUV2");
    assert_eq!(
        lightmap.ty,
        ValueType::Vector {
            size: 2,
            scalar: ScalarType::Float
        }
    );
    let glsl = &terrain.stages[0]
        .result
        .as_ref()
        .expect("vertex compiles")
        .glsl;
    // Packed vertices carry the lightmap in the packed position's `w`, block then sky byte;
    // the stage sums both sources (one of which reads zero) and rounds.
    assert!(
        glsl.contains(
            "vaUV2 = ivec2(round((iris_float_vaUV2 + vec2(float(iris_PackedPosition.w - \
             iris_PackedPosition.w / 256u * 256u), float(iris_PackedPosition.w / 256u)))));"
        ),
        "{glsl}"
    );
    assert!(
        r.vertex_inputs
            .iter()
            .any(|i| i.attribute == VertexAttribute::PackedPosition),
        "a lightmap input reads the packed position"
    );
}

#[test]
fn float_block_id_and_mid_block_declarations_read_the_stored_integers_and_convert() {
    // Iris stores `mc_Entity` as shorts and `at_midBlock` as bytes; float declarations read
    // integer inputs and convert, as GL converts integer vertex data for float inputs.
    let vertex = "#version 120\n\
        attribute vec4 mc_Entity;\n\
        attribute vec3 at_midBlock;\n\
        varying float id;\n\
        varying vec3 mid;\n\
        void main() { id = mc_Entity.x; mid = at_midBlock; gl_Position = ftransform(); }\n";
    let fragment = "#version 120\n\
        varying float id;\n\
        varying vec3 mid;\n\
        void main() { gl_FragData[0] = vec4(id, mid); }\n";
    let pack = inline_pack(&[
        ("gbuffers_terrain.vsh", vertex),
        ("gbuffers_terrain.fsh", fragment),
    ]);
    let t = pack.translate(&TranslateConfig::default());
    let terrain = program(&t, "gbuffers_terrain");
    let r = reflection(terrain);
    for (attribute, size) in [
        (VertexAttribute::BlockId, 4),
        (VertexAttribute::MidBlock, 3),
    ] {
        let input = r
            .vertex_inputs
            .iter()
            .find(|i| i.attribute == attribute)
            .expect("declared inputs are vertex inputs");
        assert_eq!(
            input.ty,
            ValueType::Vector {
                size,
                scalar: ScalarType::Int
            },
            "{attribute:?}"
        );
    }
    let glsl = &terrain.stages[0]
        .result
        .as_ref()
        .expect("vertex compiles")
        .glsl;
    for converted in [
        "mc_Entity = vec4(iris_int_mc_Entity);",
        "at_midBlock = vec3(iris_int_at_midBlock);",
    ] {
        assert!(glsl.contains(converted), "missing `{converted}` in\n{glsl}");
    }
}
