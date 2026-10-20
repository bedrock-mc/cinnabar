//! Resource-pack HUD visibility while native placement panels own input.

use super::super::super::ServerUiPack;
use super::mod_widgets::{content, presentation};
use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use launcher::menu::settings_options::SettingId;
use std::sync::Arc;
use ui::DpiScale;

/// Personal settings suppress the base HUD, while the layout editor keeps it for alignment.
#[test]
fn personal_settings_hide_base_hud_and_restore_it_for_layout_and_close() {
    use ui_engine::text_metrics::TextMetrics;
    let mut p = presentation(false);
    let runtime = UiRuntime::new(1);
    let player = player_state::PlayerState::new(1);
    let paint = |p: &mut UiPresentationRuntime| {
        let mut nodes = Vec::new();
        p.append_engine_hud(
            &player,
            &runtime,
            &mut nodes,
            &mut 0,
            TextMetrics::for_viewport([1280, 720], DpiScale::new(1.).unwrap(), None),
            [1280., 720.],
            0,
            false,
        )
        .unwrap();
        nodes.len()
    };
    let base = paint(&mut p);
    assert!(base > 0, "the base HUD fixture must actually render");
    let panel = super::mod_hud_editor::panel();
    p.mod_ui_mut()
        .set_mod_panel(mod_ui::PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    p.mod_ui_mut().set_mod_panel_open(true);
    assert_eq!(
        paint(&mut p),
        0,
        "settings must not have sharp HUD controls underneath"
    );
    p.mod_ui_mut().open_mod_hud_editor(&content()).unwrap();
    assert_eq!(
        paint(&mut p),
        base,
        "the layout editor needs the base HUD as a placement reference"
    );
    p.mod_ui_mut().set_mod_panel_open(false);
    assert_eq!(
        paint(&mut p),
        base,
        "closing settings must immediately restore the base HUD"
    );
}

/// Authored placement panels retain the real pack HUD without bypassing player visibility.
#[test]
fn authored_panels_opt_into_gameplay_hud_but_never_crosshair() {
    use ui_engine::text_metrics::TextMetrics;
    let mut p = UiPresentationRuntime::with_hud(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
    )
    .unwrap();
    p.enable_json_ui(crate::test_support::mini_carrier())
        .unwrap();
    p.hud_frame.first_person = true;
    let files = [
        (
            "ui/hud_test.json",
            json_ui::vanilla::Reference::HudHudScreen,
        ),
        (
            "ui/crosshair_test.json",
            json_ui::vanilla::Reference::HudCrosshairHudCrosshairScreen,
        ),
    ];
    let mut layer = vec![(
        "ui/_ui_defs.json".into(),
        serde_json::to_vec(&serde_json::json!({
            "ui_defs": files.iter().map(|(path, _)| *path).collect::<Vec<_>>()
        }))
        .unwrap(),
    )];
    for (path, reference) in files {
        let (namespace, name) = reference.as_str().split_once('.').unwrap();
        layer.push((path.into(), serde_json::to_vec(&serde_json::json!({
            "namespace": namespace,
            (name): {"type":"screen", "controls":[{"label":{"type":"label","size":[100,12],"text":"HUD fixture"}}]}
        })).unwrap()));
    }
    p.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![layer],
        ..Default::default()
    });
    let runtime = UiRuntime::new(1);
    let player = player_state::PlayerState::new(1);
    let paint = |p: &mut UiPresentationRuntime, crosshair| {
        let mut nodes = Vec::new();
        p.append_engine_hud(
            &player,
            &runtime,
            &mut nodes,
            &mut 0,
            TextMetrics::for_viewport([1280, 720], DpiScale::new(1.).unwrap(), None),
            [1280., 720.],
            0,
            crosshair,
        )
        .unwrap();
        nodes.len()
    };
    let base = paint(&mut p, false);
    assert!(base > 0, "base HUD fixture must render");
    assert!(
        paint(&mut p, true) > 0,
        "the crosshair screen fixture must render before focus"
    );
    let mut panel = super::mod_hud_editor::panel();
    for visible in [false, true, false, true] {
        panel.show_gameplay_hud = visible;
        p.mod_ui_mut()
            .set_mod_panel(mod_ui::PanelUiOwner::Environment(0), Some(&panel))
            .unwrap();
        p.mod_ui_mut().set_mod_panel_open(true);
        assert_eq!(paint(&mut p, false), if visible { base } else { 0 });
        assert_eq!(paint(&mut p, true), 0, "focused panels hide crosshair");
    }
    p.hud_frame.death_screen = true;
    assert_eq!(paint(&mut p, false), 0);
    p.hud_frame.death_screen = false;
    let mut options = launcher::menu::settings_options::SettingsOptions::default();
    let index = launcher::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.id == SettingId::HideHud)
        .unwrap();
    options.set(index, 1);
    p.set_chat_settings_snapshot((Arc::new(options), None));
    assert_eq!(paint(&mut p, false), 0, "player Hide HUD takes priority");
}
