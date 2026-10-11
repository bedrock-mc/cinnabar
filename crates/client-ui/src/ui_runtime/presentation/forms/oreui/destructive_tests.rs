use super::review_tests::{paint, solids};
use super::theme::{MENU_DESTRUCTIVE, MENU_NEUTRAL};
use std::collections::HashMap;
use {
    super::*,
    launcher::menu::{MenuAction, MenuScreen, MenuView},
};

/// Finds the rendered label contained by an action's hit area.
fn label(nodes: &[UiNode], bounds: UiRect) -> String {
    nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } if bounds.contains(node.bounds().min()) => Some(
                layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<String>()
        .replace(' ', "")
}

/// Checks the action face's theme colour without pinning its layout.
fn face_has(nodes: &[UiNode], bounds: UiRect, color: [u8; 4]) -> bool {
    solids(nodes).iter().any(|(area, fill)| {
        *fill == color && bounds.contains(UiPoint::new(area[0], area[1]).unwrap())
    })
}

#[test]
fn pause_quit_uses_vanilla_label_and_neutral_style_for_local_and_multiplayer() {
    for kind in [
        launcher::menu::JoinKind::Local,
        launcher::menu::JoinKind::External,
    ] {
        let mut view = MenuView::new(true, "Player".into());
        view.screen = MenuScreen::Pause;
        view.feeds.join.kind = kind;
        let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
            super::pause::draw(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
        });
        let bounds = hits
            .iter()
            .find(|(action, _)| *action == MenuAction::PauseDisconnect)
            .unwrap()
            .1;
        assert_eq!(label(&nodes, bounds), "Save&Quit");
        assert!(face_has(&nodes, bounds, MENU_NEUTRAL.fill));
        assert!(!face_has(&nodes, bounds, MENU_DESTRUCTIVE.fill));
    }
}

#[test]
fn main_quit_uses_existing_destructive_style_without_recoloring_friends() {
    let view = MenuView::new(true, "Player".into());
    let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
        super::home::draw(canvas, &view, [1280.0, 720.0], None, &|_| None).unwrap();
    });
    let bounds = |action| hits.iter().find(|(found, _)| *found == action).unwrap().1;
    let quit = bounds(MenuAction::OpenExitDialog);
    assert_eq!(label(&nodes, quit), "Quit");
    assert!(face_has(&nodes, quit, MENU_DESTRUCTIVE.fill));
    assert!(face_has(
        &nodes,
        bounds(MenuAction::Navigate(MenuScreen::Friends)),
        MENU_NEUTRAL.fill
    ));
}

#[test]
fn pause_quit_uses_installed_translation_through_the_screen_route() {
    let mut presentation =
        UiPresentationRuntime::new(crate::ui_runtime::presentation::tests::fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Pause;
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let (mut nodes, mut next) = (Vec::new(), 1);
    let hits = presentation
        .append_oreui_screen(
            &view,
            &mut nodes,
            &mut next,
            metrics,
            [1280.0, 720.0],
            None,
            &|key| (key == "pauseScreen.quit").then(|| "Quitter".into()),
        )
        .unwrap()
        .unwrap();
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::PauseDisconnect)
    );
    assert!(
        presentation
            .form_presentation
            .menu_focus
            .contains(&MenuAction::PauseDisconnect)
    );
    assert!(nodes.iter().any(
        |node| matches!(node.visual(), ui::UiVisual::Text { layout, .. }
        if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>() == "Quitter")
    ));
}
