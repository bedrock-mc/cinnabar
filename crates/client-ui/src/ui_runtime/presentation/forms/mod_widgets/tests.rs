use super::super::{ServerUiPack, snapshot, tests::mini_engine_presentation};
use super::*;
use ui::{
    DpiScale,
    mod_hud::{Anchor, Card, Row},
};

/// Loads the same real font, HUD, item icons and JSON-UI carriers used by the client.
fn installed_hud_presentation() -> Option<UiPresentationRuntime> {
    use assets::{RuntimeHudCatalog, RuntimeIconCatalog};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local");
    let read = |name: &str| {
        let path = root.join("assets/compiled").join(name);
        match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!(
                    "skipping installed personal HUD fixture: missing {}; make assets",
                    path.display()
                );
                None
            }
            Err(error) => panic!("read personal HUD fixture {}: {error}", path.display()),
        }
    };
    let hud = Arc::new(
        RuntimeHudCatalog::decode(&read(assets::carriers::HUD.output)?)
            .expect("decode installed HUD fixture"),
    );
    let icons = Arc::new(
        RuntimeIconCatalog::decode(&read(assets::carriers::ICON.output)?)
            .expect("decode installed item icon fixture"),
    );
    let font = Arc::new(
        (*super::super::pack_harness::font())
            .clone()
            .with_coverage_pages(),
    );
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(font, hud, icons)
        .expect("build installed HUD fixture");
    presentation
        .enable_json_ui(super::super::pack_harness::carrier()?)
        .expect("enable installed JSON-UI fixture");
    presentation
        .form_presentation
        .engine
        .as_mut()
        .unwrap()
        .textures
        .set_fallbacks(
            Default::default(),
            root.join(crate::install_layout::vanilla_pack_relative()),
        );
    Some(presentation)
}

fn content() -> Hud {
    Hud {
        cards: vec![Card {
            id: "equipment".into(),
            title: "Equipment".into(),
            anchor: Anchor::TopLeft,
            offset: [6., 6.],
            scale: 1.,
            rows: vec![Row {
                label: "Helmet".into(),
                value: "85%".into(),
                item: Some("minecraft:diamond_helmet".into()),
                effect_id: None,
                metadata: 0,
                progress: Some(0.85),
                color: [0.3, 1., 0.5, 1.],
            }],
        }],
    }
}
fn presentation(hidden: bool) -> UiPresentationRuntime {
    let mut presentation = mini_engine_presentation();
    let hud = serde_json::json!({"namespace":"hud","hud_screen":{"type":"screen","visible":!hidden,
        "controls":[{"label":{"type":"label","size":[100,12],"text":"Base HUD","anchor_from":"bottom_middle","anchor_to":"bottom_middle"}}]}});
    presentation.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec(),
            ),
            (
                "ui/hud_screen.json".into(),
                serde_json::to_vec(&hud).unwrap(),
            ),
        ]],
        ..Default::default()
    });
    presentation
}
fn frame(
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    size: [u32; 2],
    dpi: f32,
) -> render_model::UiRenderInput {
    presentation
        .build(
            &player_state::PlayerState::new(1),
            runtime,
            0,
            size,
            DpiScale::new(dpi).unwrap(),
        )
        .unwrap()
}
#[test]
fn cards_update_values_without_rebuilding_the_json_ui_catalog_and_revoke_cleanly() {
    let mut p = presentation(false);
    let runtime = UiRuntime::new(1);
    let before = frame(&mut p, &runtime, [1280, 720], 1.);
    let mut hud = content();
    p.set_mod_hud(Some(&hud)).unwrap();
    let after = frame(&mut p, &runtime, [1280, 720], 1.);
    assert!(
        snapshot::rasterize(&before) != snapshot::rasterize(&after),
        "published cards change the rendered frame"
    );
    let catalog = Arc::clone(&p.form_presentation.mod_widgets.as_ref().unwrap().catalog);
    assert!(
        after == frame(&mut p, &runtime, [1280, 720], 1.),
        "unchanged content reuses the published frame"
    );
    hud.cards[0].rows[0].value = "41%".into();
    hud.cards[0].rows[0].progress = Some(0.41);
    p.set_mod_hud(Some(&hud)).unwrap();
    assert!(Arc::ptr_eq(
        &catalog,
        &p.form_presentation.mod_widgets.as_ref().unwrap().catalog
    ));
    let updated = frame(&mut p, &runtime, [1280, 720], 1.);
    assert!(
        snapshot::rasterize(&after) != snapshot::rasterize(&updated),
        "new values change the rendered cards"
    );
    p.set_mod_hud(None).unwrap();
    let restored = frame(&mut p, &runtime, [1280, 720], 1.);
    assert!(
        snapshot::rasterize(&before) == snapshot::rasterize(&restored),
        "revoking cards restores the original pixels"
    );
}
#[test]
fn cards_cannot_bypass_inventory_chat_loading_server_or_player_hud_visibility() {
    for gate in 0..5 {
        let mut p = presentation(gate == 0);
        let mut runtime = UiRuntime::new(1);
        match gate {
            1 => runtime.inventory_open = true,
            2 => runtime.chat_focused = true,
            3 => p.loading_stage = Some(super::super::LoadingStage::Connecting),
            4 => {
                let mut options = crate::menu::settings_options::SettingsOptions::default();
                let index = crate::menu::settings_options::SETTINGS_OPTIONS
                    .iter()
                    .position(|option| option.name == "hide_hud")
                    .unwrap();
                options.set(index, 1);
                p.set_chat_settings_snapshot((Arc::new(options), None));
            }
            _ => {}
        }
        let before = frame(&mut p, &runtime, [1280, 720], 1.);
        p.set_mod_hud(Some(&content())).unwrap();
        let gated = frame(&mut p, &runtime, [1280, 720], 1.);
        assert!(
            snapshot::rasterize(&before) == snapshot::rasterize(&gated),
            "gate {gate} hides the personal cards"
        );
    }
}
#[test]
fn anchored_card_bounds_and_icons_scale_together_across_viewports() {
    for (size, dpi, scale) in [
        ([1280, 720], 1., 0.5),
        ([1920, 1080], 1.5, 1.),
        ([2560, 1440], 2., 2.),
    ] {
        for anchor in [
            Anchor::TopLeft,
            Anchor::TopRight,
            Anchor::BottomLeft,
            Anchor::BottomRight,
        ] {
            let mut p = presentation(false);
            let mut hud = content();
            hud.cards[0].anchor = anchor;
            hud.cards[0].scale = scale;
            hud.cards[0].offset = match anchor {
                Anchor::TopLeft => [6., 6.],
                Anchor::TopRight => [-6., 6.],
                Anchor::BottomLeft => [6., -6.],
                Anchor::BottomRight => [-6., -6.],
            };
            p.set_mod_hud(Some(&hud)).unwrap();
            frame(&mut p, &UiRuntime::new(1), size, dpi);
            let nodes = &p.last_frame.as_ref().unwrap().nodes;
            let card = nodes
                .iter()
                .filter(|node| matches!(node.visual(), ui::UiVisual::Mesh(_)))
                .next()
                .expect("card surface");
            let rect = card.bounds();
            assert!(rect.min().x() >= 0. && rect.min().y() >= 0.);
            assert!(
                rect.max().x() <= size[0] as f32 / dpi && rect.max().y() <= size[1] as f32 / dpi
            );
        }
    }
}
#[test]
fn malformed_presentation_does_not_replace_the_existing_cards_or_crosshair() {
    let mut p = presentation(false);
    let hud = content();
    p.set_mod_hud(Some(&hud)).unwrap();
    let mut invalid = hud.clone();
    invalid.cards[0].scale = f32::NAN;
    assert!(p.set_mod_hud(Some(&invalid)).is_err());
    assert_eq!(
        p.form_presentation.mod_widgets.as_ref().unwrap().content,
        hud
    );
    let cursor = Crosshair::default();
    p.set_mod_crosshair(Some(&cursor)).unwrap();
    let invalid = Crosshair {
        gap: -1.,
        ..Default::default()
    };
    assert!(p.set_mod_crosshair(Some(&invalid)).is_err());
    assert_eq!(p.form_presentation.mod_crosshair, Some(cursor));
    p.set_mod_crosshair(None).unwrap();
    assert!(p.form_presentation.mod_crosshair.is_none());
}

#[test]
fn personal_hud_snapshot_with_real_carrier() {
    let Some(mut p) = installed_hud_presentation() else {
        eprintln!(
            "skipping personal_hud_snapshot_with_real_carrier: installed local carriers unavailable (make assets)"
        );
        return;
    };
    let runtime = UiRuntime::new(1);
    p.hud_frame_mut().first_person = true;
    let before = frame(&mut p, &runtime, [1920, 1080], 1.);
    snapshot::write(&before, "personal-hud-before");
    let equipment = Card {
        id: "equipment".into(),
        title: "Equipment".into(),
        anchor: Anchor::TopLeft,
        offset: [8., 24.],
        scale: 1.,
        rows: [
            ("Helmet", "minecraft:diamond_helmet", "65 / 363", 0.18),
            (
                "Chestplate",
                "minecraft:diamond_chestplate",
                "401 / 528",
                0.76,
            ),
            ("Leggings", "minecraft:diamond_leggings", "422 / 495", 0.85),
            ("Boots", "minecraft:diamond_boots", "429 / 429", 1.),
        ]
        .into_iter()
        .map(|(label, item, value, progress)| Row {
            label: label.into(),
            value: value.into(),
            item: Some(item.into()),
            effect_id: None,
            metadata: 0,
            progress: Some(progress),
            color: if progress < 0.2 {
                [1., 0.35, 0.3, 1.]
            } else {
                [0.3, 0.9, 0.5, 1.]
            },
        })
        .collect(),
    };
    let effects = Card {
        id: "effects".into(),
        title: "Effects".into(),
        anchor: Anchor::TopRight,
        offset: [-8., 24.],
        scale: 1.,
        rows: [("Speed II", 1, "1:42"), ("Fire Resistance", 12, "6:15")]
            .into_iter()
            .map(|(label, id, value)| Row {
                label: label.into(),
                value: value.into(),
                item: None,
                effect_id: Some(id),
                metadata: 0,
                progress: None,
                color: [1.; 4],
            })
            .collect(),
    };
    let supplies = Card {
        id: "supplies".into(),
        title: "Supplies".into(),
        anchor: Anchor::BottomLeft,
        offset: [8., -8.],
        scale: 1.,
        rows: [
            ("Healing pots", "minecraft:splash_potion", 22, "12"),
            ("Pearls", "minecraft:ender_pearl", 0, "16"),
            ("Arrows", "minecraft:arrow", 0, "64"),
            ("Blocks", "minecraft:stone", 0, "192"),
        ]
        .into_iter()
        .map(|(label, item, metadata, value)| Row {
            label: label.into(),
            value: value.into(),
            item: Some(item.into()),
            effect_id: None,
            metadata,
            progress: None,
            color: [1.; 4],
        })
        .collect(),
    };
    let hud = Hud {
        cards: vec![equipment, effects, supplies],
    };
    p.set_mod_hud(Some(&hud)).unwrap();
    for shape in [
        ui::mod_hud::CrosshairShape::Cross,
        ui::mod_hud::CrosshairShape::Dot,
        ui::mod_hud::CrosshairShape::Circle,
    ] {
        p.set_mod_crosshair(Some(&Crosshair {
            shape,
            color: [0.3, 1., 0.5, 1.],
            size: 4.,
            ..Default::default()
        }))
        .unwrap();
        let after = frame(&mut p, &runtime, [1920, 1080], 1.);
        snapshot::write(&after, &format!("personal-hud-after-{shape:?}"));
        assert!(
            snapshot::rasterize(&before) != snapshot::rasterize(&after),
            "personal HUD capture renders the enabled presentation"
        );
    }
    p.set_mod_hud(None).unwrap();
    p.set_mod_crosshair(None).unwrap();
    let restored = frame(&mut p, &runtime, [1920, 1080], 1.);
    snapshot::write(&restored, "personal-hud-restored");
    assert!(
        snapshot::rasterize(&before) == snapshot::rasterize(&restored),
        "clearing personal HUD cards and cursor restores the original pixels"
    );
}

#[test]
fn custom_cursor_preserves_camera_spectator_hidden_hud_and_menu_gates() {
    use crate::menu::settings_options::{
        SETTINGS_OPTIONS, SettingsOptions, THIRD_PERSON_CROSSHAIR_OPTION,
    };
    use protocol::PlayerGameMode;
    let Some(mut p) = installed_hud_presentation() else {
        eprintln!(
            "skipping custom_cursor_preserves_camera_spectator_hidden_hud_and_menu_gates: installed local carriers unavailable (make assets)"
        );
        return;
    };
    let runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    p.set_mod_crosshair(Some(&Crosshair::default())).unwrap();
    for first_person in [false, true] {
        for third_person in [false, true] {
            for mode in [
                PlayerGameMode::Survival,
                PlayerGameMode::Creative,
                PlayerGameMode::Spectator,
            ] {
                for hidden in [false, true] {
                    let mut options = SettingsOptions::default();
                    for (name, value) in [
                        (THIRD_PERSON_CROSSHAIR_OPTION.name, third_person),
                        ("hide_hud", hidden),
                    ] {
                        let index = SETTINGS_OPTIONS
                            .iter()
                            .position(|o| o.name == name)
                            .unwrap();
                        options.set(index, i32::from(value));
                    }
                    player.facts.publish_player_game_mode(mode);
                    p.hud_frame_mut().first_person = first_person;
                    p.set_chat_settings_snapshot((Arc::new(options), None));
                    p.build(
                        &player,
                        &runtime,
                        0,
                        [1280, 720],
                        DpiScale::new(1.).unwrap(),
                    )
                    .unwrap();
                    let custom = p.last_frame.as_ref().unwrap().nodes.iter().any(|node| {
                        let bounds = node.bounds();
                        matches!(node.visual(), ui::UiVisual::Mesh(_))
                            && ((bounds.min().x() + bounds.max().x()) * 0.5 - 640.).abs() < 0.01
                            && ((bounds.min().y() + bounds.max().y()) * 0.5 - 360.).abs() < 0.01
                    });
                    assert_eq!(
                        custom,
                        (first_person || third_person)
                            && mode != PlayerGameMode::Spectator
                            && !hidden,
                        "first person={first_person} third person={third_person} mode={mode:?} hidden={hidden}"
                    );
                }
            }
        }
    }
    p.set_menu_view(Some(crate::menu::MenuView::new(true, "Fixture".into())));
    assert!(!p.mod_hud_visible(&player, &runtime));
}

#[test]
fn progress_fill_stays_within_its_track_for_icon_rows_and_each_card_scale() {
    fn mesh_bounds(presentation: &UiPresentationRuntime, color: [u8; 4]) -> Option<ui::UiRect> {
        presentation
            .last_frame
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find_map(|node| {
                let ui::UiVisual::Mesh(mesh) = node.visual() else {
                    return None;
                };
                mesh.vertices()
                    .iter()
                    .any(|vertex| vertex.color == color)
                    .then(|| node.bounds())
            })
    }
    for scale in [0.5, 1., 2.] {
        for icon in [false, true] {
            let mut presentation = presentation(false);
            let mut hud = content();
            hud.cards[0].scale = scale;
            if !icon {
                hud.cards[0].rows[0].item = None;
            }
            hud.cards[0].rows[0].color = [1., 0., 0., 1.];
            for progress in [0., 0.5, 1.] {
                hud.cards[0].rows[0].progress = Some(progress);
                presentation.set_mod_hud(Some(&hud)).unwrap();
                frame(&mut presentation, &UiRuntime::new(1), [1280, 720], 1.);
                let track =
                    mesh_bounds(&presentation, [255, 255, 255, 46]).expect("progress track");
                let fill = mesh_bounds(&presentation, [255, 0, 0, 255]);
                if progress == 0. {
                    assert!(fill.is_none(), "zero progress draws no fill");
                    continue;
                }
                let fill = fill.expect("positive progress fill");
                let width = |bounds: ui::UiRect| bounds.max().x() - bounds.min().x();
                assert_eq!(fill.min(), track.min());
                assert!(fill.max().x() <= track.max().x());
                assert!(
                    (width(fill) - width(track) * progress).abs() < 0.01,
                    "scale={scale}, icon={icon}, progress={progress}, fill={fill:?}, track={track:?}"
                );
            }
        }
    }
}
