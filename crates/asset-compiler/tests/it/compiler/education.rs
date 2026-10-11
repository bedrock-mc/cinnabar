use {
    super::support::*,
    assets::{BlockFlags, DIAGNOSTIC_MATERIAL, ModelFamily, ModelStateField, VisualKind},
};

#[test]
fn education_construction_blocks_compile_their_pack_faces_and_border_wall() {
    let directory = tempfile::tempdir().unwrap();
    write_pack(
        directory.path(),
        r#"{"allow":{"textures":"build_allow"},"deny":{"textures":"build_deny"},"border_block":{"textures":"border_block"}}"#,
        r#"{"texture_data":{"build_allow":{"textures":"textures/blocks/build_allow"},"build_deny":{"textures":"textures/blocks/build_deny"},"border_block":{"textures":"textures/blocks/border"}}}"#,
        "[]",
    );
    for (path, color) in [
        ("build_allow", [10, 20, 30, 255]),
        ("build_deny", [40, 50, 60, 255]),
        ("border", [70, 80, 90, 255]),
    ] {
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, color),
        );
    }
    let mut allow = encoded_model_record(1, 1001, "minecraft:allow", ModelFamily::Unknown, &[]);
    let mut deny = encoded_model_record(2, 1002, "minecraft:deny", ModelFamily::Unknown, &[]);
    allow.canonical_state = "{}".into();
    deny.canonical_state = "{}".into();
    let border = encoded_model_record(
        3,
        1003,
        "minecraft:border_block",
        ModelFamily::Wall,
        &[(ModelStateField::Connections, 1 << 8)],
    );
    let compiled = compile_pack(directory.path(), &[allow, deny, border]).unwrap();
    for id in [1, 2] {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Cube);
        assert!(
            visual
                .faces
                .into_iter()
                .all(|material| material != DIAGNOSTIC_MATERIAL)
        );
        assert!(visual.flags.contains(BlockFlags::OCCLUDES_FULL_FACE));
    }
    let visual = compiled.visuals[3];
    assert_eq!(visual.kind, VisualKind::Model);
    assert!(
        visual
            .faces
            .into_iter()
            .all(|material| material != DIAGNOSTIC_MATERIAL)
    );
    assert_eq!(
        model_bounds(template_quads(&compiled, visual.model_template)),
        ([64, 0, 64], [192, 256, 192])
    );
}
