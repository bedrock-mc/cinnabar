//! Optional rendered frames for the detection controls and completion notice.

use launcher::menu::settings_options::graphics_detection::TUNED_MESSAGE;
use launcher::menu::{MenuAction, MenuScreen, MenuView};

#[test]
fn graphics_detection_controls_and_toast_render_at_desktop_and_small_sizes() {
    let Some(mut presentation) = super::pack_harness::engine_presentation() else {
        eprintln!("missing fixture: UI carrier for graphics detection snapshots (make assets)");
        return;
    };
    let player = player_state::PlayerState::new(1);
    for (size, scale) in [([1280, 720], 1.0), ([800, 600], 1.25)] {
        let mut runtime = super::pack_harness::menu_runtime();
        let mut view = MenuView::new(true, "Player".into());
        view.screen = MenuScreen::Settings;
        view.settings_section = launcher::menu::settings_sections::SETTINGS_SECTIONS
            .iter()
            .find_map(|(name, index)| (*name == "video_forced_index").then_some(*index))
            .unwrap();
        {
            presentation.set_menu_view(Some(view.clone()));
            presentation
                .build(
                    &player,
                    &runtime,
                    1_000,
                    size,
                    ui::DpiScale::new(scale).unwrap(),
                )
                .unwrap();
            let input = presentation
                .build(
                    &player,
                    &runtime,
                    2_000,
                    size,
                    ui::DpiScale::new(scale).unwrap(),
                )
                .unwrap();
            assert!(
                presentation
                    .visible_menu_actions()
                    .any(|action| action == MenuAction::SettingsDetectGraphics)
            );
            super::snapshot::write(&input, &format!("graphics-detect-{}", size[0]));
        }
        runtime.push_client_toast(TUNED_MESSAGE, 2_000);
        presentation.set_menu_view(Some(view));
        let input = presentation
            .build(
                &player,
                &runtime,
                2_500,
                size,
                ui::DpiScale::new(scale).unwrap(),
            )
            .unwrap();
        assert!(runtime.hud().showing_toast(2_500).is_some());
        assert!(presentation.toast_draw_nodes().iter().any(|node| {
            matches!(&node.draw, json_ui::Draw::Text { text, .. } if text == TUNED_MESSAGE)
        }));
        super::snapshot::write(&input, &format!("graphics-completed-{}", size[0]));
    }
}
