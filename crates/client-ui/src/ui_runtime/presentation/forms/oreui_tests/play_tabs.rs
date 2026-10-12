//! Installed Play tab faces through the menu adapter.

use super::super::super::UiPresentationRuntime;
use launcher::menu::{MenuAction, MenuScreen, MenuView};
use oreui::images::{PLAY_TAB_IMAGES, checkout_compiled_dir, load_optional_oreui_images};
use ui::UiVisual;
use ui_engine::test_support::fixture_font;
use ui_engine::text_metrics::TextMetrics;

#[test]
fn installed_play_tabs_use_native_faces_and_keep_fixed_targets_while_pressed() {
    let Some(images) = load_optional_oreui_images(&checkout_compiled_dir()) else {
        eprintln!(
            "skipping installed_play_tabs_use_native_faces_and_keep_fixed_targets_while_pressed: installed OreUI bundle unavailable"
        );
        return;
    };
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    runtime.enable_oreui_originals(images).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Play;
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut frame = |pressed, seconds| {
        view.pressed = pressed;
        runtime.menu_seconds = seconds;
        let (mut nodes, mut next) = (Vec::new(), 1);
        let hits = runtime
            .append_oreui_screen(
                &view,
                &mut nodes,
                &mut next,
                metrics,
                [1280.0, 720.0],
                None,
                &|_| None,
            )
            .unwrap()
            .unwrap();
        runtime.end_animation_frame();
        (nodes, hits)
    };
    let (rest, rest_hits) = frame(None, 0.0);
    let (_, press_hits) = frame(Some(MenuAction::Navigate(MenuScreen::Servers)), 0.1);
    let (_, settled_hits) = frame(Some(MenuAction::Navigate(MenuScreen::Servers)), 0.15);
    assert_eq!(rest_hits, press_hits);
    assert_eq!(rest_hits, settled_hits);
    let originals = runtime.screens.oreui.originals.as_ref().unwrap();
    for key in [PLAY_TAB_IMAGES[0], PLAY_TAB_IMAGES[2]] {
        let sprite = originals.sprite(key).unwrap();
        let bottom = if key == PLAY_TAB_IMAGES[0] { 4 } else { 2 };
        let center = [
            sprite.bounds[0] + 2,
            sprite.bounds[1] + 2,
            sprite.bounds[2] - 2,
            sprite.bounds[3] - bottom,
        ];
        assert!(
            rest.iter().any(
                |node| matches!(node.visual(), UiVisual::Sprite { texture_page, uv, .. }
            if *texture_page == originals.page() + sprite.page && *uv == center)
            ),
            "missing native tab face {key}"
        );
    }
}
