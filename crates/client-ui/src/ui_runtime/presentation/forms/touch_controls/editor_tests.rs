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

/// Small artwork retains its gameplay size while labeled drag handles stay finger-sized.
#[test]
fn touch_editor_chrome_and_alternate_drag_handles_remain_usable_at_small_scales() {
    for (size, dpi) in [([854, 480], 1.), ([1280, 720], 1.), ([1280, 720], 2.)] {
        let Some((mut p, runtime, _)) = setup(size) else {
            return;
        };
        p.build(
            &player_state::PlayerState::new(1),
            &runtime,
            1000,
            size,
            ui::DpiScale::new(dpi).unwrap(),
        )
        .unwrap();
        let editor = p.screens.touch.editor.as_ref().unwrap();
        let mut targets = Vec::new();
        for index in 0..editor.draft().cards.len() {
            targets.push(editor.action_bounds(&format!("hud.card:{index}")).unwrap());
        }
        for action in ["save", "cancel", "undo", "redo", "reset", "grid", "snap"] {
            targets.push(
                p.touch_editor_action_bounds(&format!("hud.{action}"))
                    .unwrap(),
            );
        }
        for [x, y, w, h] in targets {
            assert!(w >= 48. && h >= 48., "{size:?}, DPI {dpi}: {w} × {h}");
            assert!(
                x >= 0.
                    && y >= 0.
                    && x + w <= size[0] as f32 / dpi + 0.1
                    && y + h <= size[1] as f32 / dpi + 0.1
            );
        }
        let original = p.touch_editor_bounds("chat").unwrap();
        let index = p
            .screens
            .touch
            .editor
            .as_ref()
            .unwrap()
            .draft()
            .cards
            .iter()
            .position(|card| card.id == "chat")
            .unwrap();
        let [x, y, w, h] = p
            .touch_editor_action_bounds(&format!("hud.card:{index}"))
            .unwrap();
        let start = [x + w / 2., y + h / 2.];
        assert!((start[0] - original[0]).hypot(start[1] - original[1]) > 48.);
        p.touch_editor_pointer(start, true, true);
        p.touch_editor_pointer([start[0] + 80., start[1] + 80.], false, true);
        p.touch_editor_pointer([start[0] + 80., start[1] + 80.], false, false);
        let moved = p.touch_editor_bounds("chat").unwrap();
        assert!(moved[0] > original[0] + 50. && moved[1] > original[1] + 50.);
        assert_eq!([original[2], original[3]], [moved[2], moved[3]]);
    }
}

/// Paging retains a reachable drag target for every control, separate from Save and Cancel.
#[test]
fn touch_editor_narrow_high_dpi_handles_never_dispatch_transaction_actions() {
    for (size, safe) in [
        ([854, 480], ui::SafeArea::default()),
        ([854, 480], ui::SafeArea::new(12., 8., 14., 6.).unwrap()),
        ([854, 480], ui::SafeArea::new(24., 32., 24., 32.).unwrap()),
        ([720, 360], ui::SafeArea::default()),
    ] {
        let Some((mut p, runtime, _)) = setup(size) else {
            return;
        };
        p.set_safe_area(safe);
        let paint = |p: &mut UiPresentationRuntime| {
            p.build(
                &player_state::PlayerState::new(1),
                &runtime,
                1000,
                size,
                ui::DpiScale::new(2.).unwrap(),
            )
            .unwrap();
        };
        paint(&mut p);
        let names: Vec<_> = p
            .screens
            .touch
            .editor
            .as_ref()
            .unwrap()
            .draft()
            .cards
            .iter()
            .map(|card| card.id.clone())
            .collect();
        for (index, name) in names.iter().enumerate() {
            for _ in 0..names.len() {
                if p.touch_editor_handle_bounds(name).is_some() {
                    break;
                }
                let [x, y, w, h] = p.touch_editor_action_bounds("hud.page_next").unwrap();
                let at = [x + w / 2., y + h / 2.];
                assert_eq!(
                    p.touch_editor_action_at(at).as_deref(),
                    Some("hud.page_next")
                );
                p.touch_editor_pointer(at, true, false);
                paint(&mut p);
            }
            let [x, y, w, h] = p
                .touch_editor_handle_bounds(name)
                .expect("reachable handle page");
            assert!(w >= 48. && h >= 48.);
            assert!(
                x >= safe.left()
                    && y >= safe.top()
                    && x + w <= size[0] as f32 / 2. - safe.right() + 0.1
                    && y + h <= size[1] as f32 / 2. - safe.bottom() + 0.1
            );
            for action in ["hud.save", "hud.cancel"] {
                if let Some([a, b, c, d]) = p.touch_editor_action_bounds(action) {
                    assert!(
                        x + w <= a || a + c <= x || y + h <= b || b + d <= y,
                        "{name} overlaps {action}"
                    );
                }
            }
            let at = [x + w / 2., y + h / 2.];
            assert_eq!(
                p.touch_editor_action_at(at),
                Some(format!("hud.card:{index}")),
                "{name}"
            );
            p.touch_editor_pointer(at, true, true);
            p.touch_editor_pointer([at[0] + 16., at[1] - 16.], false, true);
            assert!(
                p.take_touch_editor_result().is_none(),
                "{name} must not Save"
            );
            p.cancel_touch_editor_pointer();
            paint(&mut p);
        }
        let [x, y, w, h] = p.touch_editor_action_bounds("hud.page_prev").unwrap();
        p.touch_editor_pointer([x + w / 2., y + h / 2.], true, false);
        paint(&mut p);
        assert!(p.take_touch_editor_result().is_none());
        assert!(
            p.touch_editor_handle_bounds(names.last().unwrap())
                .is_none()
        );
    }
}

/// A viewport that cannot fit two finger-sized rows explains the shortage and only offers Cancel.
#[test]
fn touch_editor_insufficient_safe_height_never_exposes_selection_or_save_targets() {
    let Some((mut p, runtime, _)) = setup([720, 360]) else {
        return;
    };
    p.set_safe_area(ui::SafeArea::new(0., 64., 0., 48.).unwrap());
    p.build(
        &player_state::PlayerState::new(1),
        &runtime,
        1000,
        [720, 360],
        ui::DpiScale::new(2.).unwrap(),
    )
    .unwrap();
    assert!(p.touch_editor_handle_bounds("joystick").is_none());
    assert!(p.touch_editor_action_bounds("hud.save").is_none());
    let [x, y, w, h] = p.touch_editor_action_bounds("hud.cancel").unwrap();
    let at = [x + w / 2., y + h / 2.];
    assert_eq!(p.touch_editor_action_at(at).as_deref(), Some("hud.cancel"));
    p.touch_editor_pointer(at, true, false);
    assert!(p.take_touch_editor_result().unwrap().is_none());
}

/// Compact action navigation exposes every command without changing the draft or its history.
#[test]
fn touch_editor_compact_action_pages_preserve_placements() {
    let Some((mut p, runtime, _)) = setup([854, 480]) else {
        return;
    };
    p.set_safe_area(ui::SafeArea::new(24., 32., 24., 32.).unwrap());
    let paint = |p: &mut UiPresentationRuntime| {
        p.build(
            &player_state::PlayerState::new(1),
            &runtime,
            1000,
            [854, 480],
            ui::DpiScale::new(2.).unwrap(),
        )
        .unwrap();
    };
    paint(&mut p);
    let original = p.screens.touch.editor.as_ref().unwrap().draft().clone();
    for action in ["save", "cancel", "undo", "redo", "reset", "grid", "snap"] {
        while p
            .touch_editor_action_bounds(&format!("hud.{action}"))
            .is_none()
        {
            let [x, y, w, h] = p.touch_editor_action_bounds("hud.toolbar_next").unwrap();
            let at = [x + w / 2., y + h / 2.];
            assert_eq!(
                p.touch_editor_action_at(at).as_deref(),
                Some("hud.toolbar_next")
            );
            p.touch_editor_pointer(at, true, false);
            paint(&mut p);
        }
        let [x, y, w, h] = p
            .touch_editor_action_bounds(&format!("hud.{action}"))
            .unwrap();
        assert!(w >= 48. && h >= 48.);
        assert!(x >= 24. && y >= 32. && x + w <= 403.1 && y + h <= 208.1);
        assert_eq!(p.screens.touch.editor.as_ref().unwrap().draft(), &original);
        assert!(p.take_touch_editor_result().is_none());
    }
    p.touch_editor_key("Enter", Default::default());
    assert_eq!(
        p.take_touch_editor_result().unwrap().unwrap(),
        TouchLayout::default()
    );
}

/// Opaque editing chrome covers artwork that lies beneath it on a cramped viewport.
#[test]
fn touch_editor_toolbar_paints_above_overlapping_artwork() {
    let Some((mut p, runtime, _)) = setup([854, 480]) else {
        return;
    };
    let frame = p
        .build(
            &player_state::PlayerState::new(1),
            &runtime,
            1000,
            [854, 480],
            ui::DpiScale::new(2.).unwrap(),
        )
        .unwrap();
    let [x, y, w, h] = p.touch_editor_bounds("joystick").unwrap();
    let point = [x + w / 2., y + h / 2.];
    let [a, b, c, d] = p.touch_editor_action_bounds("hud.blocked").unwrap();
    assert!(point[0] > a && point[0] < a + c && point[1] > b && point[1] < b + d);
    let pixels = super::super::snapshot::rasterize(&frame);
    let covered = pixels.get_pixel(
        (point[0] * 2.).round() as u32,
        (point[1] * 2.).round() as u32,
    );
    let background = pixels.get_pixel(
        ((a + 1.) * 2.).round() as u32,
        ((b + 1.) * 2.).round() as u32,
    );
    for channel in 0..3 {
        assert!(
            covered[channel].abs_diff(background[channel]) <= 5,
            "toolbar must occlude the joystick: {covered:?} vs {background:?}"
        );
    }
}
