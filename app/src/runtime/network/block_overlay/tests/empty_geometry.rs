use super::*;

/// A cubeless geometry draws nothing: packs pair it with a null texture to
/// hide a state, and the client must not paint it diagnostic purple.
#[test]
fn empty_geometry_is_invisible_without_gaps() {
    let view = view_with_geometry(
        br#"{"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.empty","texture_width":16,"texture_height":16}
    }]}"#,
    );
    let hidden = block(
        "test:hidden",
        1,
        CustomBlockVisuals {
            base: CustomVisualComponents {
                geometry: Some("geometry.empty".into()),
                materials: materials("lucky"),
                ..CustomVisualComponents::default()
            },
            ..CustomBlockVisuals::default()
        },
    );
    let blocks = CustomBlocks {
        blocks: vec![hidden].into(),
        ..CustomBlocks::default()
    };
    for hashed in [false, true] {
        let compiled = compile_block_overlay(&view, &blocks, hashed, None).expect("overlay");
        assert_eq!(
            compiled.gaps,
            OverlayGaps::default(),
            "hiding a state counts no gap"
        );
        assert_eq!(compiled.overlay.visuals.len(), 1);
        assert_eq!(
            compiled.overlay.visuals[0].kind,
            VisualKind::Invisible,
            "a cubeless geometry draws nothing"
        );
        assert_eq!(compiled.overlay.visuals[0].support, VisualSupport::Exact);
        RuntimeAssets::diagnostic()
            .with_block_overlay(1, &compiled.overlay)
            .expect("invisible visuals apply");
    }
}

/// Rejected cube bounds keep the diagnostic visual instead of hiding the block.
#[test]
fn all_rejected_cubes_preserve_diagnostic_fallback() {
    let mut document: serde_json::Value = serde_json::from_str(GEOMETRY).unwrap();
    document["minecraft:geometry"][0]["bones"][0]["cubes"] = serde_json::json!([
        {"origin": [1e38, 0, 0], "size": [3e38, 1, 1], "uv": [0, 0]}
    ]);
    let view = view_with_geometry(&serde_json::to_vec(&document).unwrap());
    let blocks = CustomBlocks {
        blocks: vec![block(
            "test:rejected",
            1,
            CustomBlockVisuals {
                base: CustomVisualComponents {
                    geometry: Some("geometry.gen".into()),
                    materials: materials("lucky"),
                    ..CustomVisualComponents::default()
                },
                ..CustomBlockVisuals::default()
            },
        )]
        .into(),
        ..CustomBlocks::default()
    };
    for hashed in [false, true] {
        let compiled = compile_block_overlay(&view, &blocks, hashed, None).expect("overlay");
        assert_eq!(
            compiled.gaps,
            OverlayGaps {
                skipped_cubes: 1,
                ..OverlayGaps::default()
            }
        );
        assert_eq!(compiled.overlay.visuals.len(), 1);
        assert_eq!(compiled.overlay.visuals[0].kind, VisualKind::Diagnostic);
        assert_eq!(
            compiled.overlay.visuals[0].support,
            VisualSupport::Diagnostic
        );
        assert!(compiled.overlay.model_templates.is_empty());
        assert!(compiled.overlay.model_quads.is_empty());
        RuntimeAssets::diagnostic()
            .with_block_overlay(1, &compiled.overlay)
            .expect("diagnostic visuals apply");
    }
}

/// Hiding every valid cube still draws nothing when another cube was rejected.
#[test]
fn hidden_valid_cubes_remain_invisible_with_rejected_cubes() {
    let mut document: serde_json::Value = serde_json::from_str(BONE_GEOMETRY).unwrap();
    document["minecraft:geometry"][0]["bones"] = serde_json::json!([
        {"name": "a", "cubes": [
            {"origin": [1e38, 0, 0], "size": [3e38, 1, 1], "uv": [0, 0]},
            {"origin": [-8, 0, -8], "size": [16, 16, 16], "uv": [0, 0]}
        ]}
    ]);
    let view = view_with_geometry(&serde_json::to_vec(&document).unwrap());
    let blocks = CustomBlocks {
        blocks: vec![block(
            "test:hidden",
            1,
            CustomBlockVisuals {
                base: bone_components(&[("a", "0")]),
                ..CustomBlockVisuals::default()
            },
        )]
        .into(),
        ..CustomBlocks::default()
    };
    for hashed in [false, true] {
        let compiled = compile_block_overlay(&view, &blocks, hashed, None).expect("overlay");
        assert_eq!(
            compiled.gaps,
            OverlayGaps {
                skipped_cubes: 1,
                ..OverlayGaps::default()
            }
        );
        assert_eq!(compiled.overlay.visuals.len(), 1);
        assert_eq!(compiled.overlay.visuals[0].kind, VisualKind::Invisible);
        assert_eq!(compiled.overlay.visuals[0].support, VisualSupport::Exact);
        RuntimeAssets::diagnostic()
            .with_block_overlay(1, &compiled.overlay)
            .expect("invisible visuals apply");
    }
}
