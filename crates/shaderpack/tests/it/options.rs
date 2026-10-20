//! Option discovery, overrides, and `program.*.enabled` rules.

use crate::{open_fixture, program, reflection, texture_names, uniform_names};
use shaderpack::{
    ConstDirective, OptionKind, OptionValues, PackOption, ProgramReflection, ShaderPack,
    TranslateConfig,
};

fn values(pairs: &[(&str, &str)]) -> OptionValues {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// (element count, stride) of the `shadowWeights` uniform array.
fn shadow_weights(r: &ProgramReflection) -> Option<(u32, u32)> {
    r.uniforms
        .as_ref()?
        .members
        .iter()
        .find(|m| m.name == "shadowWeights")?
        .array
}

fn sun_path_rotation(consts: &[ConstDirective]) -> Option<&str> {
    consts
        .iter()
        .find(|c| c.name == "sunPathRotation")
        .map(|c| c.value.as_str())
}

#[test]
fn options_are_discovered_with_their_defaults() {
    let Some(pack) = open_fixture("options") else {
        return;
    };
    let option = |name: &str, line: u32, kind: OptionKind| PackOption {
        name: name.into(),
        kind,
        file: "lib/settings.glsl".into(),
        line,
    };
    assert_eq!(
        pack.options(),
        [
            option(
                "SHADOW_TAPS",
                5,
                OptionKind::Value {
                    default: "4".into(),
                    allowed: vec!["2".into(), "4".into(), "8".into()],
                }
            ),
            option("SOFT_SHADOWS", 3, OptionKind::Bool { default: true }),
            option("VOLUMETRIC_FOG", 4, OptionKind::Bool { default: false }),
            option(
                "sunPathRotation",
                6,
                OptionKind::Const {
                    ty: "float".into(),
                    default: "-30.0".into(),
                    allowed: vec!["-30.0".into(), "0.0".into(), "30.0".into()],
                }
            ),
        ]
    );
    let config = TranslateConfig::default();
    let defines = pack.defines(&config.environment, &config.options);
    assert_eq!(defines.get("SOFT_SHADOWS").map(String::as_str), Some(""));
    assert_eq!(defines.get("SHADOW_TAPS").map(String::as_str), Some("4"));
    assert!(!defines.contains_key("VOLUMETRIC_FOG"));
    assert_eq!(
        defines.get("sunPathRotation").map(String::as_str),
        Some("-30.0"),
        "const options are property macros, as in Iris"
    );
}

#[test]
fn defaults_compile_the_default_variant_and_skip_the_disabled_program() {
    let Some(pack) = open_fixture("options") else {
        return;
    };
    let t = pack.translate(&TranslateConfig::default());
    let composite = program(&t, "composite");
    let r = reflection(composite);
    assert_eq!(texture_names(r), ["colortex0"]);
    assert_eq!(
        uniform_names(r),
        [
            "iris_ModelViewMat",
            "iris_ProjMat",
            "shadowWeights",
            "penumbraScale"
        ]
    );
    assert_eq!(
        shadow_weights(r),
        Some((4, 16)),
        "std140 float arrays stride 16"
    );
    assert_eq!(sun_path_rotation(&composite.consts), Some("-30.0"));

    assert_eq!(t.properties.program_rules.len(), 1);
    let fog = program(&t, "composite1");
    assert!(!fog.enabled);
    assert!(fog.forced.is_empty());
    assert!(
        fog.stages.is_empty(),
        "a disabled program is not translated"
    );
    assert!(fog.reflection.is_none());
    assert_eq!(fog.all_diagnostics().count(), 0);
}

#[test]
fn option_overrides_change_the_compiled_program_and_enable_rules() {
    let Some(pack) = open_fixture("options") else {
        return;
    };
    let config = TranslateConfig {
        options: values(&[
            ("VOLUMETRIC_FOG", "true"),
            ("SOFT_SHADOWS", "false"),
            ("SHADOW_TAPS", "8"),
            ("sunPathRotation", "30.0"),
        ]),
        ..TranslateConfig::default()
    };
    let t = pack.translate(&config);
    let composite = program(&t, "composite");
    let r = reflection(composite);
    assert_eq!(texture_names(r), ["colortex0", "noisetex"]);
    assert_eq!(r.textures[1].binding, 3);
    assert_eq!(
        uniform_names(r),
        ["iris_ModelViewMat", "iris_ProjMat", "shadowWeights"]
    );
    assert_eq!(shadow_weights(r), Some((8, 16)));
    assert_eq!(sun_path_rotation(&composite.consts), Some("30.0"));

    let fog = program(&t, "composite1");
    assert!(fog.enabled);
    assert!(fog.forced.is_empty());
    assert_eq!(texture_names(reflection(fog)), ["colortex0", "noisetex"]);
}

#[test]
fn force_mode_translates_a_disabled_program_with_the_options_its_rule_needs() {
    let Some(pack) = open_fixture("options") else {
        return;
    };
    let config = TranslateConfig {
        force_disabled: true,
        ..TranslateConfig::default()
    };
    let t = pack.translate(&config);
    let fog = program(&t, "composite1");
    assert!(
        !fog.enabled,
        "still reported as disabled under the chosen options"
    );
    assert_eq!(
        fog.forced,
        [("VOLUMETRIC_FOG".to_string(), "true".to_string())]
    );
    assert_eq!(fog.stages.len(), 2);
    assert_eq!(texture_names(reflection(fog)), ["colortex0", "noisetex"]);

    // Forcing one program leaves the others on the player's options.
    let composite = program(&t, "composite");
    assert_eq!(texture_names(reflection(composite)), ["colortex0"]);
}

#[test]
fn unknown_override_values_are_passed_through() {
    let Some(pack) = open_fixture("options") else {
        return;
    };
    let config = TranslateConfig {
        options: values(&[("SHADOW_TAPS", "3")]),
        ..TranslateConfig::default()
    };
    let t = ShaderPack::translate(&pack, &config);
    assert_eq!(
        shadow_weights(reflection(program(&t, "composite"))),
        Some((3, 16))
    );
}
