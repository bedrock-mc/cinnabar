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

#[test]
fn pack_image_crosshair_obeys_camera_mode_and_hud_visibility() {
    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping pack_image_crosshair_obeys_camera_mode_and_hud_visibility: missing local carriers (make assets)"
        );
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let (namespace, screen) = json_ui::CROSSHAIR_SCREEN.split_once('.').unwrap();
    let overlay = serde_json::json!({
        "namespace": namespace,
        (screen): {"controls":[{"pack_cursor":{
            "type":"image", "texture":"textures/ui/pack_cursor",
            "size":assets::HudTextureRole::Crosshair.expected_size(), "anchor_from":"center", "anchor_to":"center"
        }}]}
    });
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(8, 8, image::Rgba([233, 241, 249, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    runtime.set_server_ui(Some(Arc::new(super::super::super::forms::ServerUiPack {
        ui_layers: vec![vec![(
            "ui/hud_crosshair_overlay.json".into(),
            overlay.to_string().into_bytes(),
        )]],
        textures: vec![("textures/ui/pack_cursor.png".into(), png)],
        ..Default::default()
    })));
    let mut options = SettingsOptions::default();
    for first_person in [true, false, true] {
        for third_person in [false, true] {
            for mode in [
                PlayerGameMode::Survival,
                PlayerGameMode::Creative,
                PlayerGameMode::Spectator,
            ] {
                for hidden in [false, true] {
                    player.facts.publish_player_game_mode(mode);
                    presentation.hud_frame_mut().first_person = first_person;
                    set(
                        &mut options,
                        THIRD_PERSON_CROSSHAIR_OPTION.name,
                        third_person,
                    );
                    set(&mut options, "hide_hud", hidden);
                    presentation.set_chat_settings_snapshot((Arc::new(options.clone()), None));
                    let input = build(&player, &mut presentation, &runtime, 0);
                    let shown =
                        presentation
                            .last_frame
                            .as_ref()
                            .unwrap()
                            .nodes
                            .iter()
                            .any(|node| {
                                matches!(node.visual(), ui::UiVisual::Sprite { uv, .. }
                            if uv[2] - uv[0] == 8 && uv[3] - uv[1] == 8)
                            });
                    assert_eq!(
                        shown,
                        (first_person || third_person)
                            && mode != PlayerGameMode::Spectator
                            && !hidden
                    );
                    if shown {
                        super::super::super::forms::snapshot::write(&input, "pack-image-crosshair");
                    }
                }
            }
        }
    }
}

#[test]
fn legacy_hud_cursor_does_not_invert_the_crosshair_twice() {
    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping legacy_hud_cursor_does_not_invert_the_crosshair_twice: missing local carriers (make assets)"
        );
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    presentation.hud_frame_mut().first_person = true;
    let mut runtime = UiRuntime::new(1);
    let overlay = serde_json::json!({
        "namespace":"hud", "root_panel": {"modifications":[{
            "array_name":"controls", "operation":"insert_back", "value":[{
                "legacy_cursor":{"type":"custom", "renderer":"cursor_renderer", "size":[16,16]}
            }]
        }]}
    });
    runtime.set_server_ui(Some(Arc::new(super::super::super::forms::ServerUiPack {
        ui_layers: vec![vec![(
            "ui/hud_screen.json".into(),
            overlay.to_string().into_bytes(),
        )]],
        ..Default::default()
    })));
    for _ in 0..2 {
        build(&player, &mut presentation, &runtime, 0);
        assert_eq!(
            presentation
                .last_frame
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter(|node| { matches!(node.visual(), ui::UiVisual::InvertedSprite { .. }) })
                .count(),
            1,
            "the legacy HUD and modern overlay must share one visible cursor"
        );
    }
}
