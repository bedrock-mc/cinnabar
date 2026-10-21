//! World creation through the menu adapter: shipped art and its scroll views.

use super::super::super::UiPresentationRuntime;
use launcher::local_worlds::Screen;
use launcher::local_worlds::world::Difficulty;
use launcher::menu::{LocalWorldAction as A, MenuAction, MenuView};
use ui_engine::test_support::fixture_font;
use ui_engine::text_metrics::TextMetrics;

fn local(action: A) -> MenuAction {
    MenuAction::LocalWorld(action)
}

#[test]
fn create_world_draws_its_shipped_category_art() {
    use oreui::images::{WORLD_CATEGORY_ICONS, checkout_compiled_dir, load_optional_oreui_images};
    let Some(images) = load_optional_oreui_images(&checkout_compiled_dir()) else {
        eprintln!(
            "skipping create_world_draws_its_shipped_category_art: compiled OreUI artwork carrier unavailable"
        );
        return;
    };
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    runtime.enable_oreui_originals(images).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = launcher::menu::MenuScreen::Play;
    view.local.screen = Screen::Create;
    let metrics = TextMetrics::for_viewport([1280, 900], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut nodes = Vec::new();
    runtime
        .append_oreui_screen(
            &view,
            &mut nodes,
            &mut 1,
            metrics,
            [1280.0, 900.0],
            None,
            &|_| None,
        )
        .unwrap();
    let originals = runtime.screens.oreui.originals.as_ref().unwrap();
    for key in WORLD_CATEGORY_ICONS {
        let sprite = originals.sprite(key).unwrap();
        assert!(nodes.iter().any(|node| matches!(node.visual(), ui::UiVisual::Sprite {texture_page, uv, ..} if *texture_page == originals.page() + sprite.page && *uv == sprite.bounds)), "missing world category art {key}");
    }
}

#[test]
fn create_world_keeps_sidebar_scroll_independent_and_reveals_keyboard_choices() {
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = launcher::menu::MenuScreen::Play;
    view.local.screen = Screen::Create;
    view.focused_action = Some(local(A::Difficulty(Difficulty::Hard)));
    view.navigation_focus_visible = true;
    let metrics = TextMetrics::for_viewport([1280, 150], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let hits = runtime
        .append_oreui_screen(
            &view,
            &mut Vec::new(),
            &mut 1,
            metrics,
            [1280.0, 150.0],
            None,
            &|_| None,
        )
        .unwrap()
        .unwrap();
    let rect = hits
        .iter()
        .find(|(action, _)| Some(*action) == view.focused_action)
        .unwrap()
        .1;
    assert!(rect.min().y() >= 0.0 && rect.max().y() <= 150.0);
    assert!(
        runtime
            .projections
            .menu_focus
            .contains(&local(A::NameField))
    );
    assert!(
        runtime
            .menu_scrolls
            .offsets()
            .get("world_create_general")
            .is_some_and(|offset| *offset > 0.0)
    );
    assert_eq!(
        runtime
            .menu_scrolls
            .offsets()
            .get("world_settings_sidebar")
            .copied()
            .unwrap_or(0.0),
        0.0
    );
}
