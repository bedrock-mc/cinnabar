use crate::ui_runtime::presentation::forms::{ServerUiPack, pack_harness, snapshot};

#[test]
fn inventory_pack_replacement_preserves_pixels_and_rechecks_virtualization() {
    let Some(mut presentation) =
        crate::test_support::engine_presentation_with(pack_harness::font())
    else {
        eprintln!(
            "skipping inventory_pack_replacement_preserves_pixels_and_rechecks_virtualization: missing local UI carrier; make assets"
        );
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::creative_with(&mut player, 256);
    runtime.select_inventory_layout(&mut player, 2).unwrap();
    runtime
        .screen_state_mut()
        .select_tab(ui_screens::container_actions::SEARCH_TAB);
    let mut original = None;
    for modified in [false, true, false] {
        let files = if modified {
            vec![
                (
                    "ui/_ui_defs.json".to_owned(),
                    br#"{"ui_defs":["custom/inventory_override.json"]}"#.to_vec(),
                ),
                (
                    "custom/inventory_override.json".to_owned(),
                    br#"{"namespace":"crafting","inventory_screen":{"fixture_extension":true}}"#
                        .to_vec(),
                ),
            ]
        } else {
            vec![(
                "ui/hud_screen.json".to_owned(),
                br#"{"namespace":"hud","fixture_label":{"type":"label","text":"Fixture"}}"#
                    .to_vec(),
            )]
        };
        runtime.set_server_ui(Some(
            ServerUiPack {
                ui_layers: vec![files],
                ..Default::default()
            }
            .prepare_catalog(&presentation.pack_catalog_base().unwrap()),
        ));
        let mut input = None;
        for now in [0, 500, 1000] {
            input = Some(
                presentation
                    .build(
                        &player,
                        &runtime,
                        now,
                        [1280, 720],
                        ui::DpiScale::new(1.0).unwrap(),
                    )
                    .unwrap(),
            );
        }
        let raster = snapshot::rasterize(&input.unwrap());
        if let Some(original) = &original {
            assert!(
                *original == raster,
                "full and virtualized cells must draw identical pixels"
            );
        } else {
            original = Some(raster);
        }
        assert_eq!(
            presentation
                .engine_container_frame()
                .unwrap()
                .report
                .grid_cells
                .is_empty(),
            modified
        );
    }
}

#[test]
fn unrelated_server_ui_keeps_creative_search_virtualized() {
    let Some(mut presentation) =
        crate::test_support::engine_presentation_with(pack_harness::font())
    else {
        eprintln!(
            "skipping unrelated_server_ui_keeps_creative_search_virtualized: missing local UI carrier; make assets"
        );
        return;
    };
    let files = vec![(
        "ui/hud_screen.json".to_owned(),
        br#"{"namespace":"hud","test_label":{"type":"label","text":"Fixture"}}"#.to_vec(),
    )];
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::creative_with(&mut player, 2048);
    runtime.select_inventory_layout(&mut player, 2).unwrap();
    runtime
        .screen_state_mut()
        .select_tab(ui_screens::container_actions::SEARCH_TAB);
    runtime.set_server_ui(Some(
        ServerUiPack {
            ui_layers: vec![files],
            ..Default::default()
        }
        .prepare_catalog(&presentation.pack_catalog_base().unwrap()),
    ));
    for now in [0, 500] {
        presentation
            .build(
                &player,
                &runtime,
                now,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    }
    let scroll = presentation
        .engine_container_frame()
        .unwrap()
        .report
        .scrolls
        .iter()
        .max_by(|a, b| a.1.content.total_cmp(&b.1.content))
        .unwrap()
        .0
        .clone();
    for now in 1000..1030 {
        runtime
            .screen_state_mut()
            .container_scroll
            .insert(scroll.clone(), if now % 2 == 0 { 0.0 } else { 90.0 });
        let input = presentation
            .build(
                &player,
                &runtime,
                now,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        if now == 1029 {
            snapshot::write(&input, "creative-search-server-ui");
        }
    }
    assert!(
        !presentation
            .engine_container_frame()
            .unwrap()
            .report
            .grid_cells
            .is_empty(),
        "unchanged creative cells should stay virtualized under unrelated server UI"
    );
}
