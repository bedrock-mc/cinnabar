//! Stage transform tests: whole stages through naga.

use super::*;
use crate::compile::{OutputMap, compile};
use crate::options::OptionValues;
use crate::preprocess::{expand, preprocess};
use crate::reflection::VertexAttribute;
use crate::source::PackSource;

const VERTEX: &str = "#version 120
varying vec2 texcoord;
varying vec4 color;
void main() {
gl_Position = ftransform();
texcoord = (gl_TextureMatrix[0] * gl_MultiTexCoord0).xy;
color = gl_Color;
}
";

const FRAGMENT: &str = "#version 120
uniform sampler2D colortex0;
uniform float viewWidth;
varying vec2 texcoord;
varying vec4 color;
void main() {
gl_FragData[0] = texture2D(colortex0, texcoord) * color * viewWidth;
}
";

/// Expands and transforms an in-memory stage with a shared program interface.
fn draft(
    file: &str,
    text: &str,
    stage: StageKind,
    iface: &mut ProgramInterface,
) -> (StageDraft, crate::preprocess::LineMap) {
    let source = PackSource::from_files([(file, text)]);
    let expanded = expand(&source, file, &[], &OptionValues::new()).expect("expands");
    let pp = preprocess(expanded, &[]).expect("preprocesses");
    let names = ResourceNames {
        image_formats: &HashMap::new(),
        sampler_classes: &SamplerClasses::new(),
    };
    let draft = transform(&pp, stage, iface, &names).expect("transforms");
    (draft, pp.map)
}

#[test]
fn a_version_120_program_becomes_vulkan_glsl_naga_validates() {
    let mut iface = ProgramInterface::default();
    let vertex = draft("gbuffers_basic.vsh", VERTEX, StageKind::Vertex, &mut iface);
    let fragment = draft(
        "gbuffers_basic.fsh",
        FRAGMENT,
        StageKind::Fragment,
        &mut iface,
    );
    let ubo = merge_uniforms([&vertex.0, &fragment.0]).expect("uniforms agree");
    let names: Vec<_> = ubo.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"viewWidth"), "{names:?}");

    let inputs: Vec<_> = vertex
        .0
        .vertex_inputs
        .iter()
        .map(|(_, n)| n.as_str())
        .collect();
    // `ftransform()` reads the position, so the raw packed-position parts follow the inputs.
    assert_eq!(
        inputs,
        [
            "iris_Vertex",
            "iris_Color",
            "iris_MultiTexCoord0",
            "iris_PackedPosition",
            "iris_DrawOrigin",
        ]
    );
    assert_eq!(fragment.0.frag_outputs, [(0, "iris_FragData0".to_owned())]);
    assert!(
        fragment
            .0
            .resource_names
            .iter()
            .any(|(_, _, name)| name == "colortex0")
    );

    for ((draft, lines), stage) in [(vertex, StageKind::Vertex), (fragment, StageKind::Fragment)] {
        let (glsl, origins) = assemble(&draft, &ubo, &iface.location_defines().unwrap());
        let map = OutputMap {
            glsl: &glsl,
            origins: &origins,
            lines: &lines,
        };
        if let Err(errors) = compile(stage, &map) {
            panic!("{stage:?} does not validate: {errors:?}\n{glsl}");
        }
    }
}

#[test]
fn a_vertex_stage_reading_an_undeclared_attribute_gets_iris_input() {
    let mut iface = ProgramInterface::default();
    let source = "#version 330 compatibility
in vec4 mc_Entity;
out float id;
void main() { id = mc_Entity.x; gl_Position = gl_Vertex; }
";
    let (vertex, _) = draft(
        "gbuffers_terrain.vsh",
        source,
        StageKind::Vertex,
        &mut iface,
    );
    let inputs: Vec<_> = vertex
        .vertex_inputs
        .iter()
        .map(|(location, name)| (name.as_str(), Some(*location)))
        .collect();
    // The position also reads the raw packed-position parts, at the next free locations.
    assert_eq!(
        inputs[..2],
        [
            ("mc_Entity", VertexAttribute::BlockId.location()),
            ("iris_Vertex", VertexAttribute::Position.location()),
        ]
    );
    let parts: Vec<_> = inputs[2..].iter().map(|(name, _)| *name).collect();
    assert_eq!(parts, ["iris_PackedPosition", "iris_DrawOrigin"]);
}

#[test]
fn integer_and_float_gathers_compile_with_separate_sampler_bindings() {
    let mut iface = ProgramInterface::default();
    let (fragment, lines) = draft("composite.fsh", "#version 400 compatibility
uniform usampler2D colortex0;
uniform isampler2D customInteger;
uniform sampler2D colortex1;
void main() { gl_FragData[0] = vec4(textureGather(colortex0, vec2(0.5))) + vec4(textureGather(customInteger, vec2(0.5))) + textureGather(colortex1, vec2(0.5)); }
", StageKind::Fragment, &mut iface);
    let (glsl, origins) = assemble(&fragment, &[], &iface.location_defines().unwrap());
    let compiled = compile(
        StageKind::Fragment,
        &OutputMap {
            glsl: &glsl,
            origins: &origins,
            lines: &lines,
        },
    )
    .expect("mixed gathers compile");
    let mut bindings: Vec<_> = compiled
        .module
        .global_variables
        .iter()
        .filter(|(_, variable)| {
            matches!(
                compiled.module.types[variable.ty].inner,
                naga::TypeInner::Sampler { comparison: false }
            )
        })
        .map(|(_, variable)| variable.binding.as_ref().unwrap().binding)
        .collect();
    bindings.sort_unstable();
    let mut expected = [
        SamplerClass::ClampNearest,
        SamplerClass::RepeatNonFiltering,
        SamplerClass::ClampLinear,
    ]
    .map(SamplerClass::binding);
    expected.sort_unstable();
    assert_eq!(bindings, expected);
}

#[test]
fn loop_shadowing_preserves_a_valid_shader() {
    let mut iface = ProgramInterface::default();
    let (stage, lines) = draft(
        "composite.fsh",
        "#version 330\nuniform bool enabled; out vec4 color; void main() { float value = 0.0; for (int enabled = 0; enabled < 2; enabled++) { value += float(enabled); } color = vec4(enabled ? value : 0.0); }",
        StageKind::Fragment,
        &mut iface,
    );
    let uniforms = merge_uniforms([&stage]).unwrap();
    let (glsl, origins) = assemble(&stage, &uniforms, &iface.location_defines().unwrap());
    let result = compile(
        StageKind::Fragment,
        &OutputMap {
            glsl: &glsl,
            origins: &origins,
            lines: &lines,
        },
    );
    assert!(result.is_ok(), "{result:?}\n{glsl}");
}
