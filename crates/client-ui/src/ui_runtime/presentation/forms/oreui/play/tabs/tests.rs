use super::*;
use crate::ui_runtime::{
    oreui_assets::load_optional_oreui_images,
    presentation::{TextMetrics, UiPresentationRuntime, tests::fixture_font},
};
use ui::UiVisual;

#[test]
fn focused_middle_play_tab_keeps_its_right_outline_above_the_next_tab() {
    use crate::ui_runtime::presentation::forms::oreui::review_tests::{paint, solids};
    let mut view = MenuView::new(true, "Fixture".into());
    view.navigation_focus_visible = true;
    view.focused_action = Some(MenuAction::Navigate(MenuScreen::Social));
    let (_, _, nodes) = paint(Default::default(), |canvas| {
        draw(canvas, &view, [100.0, 100.0, 1000.0, 160.0], 1).unwrap();
    });
    let (mut nodes_for_metrics, mut next, mut layouts) =
        (Vec::new(), 1, ui::TextLayoutCache::new(1, 1024));
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let canvas = Canvas::new(
        &mut nodes_for_metrics,
        &mut next,
        &mut layouts,
        &font,
        metrics,
        0,
        None,
    );
    let edge = canvas.r(EDGE);
    let width = (900.0 + edge * 2.0) / 3.0;
    let x = 100.0 + (width - edge) + width + edge * 0.5;
    let visible = solids(&nodes)
        .into_iter()
        .rev()
        .find(|(b, _)| x >= b[0] && x < b[2] && 130.0 >= b[1] && 130.0 < b[3])
        .unwrap()
        .1;
    assert_eq!(visible, OUTLINE);
}

#[test]
fn installed_play_tabs_use_native_faces_and_keep_fixed_targets_while_pressed() {
    let Some(images) = load_optional_oreui_images() else {
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
    let originals = runtime.form_presentation.oreui_originals.as_ref().unwrap();
    for key in [ART[0], ART[2]] {
        let sprite = originals.sprites[key];
        let bottom = if key == ART[0] { 4 } else { 2 };
        let center = [
            sprite.bounds[0] + 2,
            sprite.bounds[1] + 2,
            sprite.bounds[2] - 2,
            sprite.bounds[3] - bottom,
        ];
        assert!(
            rest.iter().any(
                |node| matches!(node.visual(), UiVisual::Sprite { texture_page, uv, .. }
            if *texture_page == originals.page + sprite.page && *uv == center)
            ),
            "missing native tab face {key}"
        );
    }
}
