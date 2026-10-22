//! Touch artwork, editor transactions and gameplay regions share one geometry authority.
use super::*;
use crate::ui_runtime::presentation::forms::pack_harness;
use launcher::menu::{
    MenuScreen, MenuView,
    settings_options::{SettingsOptions, touch_layout::TouchLayout},
};

/// Opens a real touch draft through the presentation route, with optional safe-area insets.
fn setup(size: [u32; 2]) -> Option<(UiPresentationRuntime, UiRuntime, MenuView)> {
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        eprintln!(
            "skipping touch_editor presentation tests: missing installed UI carrier (make assets)"
        );
        return None;
    };
    let runtime = pack_harness::menu_runtime();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Settings;
    view.touch_available = true;
    view.touch_editor_open = true;
    presentation.set_menu_view(Some(view.clone()));
    paint(&mut presentation, &runtime, size);
    Some((presentation, runtime, view))
}
/// Builds the full production menu frame, including touch artwork and editor chrome.
fn paint(p: &mut UiPresentationRuntime, runtime: &UiRuntime, size: [u32; 2]) {
    p.build(
        &player_state::PlayerState::new(1),
        runtime,
        1000,
        size,
        ui::DpiScale::new(1.).unwrap(),
    )
    .unwrap();
}
/// Sends a complete gesture through the shared editor's retained hit regions.
fn drag(p: &mut UiPresentationRuntime, name: &str, delta: [f32; 2], release: bool) {
    let [x, y, w, h] = p.touch_editor_bounds(name).unwrap();
    let at = [x + w / 2., y + h / 2.];
    p.touch_editor_pointer(at, true, true);
    let moved = [at[0] + delta[0], at[1] + delta[1]];
    p.touch_editor_pointer(moved, false, true);
    if release {
        p.touch_editor_pointer(moved, false, false);
    }
}
#[test]
fn touch_editor_save_changes_drawn_and_active_regions_and_survives_reload() {
    let Some((mut p, runtime, _view)) = setup([1280, 720]) else {
        return;
    };
    let original = p.touch_editor_bounds("jump").unwrap();
    drag(&mut p, "jump", [-140., -90.], true);
    paint(&mut p, &runtime, [1280, 720]);
    let moved = p.touch_editor_bounds("jump").unwrap();
    assert!(moved[0] < original[0] - 100.);
    assert!(p.gameplay_touch_region([0.5, 0.5], [1280., 720.]).is_none());
    p.touch_editor_key("Enter", Default::default());
    let layout = p.take_touch_editor_result().unwrap().unwrap();
    let mut settings = SettingsOptions::default();
    settings.set_touch_layout(layout);
    let decoded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    p.set_chat_settings_snapshot((Arc::new(decoded), None));
    p.set_menu_view(None);
    p.enable_touch_controls(true);
    let mut nodes = Vec::new();
    let mut next = 0;
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.).unwrap(), None);
    p.append_touch_controls(&runtime, &mut nodes, &mut next, metrics, [1280., 720.])
        .unwrap();
    let at = [
        (moved[0] + moved[2] / 2.) / 1280.,
        (moved[1] + moved[3] / 2.) / 720.,
    ];
    assert_eq!(
        p.gameplay_touch_region(at, [1280., 720.]).unwrap().hit_id,
        touch::JUMP
    );
    let at = [
        (original[0] + original[2] / 2.) / 1280.,
        (original[1] + original[3] / 2.) / 720.,
    ];
    assert_ne!(
        p.gameplay_touch_region(at, [1280., 720.]).unwrap().hit_id,
        touch::JUMP
    );
}
#[test]
fn touch_editor_cancel_and_cancelled_finger_preserve_the_committed_layout() {
    let Some((mut p, runtime, _)) = setup([1280, 720]) else {
        return;
    };
    let original = p.touch_editor_bounds("joystick").unwrap();
    drag(&mut p, "joystick", [130., -80.], false);
    p.cancel_touch_editor_pointer();
    paint(&mut p, &runtime, [1280, 720]);
    assert_eq!(p.touch_editor_bounds("joystick").unwrap(), original);
    drag(&mut p, "joystick", [130., -80.], true);
    p.touch_editor_key("Escape", Default::default());
    assert!(p.take_touch_editor_result().unwrap().is_none());
    assert_eq!(p.screens.touch.saved, TouchLayout::default());
}
#[test]
fn touch_editor_includes_every_button_and_keeps_layout_inside_resized_safe_area() {
    let Some((mut p, runtime, _)) = setup([1280, 720]) else {
        return;
    };
    assert_eq!(
        p.screens.touch.editor.as_ref().unwrap().draft().cards.len(),
        CONTROLS.len() - 1
    );
    for &(name, id) in CONTROLS {
        if id != touch::LOOK_SURFACE {
            assert!(p.touch_editor_bounds(name).is_some(), "{name}");
        }
    }
    p.set_safe_area(ui::SafeArea::new(12., 8., 14., 6.).unwrap());
    paint(&mut p, &runtime, [854, 480]);
    for &(name, id) in CONTROLS {
        if id == touch::LOOK_SURFACE {
            continue;
        }
        let [x, y, w, h] = p.touch_editor_bounds(name).unwrap();
        assert!(
            x >= 12. && y >= 8. && x + w <= 840.1 && y + h <= 474.1,
            "{name}: {x},{y},{w},{h}"
        );
    }
}

/// Every editable control must move its actual artwork and hit area, including packed utilities.
#[test]
fn touch_editor_saved_positions_move_every_control_hit_area() {
    let Some(mut p) = pack_harness::engine_presentation() else {
        return;
    };
    let runtime = pack_harness::menu_runtime();
    p.enable_touch_controls(true);
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.).unwrap(), None);
    for &(name, id) in CONTROLS {
        if id == touch::LOOK_SURFACE {
            continue;
        }
        let mut layout = TouchLayout::default();
        layout.set(name, [0.4, 0.35]);
        p.screens.touch.saved = layout;
        let mut nodes = Vec::new();
        let mut next = 0;
        p.append_touch_controls(&runtime, &mut nodes, &mut next, metrics, [1280., 720.])
            .unwrap();
        let base = p.screens.touch.base.as_ref().unwrap();
        let hit = base
            .hits
            .iter()
            .find(|hit| control_id(hit.pressed.as_deref()) == Some(id))
            .unwrap();
        let at = [
            0.4 * (1280. - hit.rect.w as f32 * base.scale) + hit.rect.w as f32 * base.scale / 2.,
            0.35 * (720. - hit.rect.h as f32 * base.scale) + hit.rect.h as f32 * base.scale / 2.,
        ];
        assert_eq!(
            p.gameplay_touch_region([at[0] / 1280., at[1] / 720.], [1280., 720.])
                .map(|hit| hit.hit_id),
            Some(id),
            "{name}"
        );
    }
}

/// Snap, undo, redo and Reset use the same completed gesture history as mod layout editing.
#[test]
fn touch_editor_alignment_history_and_reset_keep_factory_layout_available() {
    let Some((mut p, runtime, _)) = setup([1280, 720]) else {
        return;
    };
    let original = p.touch_editor_bounds("jump").unwrap();
    let at = [
        original[0] + original[2] / 2.,
        original[1] + original[3] / 2.,
    ];
    p.touch_editor_pointer(at, true, true);
    p.touch_editor_pointer([640. + original[2] / 2. + 2., 300.], false, true);
    paint(&mut p, &runtime, [1280, 720]);
    assert!((p.touch_editor_bounds("jump").unwrap()[0] - 640.).abs() < 0.1);
    p.touch_editor_pointer([640. + original[2] / 2. + 2., 300.], false, false);
    p.touch_editor_key(
        "KeyZ",
        ui::mod_panel::FieldModifiers {
            control: true,
            ..Default::default()
        },
    );
    assert_eq!(p.touch_editor_bounds("jump").unwrap(), original);
    p.touch_editor_key(
        "KeyY",
        ui::mod_panel::FieldModifiers {
            control: true,
            ..Default::default()
        },
    );
    assert!(p.touch_editor_bounds("jump").unwrap()[0] < original[0] - 100.);
    paint(&mut p, &runtime, [1280, 720]);
    let [x, y, w, h] = p.touch_editor_action_bounds("hud.reset").unwrap();
    p.touch_editor_pointer([x + w / 2., y + h / 2.], true, true);
    p.touch_editor_pointer([x + w / 2., y + h / 2.], false, false);
    paint(&mut p, &runtime, [1280, 720]);
    assert_eq!(p.touch_editor_bounds("jump").unwrap(), original);
    p.touch_editor_key("Enter", Default::default());
    assert_eq!(
        p.take_touch_editor_result().unwrap().unwrap(),
        TouchLayout::default()
    );
}

/// Changing automatic GUI scale must not turn factory placements into user customization.
#[test]
fn touch_editor_factory_layout_matches_fresh_geometry_after_scale_changing_resize() {
    let Some((mut p, runtime, _)) = setup([1366, 768]) else {
        return;
    };
    paint(&mut p, &runtime, [1280, 720]);
    let [x, y, w, h] = p.touch_editor_action_bounds("hud.reset").unwrap();
    p.touch_editor_pointer([x + w / 2., y + h / 2.], true, true);
    p.touch_editor_pointer([x + w / 2., y + h / 2.], false, false);
    paint(&mut p, &runtime, [1280, 720]);
    let base = p.screens.touch.base.as_ref().unwrap();
    for &(name, id) in CONTROLS {
        if id == touch::LOOK_SURFACE {
            continue;
        }
        let hit = base
            .hits
            .iter()
            .find(|hit| control_id(hit.pressed.as_deref()) == Some(id))
            .unwrap();
        let expected = [
            base.origin[0] + hit.rect.x as f32 * base.scale,
            base.origin[1] + hit.rect.y as f32 * base.scale,
            hit.rect.w as f32 * base.scale,
            hit.rect.h as f32 * base.scale,
        ];
        let actual = p.touch_editor_bounds(name).unwrap();
        assert!(
            (0..4).all(|axis| (actual[axis] - expected[axis]).abs() < 0.1),
            "{name}: {actual:?} != {expected:?}"
        );
    }
    p.touch_editor_key("Enter", Default::default());
    assert_eq!(
        p.take_touch_editor_result().unwrap().unwrap(),
        TouchLayout::default()
    );
}
