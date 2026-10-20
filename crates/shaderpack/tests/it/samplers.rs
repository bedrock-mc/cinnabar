//! Pack helpers that take a sampler: each call with a global texture gets its own copy.

use crate::program;
use shaderpack::{CompiledStage, PackSource, ShaderPack, StageKind, TranslateConfig};

const VERTEX: &str = "#version 120\nvarying vec2 uv;\nvoid main() { gl_Position = ftransform(); uv = gl_MultiTexCoord0.st; }\n";

/// A pack whose only program is `composite`, with `fragment` as its fragment stage.
fn pack(fragment: &str) -> ShaderPack {
    ShaderPack::new(PackSource::from_files([
        ("composite.vsh", VERTEX),
        ("composite.fsh", fragment),
    ]))
}

/// Returns the validated fragment stage.
fn fragment_stage(t: &shaderpack::ProgramTranslation) -> &CompiledStage {
    let stage: &CompiledStage = t
        .stages
        .iter()
        .find(|s| s.stage == StageKind::Fragment)
        .and_then(|s| s.result.as_ref().ok())
        .unwrap_or_else(|| {
            panic!(
                "fragment compiles: {:#?}",
                t.all_diagnostics().collect::<Vec<_>>()
            )
        });
    stage
}

#[test]
fn a_helper_taking_a_sampler_gets_a_copy_per_global_texture() {
    let fragment = "#version 120\n\
        uniform sampler2D colortex0;\n\
        uniform sampler2D colortex1;\n\
        varying vec2 uv;\n\
        vec4 tap(sampler2D s) { return texture2D(s, uv); }\n\
        void main() { gl_FragData[0] = tap(colortex0) + tap(colortex1); }\n";
    let t = pack(fragment).translate(&TranslateConfig::default());
    let translated = program(&t, "composite");
    let stage = fragment_stage(translated);
    let sampling_functions = stage
        .module
        .functions
        .iter()
        .filter(|(_, function)| {
            function
                .expressions
                .iter()
                .any(|(_, expression)| matches!(expression, naga::Expression::ImageSample { .. }))
        })
        .count();
    assert_eq!(sampling_functions, 2);
    assert_eq!(crate::reflection(translated).textures.len(), 2);
}

#[test]
fn overloads_taking_a_sampler_each_get_a_specialized_copy() {
    let fragment = "#version 120\n\
        #====================#\n\
        #   Copyright banner #\n\
        uniform sampler2D depthtex1;\n\
        varying vec2 uv;\n\
        vec4 trace(sampler2D depth, vec2 p, float s) { return texture2D(depth, p) * s; }\n\
        vec3 trace(sampler2D depth, vec2 p, float s, out float hit) { vec4 c = trace(depth, p, s); hit = c.a; return c.rgb; }\n\
        void main() { float hit; vec3 c = trace(depthtex1, uv, 2.0, hit); gl_FragData[0] = vec4(c, hit); }\n";
    let t = pack(fragment).translate(&TranslateConfig::default());
    let stage = fragment_stage(program(&t, "composite"));
    let mut arities: Vec<_> = stage
        .module
        .functions
        .iter()
        .map(|(_, function)| function.arguments.len())
        .filter(|arity| *arity > 0)
        .collect();
    arities.sort_unstable();
    assert_eq!(arities, [2, 3]);
}
