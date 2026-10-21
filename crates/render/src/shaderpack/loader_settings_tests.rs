//! Pack-wide settings and storage binding size regressions.

use super::*;

/// A program with one pack-wide setting, independently of host execution support.
fn setting_program(group: ProgramGroup, enabled: bool, value: &str) -> ProgramTranslation {
    ProgramTranslation {
        program: shaderpack::ProgramSource {
            name: String::new(),
            group,
            kind: ProgramKind::Render,
            folder: String::new(),
            stages: vec![],
        },
        enabled,
        forced: vec![],
        stages: vec![],
        diagnostics: vec![],
        reflection: None,
        draw_buffers: vec![],
        consts: vec![shaderpack::ConstDirective {
            ty: "float".into(),
            name: "sunPathRotation".into(),
            value: value.into(),
        }],
    }
}

#[test]
fn pack_settings_include_enabled_geometry_and_shadow_programs() {
    let programs = [
        setting_program(ProgramGroup::Gbuffers, true, "30.0"),
        setting_program(ProgramGroup::Shadow, true, "45.0"),
        setting_program(ProgramGroup::Composite, false, "90.0"),
    ];
    assert_eq!(pack_settings(&programs).sun_path_rotation, 45.0);
}

#[test]
fn storage_binding_lengths_keep_complete_declared_words() {
    for declared in [20, 21, 23] {
        assert_eq!(storage_binding_size(4, declared).unwrap(), 20);
    }
    assert_eq!(storage_binding_size(16, 4).unwrap(), 16);
    assert!(storage_binding_size(4, u64::MAX).is_err());
}
