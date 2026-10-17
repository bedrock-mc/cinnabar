//! `#include` expansion and diagnostics located in included files.

use crate::{open_fixture, program, reflection, texture_names};
use shaderpack::{Phase, SourceLocation, StageKind, TranslateConfig};

#[test]
fn absolute_relative_and_nested_includes_expand_into_one_stage() {
    let Some(pack) = open_fixture("includes") else {
        return;
    };
    let t = pack.translate(&TranslateConfig::default());
    let composite = program(&t, "composite");
    let r = reflection(composite);
    assert_eq!(texture_names(r), ["colortex0"]);
    // `/lib/common.glsl` → relative `nested/curves.glsl` → absolute `/lib/constants.glsl`.
    let fragment = composite
        .stages
        .iter()
        .find(|s| s.stage == StageKind::Fragment)
        .unwrap();
    let module = &fragment.result.as_ref().unwrap().module;
    let functions: Vec<&str> = module
        .functions
        .iter()
        .filter_map(|(_, f)| f.name.as_deref())
        .collect();
    assert!(functions.contains(&"gradeColor"), "{functions:?}");
    assert!(functions.contains(&"softCurve"), "{functions:?}");
    // The vertex stage is the relative include `lib/fullscreen.glsl` of the root file.
    let vertex = composite
        .stages
        .iter()
        .find(|s| s.stage == StageKind::Vertex)
        .unwrap();
    assert_eq!(vertex.path, "composite.vsh");
    assert!(vertex.result.is_ok());
    assert_eq!(r.color_outputs.len(), 1);
}

#[test]
fn an_error_in_an_included_file_is_located_there_and_fails_only_its_program() {
    let Some(pack) = open_fixture("includes") else {
        return;
    };
    let t = pack.translate(&TranslateConfig::default());

    let broken = program(&t, "composite1");
    assert!(broken.enabled);
    assert!(!broken.is_ok());
    let (vertex, fragment) = (&broken.stages[0], &broken.stages[1]);
    assert_eq!(vertex.stage, StageKind::Vertex);
    assert!(
        vertex.result.is_ok(),
        "the stage without the mistake still compiles"
    );
    let errors = fragment.result.as_ref().expect_err("fragment stage fails");
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!(errors[0].phase, Phase::Parse);
    assert!(
        errors[0].message.contains("missingDetailTerm"),
        "{}",
        errors[0].message
    );
    assert_eq!(
        errors[0].location,
        Some(SourceLocation {
            file: "lib/broken.glsl".into(),
            line: 4
        })
    );
    assert!(
        errors[0].snippet.is_none(),
        "located errors carry no generated snippet"
    );

    let missing = program(&t, "composite2");
    assert!(!missing.is_ok());
    let errors: Vec<_> = missing.all_diagnostics().collect();
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!(errors[0].phase, Phase::Include);
    assert!(
        errors[0].message.contains("lib/does_not_exist.glsl"),
        "{}",
        errors[0].message
    );
    assert_eq!(
        errors[0].location,
        Some(SourceLocation {
            file: "composite2.fsh".into(),
            line: 5
        }),
        "a missing include points at the #include line"
    );

    // The healthy program in the same pack is unaffected.
    assert!(program(&t, "composite").is_ok());
    let names: Vec<&str> = t.programs.iter().map(|p| p.program.name.as_str()).collect();
    assert_eq!(names, ["composite", "composite1", "composite2"]);
}
