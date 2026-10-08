//! Real-pack HUD placement and settings, including the built-in Java overlay.
use super::engine_hud_tests::engine_presentation;
use crate::{
    menu::settings_options::{SETTINGS_OPTIONS, SettingsOptions},
    ui_runtime::UiRuntime,
};
use json_ui::{Draw, RectOut};
use protocol::PlayerGameMode;
use std::sync::Arc;

#[test]
fn paper_doll_uses_the_pack_control_and_visibility_binding() {
    let Some(mut presentation) = engine_presentation() else {
        return;
    };
    let runtime = UiRuntime::new(1);
    let mut player_runtime = player_state::PlayerState::new(1);
    for (mode, setting, visible) in [
        (PlayerGameMode::Survival, None, true),
        (PlayerGameMode::Creative, None, true),
        (PlayerGameMode::Spectator, None, false),
        (PlayerGameMode::Survival, Some("hide_paperdoll"), false),
        (PlayerGameMode::Survival, Some("hide_hud"), false),
        (PlayerGameMode::Survival, None, true),
    ] {
        player_runtime.facts.publish_player_game_mode(mode);
        let mut options = SettingsOptions::default();
        if let Some(setting) = setting {
            let index = SETTINGS_OPTIONS
                .iter()
                .position(|option| option.name == setting)
                .unwrap();
            options.set(index, 1);
        }
        presentation.set_chat_settings_snapshot((Arc::new(options), None));
        presentation.hud_frame_mut().paper_doll_visible = true;
        for (scale, safe) in [
            (2, ui::SafeArea::ZERO),
            (3, ui::SafeArea::new(20., 10., 40., 30.).unwrap()),
        ] {
            presentation.set_gui_scale_preference(Some(scale));
            presentation.set_safe_area(safe);
            presentation
                .build(
                    &player_runtime,
                    &runtime,
                    4000,
                    [1280, 720],
                    ui::DpiScale::new(1.).unwrap(),
                )
                .unwrap();
            let dolls: Vec<_> = presentation.hud_draw_nodes().iter().filter(|node| matches!(&node.draw, Draw::Custom { renderer, .. } if renderer == "hud_player_renderer")).collect();
            assert_eq!(dolls.len(), usize::from(visible), "{mode:?} {setting:?}");
            if visible {
                assert_eq!(
                    dolls[0].dest,
                    RectOut {
                        x: 15.,
                        y: 15.,
                        w: 15.,
                        h: 15.
                    }
                );
            }
        }
    }
    presentation.hud_frame_mut().paper_doll_visible = false;
    presentation
        .build(
            &player_runtime,
            &runtime,
            5000,
            [1280, 720],
            ui::DpiScale::new(1.).unwrap(),
        )
        .unwrap();
    assert!(!presentation.hud_draw_nodes().iter().any(|node| matches!(&node.draw, Draw::Custom { renderer, .. } if renderer == "hud_player_renderer")));
}
