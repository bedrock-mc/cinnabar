//! Gameplay utility geometry and HUD-owned inventory access.

use super::*;
use crate::ui_runtime::presentation::forms::pack_harness;

/// Paints both HUD and touch controls using the installed UI carrier.
fn paint(
    presentation: &mut UiPresentationRuntime,
    player: &player_state::PlayerState,
    runtime: &UiRuntime,
    size: [u32; 2],
) -> Vec<UiNode> {
    let mut nodes = Vec::new();
    let mut next = 0;
    let metrics = TextMetrics::for_viewport(size, ui::DpiScale::new(1.0).unwrap(), None);
    let content = size.map(|value| value as f32);
    presentation.screens.touch.begin_frame();
    presentation
        .append_engine_hud(
            player, runtime, &mut nodes, &mut next, metrics, content, 0, false,
        )
        .unwrap();
    presentation
        .append_touch_controls(runtime, &mut nodes, &mut next, metrics, content)
        .unwrap();
    nodes
}

/// Utility buttons keep one centered row and use their painted bounds for input at both scales.
#[test]
fn utility_row_keeps_chat_between_utilities_and_pause() {
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        return;
    };
    presentation.enable_touch_controls(true);
    let player = player_state::PlayerState::new(1);
    let runtime = pack_harness::menu_runtime();
    for size in [[854, 460], [1280, 700]] {
        let nodes = paint(&mut presentation, &player, &runtime, size);
        assert!(!nodes.is_empty());
        let window = size.map(|value| value as f32);
        let px = TextMetrics::for_viewport(size, ui::DpiScale::new(1.0).unwrap(), None).gui_scale;
        for (id, offset) in [
            (touch::UTILITIES, -20.0),
            (touch::CHAT, 0.0),
            (touch::MENU, 20.0),
        ] {
            let center = [window[0] / 2.0 + offset * px, 12.0 * px];
            let region = presentation
                .gameplay_touch_region([center[0] / window[0], center[1] / window[1]], window)
                .expect("painted utility is reachable");
            assert_eq!(region.hit_id, id);
            assert!(
                ((region.bounds.max[0] - region.bounds.min[0]) * window[0] - 18.0 * px).abs()
                    < 0.01
            );
            assert!(
                ((region.bounds.max[1] - region.bounds.min[1]) * window[1] - 18.0 * px).abs()
                    < 0.01
            );
        }
        assert_ne!(
            presentation
                .gameplay_touch_region([0.98, 0.02], window)
                .map(|hit| hit.hit_id),
            Some(touch::INVENTORY)
        );
    }
}

/// Changing touch enablement refreshes retained HUD bindings and removes obsolete geometry.
#[test]
fn inventory_access_tracks_the_painted_hotbar_and_touch_enablement() {
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    player.inventory.set_local_selected_slot(0);
    let runtime = pack_harness::menu_runtime();
    let window = [1280.0, 720.0];
    let mut old_center = None;
    for enabled in [false, true, false, true] {
        presentation.enable_touch_controls(enabled);
        paint(&mut presentation, &player, &runtime, [1280, 720]);
        let frame = presentation.screens.touch.hud.as_ref().unwrap();
        let inventory = frame
            .hits
            .iter()
            .find(|hit| hit.pressed.as_deref() == Some("button.hotbar_inventory_button"));
        assert_eq!(inventory.is_some(), enabled);
        if let Some(hit) = inventory {
            let center = [
                (frame.origin[0] + (hit.rect.x + hit.rect.w / 2.0) as f32 * frame.scale)
                    / window[0],
                (frame.origin[1] + (hit.rect.y + hit.rect.h / 2.0) as f32 * frame.scale)
                    / window[1],
            ];
            assert!(center[1] > 0.8, "inventory stays with the bottom HUD");
            assert_eq!(
                presentation
                    .gameplay_touch_region(center, window)
                    .unwrap()
                    .hit_id,
                touch::INVENTORY
            );
            old_center = Some(center);
        } else if let Some(center) = old_center {
            assert!(presentation.gameplay_touch_region(center, window).is_none());
        }
    }
}

/// Default movement/action centers and dimensions scale with the viewport, independently of GUI size.
#[test]
fn crosshair_factory_layout_uses_proportional_centers_and_height_based_sizes() {
    let Some(mut p) = pack_harness::engine_presentation() else {
        return;
    };
    let runtime = pack_harness::menu_runtime();
    let player = player_state::PlayerState::new(1);
    p.enable_touch_controls(true);
    for size in [[854, 460], [1280, 700], [1920, 1080]] {
        paint(&mut p, &player, &runtime, size);
        let frame = p.screens.touch.frame.as_ref().unwrap();
        let window = size.map(f64::from);
        for (id, center, height) in [
            (touch::JOYSTICK, [0.15, 0.55], 1. / 3.),
            (touch::JUMP, [0.925, 0.4], 0.1375),
            (touch::SPRINT, [0.825, 0.5], 0.1375),
            (touch::SNEAK, [0.925, 0.6], 0.1375),
            (touch::ATTACK, [0.825, 0.7], 0.1375),
            (touch::USE, [0.925, 0.8], 0.1375),
        ] {
            let hit = frame
                .hits
                .iter()
                .find(|hit| control_id(hit.pressed.as_deref()) == Some(id))
                .unwrap();
            let actual = [
                f64::from(frame.origin[0])
                    + (hit.rect.x + hit.rect.w / 2.) * f64::from(frame.scale),
                f64::from(frame.origin[1])
                    + (hit.rect.y + hit.rect.h / 2.) * f64::from(frame.scale),
            ];
            assert!(
                (0..2).all(|axis| (actual[axis] - center[axis] * window[axis]).abs() < 0.1),
                "{id}: {actual:?}"
            );
            assert!(
                (hit.rect.w - hit.rect.h).abs() < 0.001,
                "square control {id}"
            );
            assert!(
                (hit.rect.h * f64::from(frame.scale) - height * window[1]).abs() < 0.1,
                "control size {id}"
            );
            assert_eq!(
                p.gameplay_touch_region(
                    center.map(|value| value as f32),
                    size.map(|value| value as f32)
                )
                .unwrap()
                .hit_id,
                id
            );
        }
    }
}

/// Paints the overlay with deterministic in-memory fixtures, so menu interaction needs no install.
fn paint_touch(p: &mut UiPresentationRuntime, runtime: &UiRuntime, size: [u32; 2]) {
    let metrics = TextMetrics::for_viewport(size, ui::DpiScale::new(1.).unwrap(), None);
    p.append_touch_controls(
        runtime,
        &mut Vec::new(),
        &mut 0,
        metrics,
        [
            size[0] as f32 - p.safe_area.left() - p.safe_area.right(),
            size[1] as f32 - p.safe_area.top() - p.safe_area.bottom(),
        ],
    )
    .unwrap();
}

/// Resolves a painted control's center through the same hit test used by gameplay.
fn hit_center(p: &UiPresentationRuntime, id: u16, size: [u32; 2]) -> TouchControlRegion {
    let frame = p.screens.touch.frame.as_ref().unwrap();
    let hit = frame
        .hits
        .iter()
        .find(|hit| control_id(hit.pressed.as_deref()) == Some(id))
        .unwrap();
    let window = size.map(|v| v as f32);
    let center = [
        (frame.origin[0] + (hit.rect.x + hit.rect.w / 2.) as f32 * frame.scale) / window[0],
        (frame.origin[1] + (hit.rect.y + hit.rect.h / 2.) as f32 * frame.scale) / window[1],
    ];
    p.gameplay_touch_region(center, window).unwrap()
}

#[test]
fn dropdown_choices_are_reachable_and_outside_taps_cannot_drive_gameplay() {
    let mut p = crate::test_support::mini_engine_presentation();
    let runtime = UiRuntime::new(1);
    p.enable_touch_controls(true);
    for size in [[854, 480], [720, 1600]] {
        paint_touch(&mut p, &runtime, size);
        assert_eq!(
            hit_center(&p, touch::UTILITIES, size).hit_id,
            touch::UTILITIES
        );
        assert!(
            !p.screens
                .touch
                .frame
                .as_ref()
                .unwrap()
                .hits
                .iter()
                .any(|hit| control_id(hit.pressed.as_deref()) == Some(touch::EMOTE))
        );
        p.toggle_touch_utilities();
        paint_touch(&mut p, &runtime, size);
        for id in [
            touch::DEBUG_OVERLAY,
            touch::PLAYER_LIST,
            touch::EMOTE,
            touch::UTILITIES,
        ] {
            assert_eq!(hit_center(&p, id, size).hit_id, id);
        }
        assert_eq!(
            p.gameplay_touch_region([0.15, 0.55], size.map(|v| v as f32))
                .unwrap()
                .hit_id,
            touch::DISMISS_UTILITIES
        );
        p.close_touch_utilities();
        paint_touch(&mut p, &runtime, size);
        assert_eq!(
            p.gameplay_touch_region([0.15, 0.55], size.map(|v| v as f32))
                .unwrap()
                .hit_id,
            touch::JOYSTICK
        );
    }
}

#[test]
fn dropdown_follows_saved_trigger_and_fits_safe_area_at_screen_edges() {
    let mut p = crate::test_support::mini_engine_presentation();
    let runtime = UiRuntime::new(1);
    let size = [854, 480];
    p.set_safe_area(ui::SafeArea::new(25., 12., 30., 18.).unwrap());
    p.enable_touch_controls(true);
    p.toggle_touch_utilities();
    for at in [[0., 0.], [1., 0.], [0., 1.], [1., 1.]] {
        p.screens.touch.saved.set("utilities", at);
        paint_touch(&mut p, &runtime, size);
        for id in [touch::DEBUG_OVERLAY, touch::PLAYER_LIST, touch::EMOTE] {
            let hit = hit_center(&p, id, size);
            assert_eq!(hit.hit_id, id);
            assert!(hit.bounds.min[0] * size[0] as f32 >= 24.99);
            assert!(hit.bounds.max[0] * size[0] as f32 <= size[0] as f32 - 29.99);
            assert!(hit.bounds.min[1] * size[1] as f32 >= 11.99);
            assert!(hit.bounds.max[1] * size[1] as f32 <= size[1] as f32 - 17.99);
        }
    }
}

#[test]
fn touch_debug_requests_are_consumed_once_and_cancel_in_pairs() {
    let mut p = crate::test_support::mini_engine_presentation();
    assert!(!p.take_touch_debug_toggle());
    p.request_touch_debug_toggle();
    assert!(p.take_touch_debug_toggle());
    assert!(!p.take_touch_debug_toggle());
    p.request_touch_debug_toggle();
    p.request_touch_debug_toggle();
    assert!(!p.take_touch_debug_toggle());
}
