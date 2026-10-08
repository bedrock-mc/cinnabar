use super::*;
use launcher::menu::settings_options::EMOTE_SLOT_COUNT;

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
        let catalog_label = client_world::CustomEmote::ALL[0].label();
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
    assert_eq!(nodes.iter().filter(|node| matches!(node.visual(), ui::UiVisual::Sprite { texture_page, uv, .. } if *texture_page == marker.page && *uv == marker.uv)).count(), 1, "only the occupied native emote image uses the live model marker");
}

#[test]
fn native_wheel_instruction_controls_follow_the_last_active_device() {
    let Some(mut presentation) = super::super::pack_harness::engine_presentation() else {
        return;
    };
    let mut runtime = super::super::pack_harness::menu_runtime();
    runtime.emotes_mut().open();
    let Some(keyboard) = runtime.translation("emotes.instructions_keyboard") else {
        return;
    };
    let keyboard = keyboard.to_string();
    let Some(gamepad) = runtime.translation("emote_wheel.gamepad_helper.select") else {
        return;
    };
    let gamepad = gamepad.to_string();
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
