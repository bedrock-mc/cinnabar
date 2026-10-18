use super::*;

#[test]
fn budget_rejected_runs_do_not_materialize_visuals_or_item_indices() {
    let admitted = block("test:admitted", 1, CustomBlockVisuals::default());
    let rejected = block(
        "test:rejected",
        u32::MAX,
        CustomBlockVisuals {
            state_identity_incomplete: true,
            materialization_disabled: true,
            ..Default::default()
        },
    );
    let definitions = CustomBlocks {
        blocks: Arc::from([admitted, rejected]),
        ..Default::default()
    };
    for hashed in [false, true] {
        let compiled = compile_block_overlay(&view(), &definitions, hashed, None).unwrap();
        assert_eq!(compiled.overlay.visuals.len(), 1);
        assert_eq!(compiled.overlay.hashes.len(), 1);
        assert_eq!(compiled.gaps.incomplete_state_identities, 1);
        let icons = crate::runtime::network::item_icons::custom_block_icons(
            &compiled.overlay,
            &definitions,
            hashed,
            &[("test:rejected".into(), "test:rejected".into())],
            None,
        );
        assert!(icons.icons.is_empty());
        assert!(icons.block_sheets.is_empty());
        assert_eq!(icons.misses.len(), 1);
    }
}
