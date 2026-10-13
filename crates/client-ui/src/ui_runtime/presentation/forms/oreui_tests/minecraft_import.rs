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
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut nodes = Vec::new();
    let hits = presentation
        .append_menu(
            &UiRuntime::new(1),
            &mut nodes,
            &mut 1,
            metrics,
            1280.0,
            720.0,
        )
        .unwrap();
    (hits, nodes)
}

/// Opens the import modal over the fallback launcher with no focused underlying control.
fn fixture(phase: Phase) -> UiPresentationRuntime {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.dialog = Some(MenuDialog::MinecraftImport);
    view.focused_action = None;
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
