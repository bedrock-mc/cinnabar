use super::*;
use crate::menu::settings_options::{
    INVERT_CROSSHAIR_OPTION, SETTINGS_OPTIONS, SettingsOptions, THIRD_PERSON_CROSSHAIR_OPTION,
};

/// Updates a registered setting by name for a live presentation change.
fn set(options: &mut SettingsOptions, name: &str, value: bool) {
    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == name)
        .unwrap();
    options.set(index, i32::from(value));
}

#[test]
fn crosshair_preferences_update_live_and_preserve_visibility_gates() {
    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping crosshair_preferences_update_live_and_preserve_visibility_gates: missing local carriers (make assets)"
        );
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let runtime = UiRuntime::new(1);
    let mut options = SettingsOptions::default();
    for first_person in [true, false] {
        for third_person_crosshair in [false, true] {
            for invert in [true, false, true] {
                for mode in [
                    PlayerGameMode::Survival,
                    PlayerGameMode::Creative,
                    PlayerGameMode::Spectator,
                ] {
                    for hide_hud in [false, true] {
                        player.facts.publish_player_game_mode(mode);
                        presentation.hud_frame_mut().first_person = first_person;
                        set(
                            &mut options,
                            THIRD_PERSON_CROSSHAIR_OPTION.name,
                            third_person_crosshair,
                        );
                        set(&mut options, INVERT_CROSSHAIR_OPTION.name, invert);
                        set(&mut options, "hide_hud", hide_hud);
                        presentation.set_chat_settings_snapshot((Arc::new(options.clone()), None));
                        build(&player, &mut presentation, &runtime, 0);
                        let crosshair = presentation
                            .last_frame
                            .as_ref()
                            .unwrap()
                            .nodes
                            .iter()
                            .find(|node| match node.visual() {
                                ui::UiVisual::InvertedSprite { uv, .. }
                                | ui::UiVisual::Sprite { uv, .. } => {
                                    let [width, height] =
                                        assets::HudTextureRole::Crosshair.expected_size();
                                    uv[2] - uv[0] == width as u16 && uv[3] - uv[1] == height as u16
                                }
                                _ => false,
                            });
                        let shown = (first_person || third_person_crosshair)
                            && mode != PlayerGameMode::Spectator
                            && !hide_hud;
                        assert_eq!(
                            crosshair.is_some(),
                            shown,
                            "first={first_person}, third={third_person_crosshair}, invert={invert}, mode={mode:?}, hide={hide_hud}"
                        );
                        if let Some(node) = crosshair {
                            assert_eq!(
                                matches!(node.visual(), ui::UiVisual::InvertedSprite { .. }),
                                invert
                            );
                        }
                    }
                }
            }
        }
    }
}
