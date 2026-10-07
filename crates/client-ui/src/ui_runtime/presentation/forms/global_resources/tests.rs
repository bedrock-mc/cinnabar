//! Offline Global Resources evidence through the real carrier and JSON-UI engine.
use super::super::{pack_harness, snapshot};
use crate::{
    global_resources::{Action, Snapshot},
    menu::{MenuAction, MenuScreen},
    ui_runtime::UiRuntime,
};
use std::sync::Arc;

/// A synthetic pack description; no third-party content is embedded.
fn fixture() -> resource_pack::InstalledPack {
    resource_pack::InstalledPack {
        id: "11111111-1111-4111-8111-111111111111".parse().unwrap(),
        version: [1, 0, 0],
        name: "Copper Test Pack".into(),
        description: "A local resource pack for reload verification.".into(),
        min_engine_version: None,
        revision: 0,
        subpacks: vec![
            resource_pack::Subpack {
                folder: "low".into(),
                name: "Low resolution".into(),
                memory_tier: 0,
            },
            resource_pack::Subpack {
                folder: "high".into(),
                name: "High resolution".into(),
                memory_tier: 2,
            },
        ],
    }
}

#[test]
fn global_resources_screen_renders_actions_and_pack_settings() {
    let player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = pack_harness::engine_presentation() else {
        eprintln!(
            "skipping global_resources_screen_renders_actions_and_pack_settings: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut view = crate::menu::MenuView::new(true, "Steve".into());
    view.screen = MenuScreen::Settings;
    view.settings_section = 24;
    let pack = fixture();
    let mut state = Snapshot {
        active: vec![pack.clone()],
        selection: vec![resource_pack::ActivePack {
            id: pack.id,
            subpack: "high".into(),
            revision: 0,
        }],
        selected: Some((true, 0)),
        details_expanded: Some((true, 0)),
        applied_selection: Some(Vec::new()),
        memory_tier: 2,
        ..Snapshot::default()
    };
    view.global_resources = Arc::new(state.clone());
    let mut runtime = UiRuntime::new(1);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("../.local/assets/compiled/vanilla-v1.mcbelang");
    if let Ok(bytes) = std::fs::read(root)
        && let Ok(lang) = assets::RuntimeLangCatalog::decode(&bytes)
    {
        runtime.set_lang_catalog(Arc::new(lang));
    }
    presentation.set_menu_view(Some(view.clone()));
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    snapshot::write(&input, "global-resources");
    let mut nodes = Vec::new();
    let mut next = 1;
    let metrics = crate::ui_runtime::presentation::TextMetrics::for_viewport(
        [1280, 720],
        ui::DpiScale::new(1.0).unwrap(),
        None,
    );
    presentation
        .append_menu(&runtime, &mut nodes, &mut next, metrics, 1280.0, 720.0)
        .unwrap();
    let labels = pack_harness::drawn_texts(&nodes);
    assert!(
        labels.iter().any(|text| text.contains("Copper Test Pack")),
        "pack name must be visible: {labels:?}"
    );
    let actions = &presentation.menu_hit_targets;
    assert!(
        actions
            .iter()
            .any(|(action, _)| *action == MenuAction::GlobalResources(Action::Deactivate(0))),
        "deactivate action must render"
    );
    assert!(
        actions
            .iter()
            .any(|(action, _)| *action == MenuAction::GlobalResources(Action::Import)),
        "import action must remain available"
    );
    assert!(
        actions
            .iter()
            .any(|(action, _)| *action == MenuAction::GlobalResources(Action::Apply)),
        "apply action must remain available"
    );
    let sidebar = actions
        .iter()
        .find_map(|(action, bounds)| {
            matches!(action, MenuAction::SettingsSection(_)).then_some(*bounds)
        })
        .expect("resource settings must retain the settings sidebar");
    let sidebar_point = ui::UiPoint::new(
        (sidebar.min().x() + sidebar.max().x()) * 0.5,
        (sidebar.min().y() + sidebar.max().y()) * 0.5,
    )
    .unwrap();
    assert!(presentation.scroll_menu(sidebar_point, -1.0, false));
    state.settings = Some(0);
    view.global_resources = Arc::new(state.clone());
    presentation.set_menu_view(Some(view.clone()));
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    snapshot::write(&input, "global-resource-settings");
    nodes.clear();
    next = 1;
    let actions = presentation
        .append_menu(&runtime, &mut nodes, &mut next, metrics, 1280.0, 720.0)
        .unwrap();
    let labels = pack_harness::drawn_texts(&nodes);
    assert!(
        labels.iter().any(|text| text.contains("High resolution")),
        "the OreUI picker must show the selected variant: {labels:?}"
    );
    for action in [
        Action::CloseSettings,
        Action::Subpack(0),
        Action::Subpack(1),
    ] {
        assert!(
            actions
                .iter()
                .any(|(found, _)| *found == MenuAction::GlobalResources(action)),
            "the pack variant choices must remain interactive: {action:?}; {actions:?}"
        );
    }
    assert!(
        actions.iter().all(|(action, _)| matches!(
            action,
            MenuAction::GlobalResources(Action::CloseSettings | Action::Subpack(_))
        )),
        "the overlay must exclusively own input: {actions:?}"
    );
    assert!(
        presentation.visible_menu_actions().all(|action| matches!(
            action,
            MenuAction::GlobalResources(Action::CloseSettings | Action::Subpack(_))
        )),
        "background settings controls must not enter keyboard or controller focus"
    );
    let offsets = presentation.menu_scrolls.offsets().clone();
    presentation.scroll_menu(sidebar_point, -1.0, false);
    assert!(
        offsets
            .iter()
            .all(|(key, offset)| presentation.menu_scrolls.offsets().get(key) == Some(offset)),
        "scroll input over the overlay must not move the settings panels beneath it"
    );
    state.settings = None;
    view.global_resources = Arc::new(state);
    presentation.set_menu_view(Some(view));
    nodes.clear();
    next = 1;
    let actions = presentation
        .append_menu(&runtime, &mut nodes, &mut next, metrics, 1280.0, 720.0)
        .unwrap();
    assert!(
        actions
            .iter()
            .any(|(action, _)| *action == MenuAction::GlobalResources(Action::Deactivate(0))),
        "closing pack settings must restore the resource list's actions"
    );
}

/// Builds an original test raster for a known HUD texture path.
fn test_raster() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(24, 24, image::Rgba([235, 110, 35, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

#[test]
fn live_hud_texture_and_definition_swap_reverts_without_session_change() {
    let mut player_runtime = player_state::PlayerState::new(7);

    let Some(mut presentation) = pack_harness::engine_presentation() else {
        eprintln!(
            "skipping live_hud_texture_and_definition_swap_reverts_without_session_change: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(7);
    player_runtime
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let dpi = ui::DpiScale::new(1.0).unwrap();
    let before = presentation
        .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
        .unwrap();
    let before_image = snapshot::rasterize(&before);
    snapshot::write(&before, "hud-before");
    let definition = br##"{"namespace":"hud","root_panel":{"modifications":[{"array_name":"controls","operation":"insert_back","value":[{"live_pack_marker":{"type":"image","texture":"textures/ui/cinnabar_live_marker","size":[96,32],"offset":[0,40],"anchor_from":"top_middle","anchor_to":"top_middle","layer":100}}]}]}}"##.to_vec();
    let pack = super::super::ServerUiPack {
        screen_settings: None,
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/live_reload.json"]}"#.to_vec(),
            ),
            ("ui/live_reload.json".into(), definition),
        ]],
        textures: vec![
            ("textures/ui/cinnabar_live_marker.png".into(), test_raster()),
            ("textures/ui/hotbar_0.png".into(), test_raster()),
        ],
        catalog: None,
        view: None,
    }
    .prepare_catalog(&presentation.pack_catalog_base().unwrap());
    let retired = Arc::downgrade(&pack);
    runtime.set_server_ui(Some(pack));
    let after = presentation
        .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
        .unwrap();
    snapshot::write(&after, "hud-after");
    assert!(
        presentation
            .hud_draw_nodes()
            .iter()
            .any(|node| node.name == "live_pack_marker"),
        "the registered pack definition must reach the HUD"
    );
    assert_ne!(
        before_image,
        snapshot::rasterize(&after),
        "pack definition and raster must affect the HUD"
    );
    runtime.set_server_ui(None);
    let removed = presentation
        .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
        .unwrap();
    snapshot::write(&removed, "hud-removed");
    assert_eq!(
        before_image,
        snapshot::rasterize(&removed),
        "removal resolves the original catalog and art"
    );
    assert_eq!(runtime.session_id(), 7, "no session replacement");
    assert!(
        retired.upgrade().is_none(),
        "old pack data is retired after the frame swap"
    );
}
