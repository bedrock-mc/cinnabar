//! The native emote wheel renders its slices, labels and device instructions.

use json_ui::InputMode;
use launcher::menu::settings_options::EMOTE_SLOT_COUNT;
use ui::{UiNode, UiPoint};
use ui_engine::text_metrics::TextMetrics;
use ui_screens::emote_screen::EmoteHit;

use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::presentation::UiPresentationRuntime;

fn draw(presentation: &mut UiPresentationRuntime, runtime: &UiRuntime) -> Vec<UiNode> {
    let mut nodes = Vec::new();
    let mut next = 0;
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), None);
    presentation
        .append_emote_screen(runtime, &mut nodes, &mut next, metrics, [1280.0, 720.0], 0)
        .unwrap();
    nodes
}

#[test]
fn native_wheel_and_equip_popup_render_four_hit_slices_and_unique_catalog_label() {
    let _catalog = cosmetics::test_support::catalog_lock();
    let clip = cosmetics::test_support::emote("fixture:picker", "Bend", true);
    cosmetics::emotes::activate(&[clip]);
    let Some(mut presentation) = super::super::pack_harness::engine_presentation() else {
        return;
    };
    let mut runtime = super::super::pack_harness::menu_runtime();
    runtime.emotes_mut().open();
    for equip in [false, true] {
        if equip {
            runtime.emotes_mut().change_emotes();
        }
        let nodes = draw(&mut presentation, &runtime);
        assert!(!nodes.is_empty());
        let frame = presentation
            .emote_frame()
            .expect("native screen produced an engine frame");
        let hit = frame
            .hits
            .iter()
            .find(|hit| hit.widget.selection_wheel.is_some())
            .expect("authored native selection wheel");
        assert_eq!(
            hit.widget.selection_wheel.as_ref().unwrap().slice_count,
            EMOTE_SLOT_COUNT
        );
        let center = [hit.rect.x + hit.rect.w / 2.0, hit.rect.y + hit.rect.h / 2.0];
        let radius = hit.rect.w.min(hit.rect.h) * 0.35;
        let origin = frame.origin;
        let scale = frame.scale;
        for (slot, delta) in [[0.0, -1.0], [1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]]
            .into_iter()
            .enumerate()
        {
            let point = UiPoint::new(
                origin[0] + (center[0] + delta[0] * radius) as f32 * scale,
                origin[1] + (center[1] + delta[1] * radius) as f32 * scale,
            )
            .unwrap();
            assert_eq!(
                presentation.hit_test_emote(point),
                Some(EmoteHit::Slot(slot)),
                "native cardinal slice {slot}, equip={equip}"
            );
        }
        let center = UiPoint::new(
            origin[0] + center[0] as f32 * scale,
            origin[1] + center[1] as f32 * scale,
        )
        .unwrap();
        assert_eq!(
            presentation.hit_test_emote(center),
            None,
            "native inner dead zone"
        );
        let catalog_label = clip.label();
        assert!(
            super::super::pack_harness::drawn_texts(&nodes)
                .iter()
                .any(|text| text == catalog_label)
        );
        if !equip {
            assert!(
                presentation
                    .emote_frame()
                    .unwrap()
                    .hits
                    .iter()
                    .any(|hit| hit.pressed.as_deref() == Some("button.dressing_room")),
                "native Change Emotes control"
            );
        }
        let player = player_state::PlayerState::new(1);
        let input = presentation
            .build(
                &player,
                &runtime,
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        super::super::snapshot::write(&input, if equip { "emote_equip" } else { "emote_wheel" });
    }
}

#[test]
fn occupied_native_image_routes_the_live_player_marker_without_fabricating_empty_slot_icons() {
    let Some(mut presentation) = super::super::pack_harness::engine_presentation() else {
        return;
    };
    presentation.set_player_preview_skin(None, Default::default());
    let marker = presentation
        .player_preview_icon()
        .expect("player preview texture marker");
    let mut runtime = UiRuntime::new(1);
    runtime.emotes_mut().open();
    let nodes = draw(&mut presentation, &runtime);
    assert_eq!(
        presentation.player_preview_view,
        super::super::super::player_preview::PreviewView::Hud
    );
    assert_eq!(nodes.iter().filter(|node| matches!(node.visual(), ui::UiVisual::Sprite { texture_page, uv, .. } if *texture_page == marker.page && *uv == marker.uv)).count(), runtime.emotes().slots().iter().flatten().count(), "only occupied native emote images use the live model marker");
}

#[test]
fn catalog_wheel_names_owned_clips_from_saved_preferences() {
    let _catalog = cosmetics::test_support::playback_catalog();
    let Some(mut presentation) = super::super::pack_harness::engine_presentation() else {
        return;
    };
    let mut runtime = super::super::pack_harness::menu_runtime();
    runtime.emotes_mut().apply_preferences(Some(&[
        Some(cosmetics::test_support::playback_emote().id().to_owned()),
        None,
        None,
        None,
    ]));
    runtime.emotes_mut().open();
    runtime.emotes_mut().change_emotes();
    let nodes = draw(&mut presentation, &runtime);
    assert!(
        super::super::pack_harness::drawn_texts(&nodes)
            .iter()
            .any(|text| text == cosmetics::test_support::playback_emote().label())
    );
}

#[test]
fn native_wheel_instruction_controls_follow_the_last_active_device() {
    let Some(mut presentation) = super::super::pack_harness::engine_presentation() else {
        return;
    };
    let mut runtime = super::super::pack_harness::menu_runtime();
    runtime.emotes_mut().open();
    let settings = launcher::menu::settings_options::SettingsOptions::default();
    let keyboard = ui_screens::emote_screen::instructions(InputMode::Mouse, None, false, &settings);
    let gamepad =
        ui_screens::emote_screen::instructions(InputMode::Gamepad, None, false, &settings);
    assert_ne!(keyboard, gamepad);
    presentation.set_emote_input_mode(InputMode::Mouse);
    let mouse_nodes = draw(&mut presentation, &runtime);
    assert!(super::super::pack_harness::drawn_texts(&mouse_nodes).contains(&keyboard));
    presentation.set_emote_input_mode(InputMode::Gamepad);
    // A stationary cursor belongs to the prior mouse frame and must not change mode.
    presentation.set_emote_pointer(Some(UiPoint::new(640.0, 360.0).unwrap()));
    let pad_nodes = draw(&mut presentation, &runtime);
    let texts = super::super::pack_harness::drawn_texts(&pad_nodes);
    assert!(!texts.contains(&keyboard));
    assert!(texts.contains(&gamepad));
}

#[test]
fn native_wheel_controller_instructions_follow_saved_button_swaps() {
    use launcher::menu::settings_options::{SettingId, SettingsOptions};
    use std::sync::Arc;

    let Some(mut presentation) = super::super::pack_harness::engine_presentation() else {
        return;
    };
    let mut runtime = super::super::pack_harness::menu_runtime();
    runtime.emotes_mut().open();
    presentation.set_emote_input_mode(InputMode::Gamepad);
    for (swap_ab, swap_xy, expected) in [
        (0, 0, "D-pad/A: play. Y: catalog"),
        (1, 0, "D-pad/B: play. Y: catalog"),
        (0, 1, "D-pad/A: play. X: catalog"),
        (1, 1, "D-pad/B: play. X: catalog"),
    ] {
        let mut settings = SettingsOptions::default();
        settings.set(SettingId::SwapGamepadAbButtons.index(), swap_ab);
        settings.set(SettingId::SwapGamepadXyButtons.index(), swap_xy);
        presentation.set_chat_settings_snapshot((Arc::new(settings), None));
        let nodes = draw(&mut presentation, &runtime);
        let input = presentation
            .build(
                &player_state::PlayerState::new(1),
                &runtime,
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        super::super::snapshot::write(&input, &format!("emote_swaps_{swap_ab}_{swap_xy}"));
        let texts = super::super::pack_harness::drawn_texts(&nodes);
        assert!(
            texts.iter().any(|text| text == expected),
            "swap_ab={swap_ab}, swap_xy={swap_xy}: {texts:?}"
        );
    }
}

#[test]
fn catalog_button_is_clickable_without_the_marketplace() {
    let Some(mut presentation) = super::super::pack_harness::engine_presentation() else {
        return;
    };
    let mut runtime = super::super::pack_harness::menu_runtime();
    runtime.emotes_mut().open();
    draw(&mut presentation, &runtime);
    let frame = presentation.emote_frame().unwrap();
    let button = frame
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("button.dressing_room"))
        .unwrap();
    assert!(
        button.enabled,
        "the local clip catalog is available without Marketplace"
    );
    let point = UiPoint::new(
        frame.origin[0] + (button.rect.x + button.rect.w / 2.0) as f32 * frame.scale,
        frame.origin[1] + (button.rect.y + button.rect.h / 2.0) as f32 * frame.scale,
    )
    .unwrap();
    assert_eq!(
        presentation.hit_test_emote(point),
        Some(EmoteHit::ChangeEmotes)
    );
}
