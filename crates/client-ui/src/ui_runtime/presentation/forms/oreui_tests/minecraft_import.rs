//! The dedicated import modal remains usable without the JSON-UI carrier.

use super::super::super::UiPresentationRuntime;
use crate::ui_runtime::UiRuntime;
use launcher::{
    menu::{MenuAction, MenuDialog, MenuView},
    minecraft_import::{Action, Phase},
};
use std::sync::Arc;
use ui::{UiNode, UiPoint, UiRect, UiVisual};
use ui_engine::{test_support::fixture_font, text_metrics::TextMetrics};

/// Draws through the public menu adapter with no engine so fallback routing is exercised.
fn paint(presentation: &mut UiPresentationRuntime) -> (Vec<(MenuAction, UiRect)>, Vec<UiNode>) {
    paint_at(presentation, [1280.0, 720.0], 1.0)
}

/// Publishes the fallback adapter's actual pointer targets at the requested viewport and DPI.
fn paint_at(
    presentation: &mut UiPresentationRuntime,
    size: [f32; 2],
    dpi: f32,
) -> (Vec<(MenuAction, UiRect)>, Vec<UiNode>) {
    let metrics = TextMetrics::for_viewport(
        size.map(|value| (value * dpi) as u32),
        ui::DpiScale::new(dpi).unwrap(),
        Some(2),
    );
    let mut nodes = Vec::new();
    let hits = presentation
        .append_menu(
            &UiRuntime::new(1),
            &mut nodes,
            &mut 1,
            metrics,
            size[0],
            size[1],
        )
        .unwrap();
    presentation.menu_hit_targets = hits.clone();
    (hits, nodes)
}

#[test]
fn fallback_import_honors_saved_scale_and_keeps_small_window_actions_clickable() {
    use launcher::menu::settings_options::{EXTRA_LARGE_UI, SettingsOptions};
    use oreui::test_support::{centre, option_index};

    for size in [ui::MIN_WINDOW_LOGICAL_SIZE, [400.0, 250.0], [1280.0, 720.0]] {
        for dpi in [1.0, 2.0] {
            for phase in [
                Phase::Offered,
                Phase::Profiles,
                Phase::Preview,
                Phase::Revert,
            ] {
                let mut presentation = fixture(phase);
                let view = Arc::make_mut(presentation.menu_view.as_mut().unwrap());
                let state = Arc::make_mut(&mut view.minecraft_import);
                state.profiles = (0..40).map(|index| format!("Profile {index}")).collect();
                if phase != Phase::Profiles {
                    state.body = "Import details\n".repeat(80);
                }
                let actions = state.actions();
                let fixed_action = MenuAction::MinecraftImport(
                    *actions
                        .iter()
                        .find(|action| !matches!(action, Action::Select(_)))
                        .unwrap(),
                );
                let button = |hits: &[(MenuAction, UiRect)]| {
                    hits.iter()
                        .rev()
                        .find_map(|(at, bounds)| (*at == fixed_action).then_some(*bounds))
                        .unwrap()
                };
                let ordinary = paint_at(&mut presentation, size, dpi).0;
                for enabled in [1, 0] {
                    let view = Arc::make_mut(presentation.menu_view.as_mut().unwrap());
                    Arc::make_mut(&mut view.settings_options)
                        .set(option_index(EXTRA_LARGE_UI), enabled);
                    view.settings_options = Arc::new(
                        SettingsOptions::decode(
                            &serde_json::to_vec(&view.settings_options).unwrap(),
                        )
                        .unwrap(),
                    );
                    let (hits, nodes) = paint_at(&mut presentation, size, dpi);
                    for action in actions
                        .iter()
                        .filter(|action| !matches!(action, Action::Select(_)))
                    {
                        assert!(
                            hits.iter()
                                .any(|(at, _)| *at == MenuAction::MinecraftImport(*action))
                        );
                    }
                    for (action, bounds) in &hits {
                        assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
                        assert!(bounds.min().x() >= 0.0 && bounds.min().y() >= 0.0);
                        assert!(bounds.max().x() <= size[0] && bounds.max().y() <= size[1]);
                        assert_eq!(presentation.hit_test_menu(centre(*bounds)), Some(*action));
                    }
                    if size == [1280.0, 720.0] && enabled == 1 {
                        let large = button(&hits);
                        let normal = button(&ordinary);
                        assert!((large.height() / normal.height() - 1.5).abs() < 0.001);
                        let point = UiPoint::new(
                            (large.min().x() + normal.min().x()) * 0.5,
                            centre(large).y(),
                        )
                        .unwrap();
                        assert!(!normal.contains(point));
                        assert_eq!(presentation.hit_test_menu(point), Some(fixed_action));
                    }
                    if enabled == 0 {
                        assert_eq!(hits, ordinary);
                    }
                    if phase == Phase::Profiles {
                        let viewport = nodes
                            .iter()
                            .rev()
                            .find(|node| node.clips_children())
                            .unwrap()
                            .bounds();
                        assert!(
                            presentation
                                .menu_scrolls
                                .wheel(centre(viewport), -100000.0, true)
                        );
                        let (scrolled, _) = paint_at(&mut presentation, size, dpi);
                        let last = MenuAction::MinecraftImport(Action::Select(39));
                        let bounds = scrolled
                            .iter()
                            .find_map(|(at, bounds)| (*at == last).then_some(*bounds))
                            .expect("the last profile remains reachable by scrolling");
                        assert_eq!(presentation.hit_test_menu(centre(bounds)), Some(last));
                        // Return to the same starting offset before changing the saved preference.
                        assert!(
                            presentation
                                .menu_scrolls
                                .wheel(centre(viewport), 100000.0, true)
                        );
                    }
                }
            }
        }
    }
}

/// Opens the import modal over the fallback launcher with no focused underlying control.
fn fixture(phase: Phase) -> UiPresentationRuntime {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.dialog = Some(MenuDialog::MinecraftImport);
    view.focused_action = None;
    Arc::make_mut(&mut view.settings_options).set(
        launcher::menu::settings_options::SettingId::ScreenAnimations.index(),
        0,
    );
    Arc::make_mut(&mut view.minecraft_import).phase = phase;
    presentation.menu_view = Some(Arc::new(view));
    presentation
}

#[test]
fn fallback_import_profiles_remain_mouse_selectable_after_scrolling() {
    let mut presentation = fixture(Phase::Profiles);
    let view = Arc::make_mut(presentation.menu_view.as_mut().unwrap());
    Arc::make_mut(&mut view.minecraft_import).profiles =
        (0..40).map(|i| format!("Profile {i}")).collect();
    let (hits, _) = paint(&mut presentation);
    for index in [0, 1] {
        assert!(
            hits.iter()
                .any(|(action, _)| *action == MenuAction::MinecraftImport(Action::Select(index)))
        );
    }
    assert!(
        hits.iter()
            .all(|(action, _)| matches!(action, MenuAction::MinecraftImport(_)))
    );
    assert!(
        presentation
            .menu_scrolls
            .wheel(UiPoint::new(640.0, 360.0).unwrap(), -100000.0, true)
    );
    let (scrolled, _) = paint(&mut presentation);
    assert!(
        scrolled
            .iter()
            .any(|(action, _)| *action == MenuAction::MinecraftImport(Action::Select(39)))
    );
    assert_eq!(paint(&mut presentation).0, scrolled);
}

#[test]
fn fallback_import_preview_scrolls_without_moving_its_actions() {
    let mut presentation = fixture(Phase::Preview);
    let view = Arc::make_mut(presentation.menu_view.as_mut().unwrap());
    Arc::make_mut(&mut view.minecraft_import).body = "Preview entry\n".repeat(80);
    let (initial, nodes) = paint(&mut presentation);
    for expected in [Action::Apply, Action::Begin, Action::Close] {
        assert!(
            initial
                .iter()
                .any(|(action, _)| *action == MenuAction::MinecraftImport(expected))
        );
    }
    let body_bounds = |nodes: &[UiNode]| {
        nodes
            .iter()
            .rev()
            .find(|node| matches!(node.visual(), UiVisual::Text { .. }))
            .unwrap()
            .bounds()
    };
    let initial_body = body_bounds(&nodes);
    assert!(
        presentation
            .menu_scrolls
            .wheel(UiPoint::new(640.0, 360.0).unwrap(), -100000.0, true)
    );
    let (scrolled, nodes) = paint(&mut presentation);
    assert!(body_bounds(&nodes).min().y() < initial_body.min().y());
    assert_eq!(initial, scrolled);
}
