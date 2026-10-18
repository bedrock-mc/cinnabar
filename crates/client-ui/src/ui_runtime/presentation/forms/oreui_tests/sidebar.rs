//! The settings sidebar's icon sweep through the menu adapter's clock.

use super::super::super::UiPresentationRuntime;
use launcher::menu::settings_sections::SETTINGS_SECTIONS;
use launcher::menu::{MenuScreen, MenuView};
use ui_engine::test_support::fixture_font;
use ui_engine::text_metrics::TextMetrics;

fn section_index(name: &str) -> u8 {
    SETTINGS_SECTIONS
        .iter()
        .find_map(|(key, index)| (*key == name).then_some(*index))
        .expect("registered settings section")
}

fn presentation() -> (UiPresentationRuntime, u16) {
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let page = runtime.textures.dynamic_start() as u16 + 1;
    runtime
        .enable_oreui_originals(oreui::test_support::settings_icon_images())
        .unwrap();
    (runtime, page)
}

fn frame(
    runtime: &mut UiPresentationRuntime,
    section: u8,
    seconds: f64,
    page: u16,
) -> Vec<[u16; 4]> {
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Settings;
    view.settings.section = section;
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let (mut nodes, mut next) = (Vec::new(), 1);
    runtime.menu_seconds = seconds;
    runtime
        .append_oreui_screen(
            &view,
            &mut nodes,
            &mut next,
            metrics,
            [1280.0, 720.0],
            None,
            &|_| None,
        )
        .unwrap();
    runtime.end_animation_frame();
    nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Sprite {
                texture_page, uv, ..
            } if *texture_page == page => Some(*uv),
            _ => None,
        })
        .collect()
}

#[test]
fn selected_settings_icon_sweeps_once_and_restarts_on_reselection() {
    let (mut runtime, page) = presentation();
    let controller = section_index("controller_and_switch_forced_index");
    let audio = section_index("sound_forced_index");
    assert_eq!(frame(&mut runtime, controller, 2.0, page), [[0, 0, 24, 24]]);
    assert_eq!(
        frame(&mut runtime, controller, 2.051, page),
        [[48, 0, 72, 24]]
    );
    assert_eq!(
        frame(&mut runtime, controller, 2.5, page),
        [[192, 0, 216, 24]]
    );
    assert_eq!(
        frame(&mut runtime, controller, 12.0, page),
        [[192, 0, 216, 24]]
    );
    assert_eq!(frame(&mut runtime, audio, 12.1, page), [[0, 0, 24, 24]]);
    assert_eq!(
        frame(&mut runtime, controller, 12.2, page),
        [[0, 0, 24, 24]]
    );
    let mut view = MenuView::new(true, "Player".into());
    view.screen = MenuScreen::Home;
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    runtime
        .append_oreui_screen(
            &view,
            &mut Vec::new(),
            &mut 1,
            metrics,
            [1280.0, 720.0],
            None,
            &|_| None,
        )
        .unwrap();
    runtime.end_animation_frame();
    assert_eq!(
        frame(&mut runtime, controller, 13.0, page),
        [[0, 0, 24, 24]]
    );
}
