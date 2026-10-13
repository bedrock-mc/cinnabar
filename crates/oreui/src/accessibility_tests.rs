//! Accessibility sizing through the same screen entry point the client paints.

use std::{collections::HashMap, sync::Arc};

use launcher::menu::{
    MenuAction, MenuScreen, MenuView,
    settings_options::{EXTRA_LARGE_UI, SettingsOptions},
};
use ui::{DpiScale, SafeArea, TextLayoutCache, UiNode, UiPoint, UiRect};
use ui_engine::{
    engine::screen::{PaintFrame, Screen},
    menu_scroll::MenuScrolls,
    test_support::fixture_font,
    text_metrics::TextMetrics,
};

use crate::{
    OreUiScreen,
    menu_screen::{MenuGeometry, MenuInput},
    test_support::option_index,
};

struct Painted {
    nodes: Vec<UiNode>,
    hits: Vec<(MenuAction, UiRect)>,
}

/// Paints a real menu route with unchanged gameplay metrics and no installed assets.
fn paint(
    screen: &mut OreUiScreen,
    view: &MenuView,
    metrics: TextMetrics,
    size: [f32; 2],
) -> Painted {
    paint_with_scrolls(screen, view, metrics, size, &mut MenuScrolls::default())
}

/// Paints a route with the offsets produced by actual scrolling of its previous frame.
fn paint_with_scrolls(
    screen: &mut OreUiScreen,
    view: &MenuView,
    metrics: TextMetrics,
    size: [f32; 2],
    scrolls: &mut MenuScrolls<MenuAction>,
) -> Painted {
    let font = fixture_font();
    let mut layouts = TextLayoutCache::new(64, 1024 * 1024);
    let (mut nodes, mut next) = (Vec::new(), 1);
    let artwork = HashMap::new();
    let mut frame = PaintFrame {
        layouts: &mut layouts,
        font: &font,
        metrics,
        solid_page: 0,
        safe_area: SafeArea::ZERO,
        content: size,
        translate: &|_| None,
        language: [0; 3],
        now: 0.0,
        nodes: &mut nodes,
        next: &mut next,
        renderers: &mut [],
    };
    let output = screen
        .paint(
            MenuInput {
                view,
                size,
                portrait: None,
                translate: &|_| None,
                artwork: &artwork,
                title_artwork: None,
                scrolls,
                geometry: MenuGeometry::default(),
            },
            &mut frame,
        )
        .unwrap();
    // The OreUI adjustment must never leak into gameplay's frame metrics.
    assert_eq!(frame.metrics.gui_scale, metrics.gui_scale);
    assert_eq!(frame.metrics.scale, metrics.scale);
    Painted {
        nodes,
        hits: output.hits,
    }
}

#[test]
fn extra_large_add_server_keeps_fields_and_actions_reachable_in_small_windows() {
    for enabled in [0, 1] {
        for size in [[400.0, 250.0], ui::MIN_WINDOW_LOGICAL_SIZE, [752.0, 500.0]] {
            for dpi in [1.0, 2.0] {
                let mut view = MenuView::new(true, "Fixture".into());
                view.screen = MenuScreen::AddServer;
                view.name = "Fixture".into();
                view.address = "fixture.invalid".into();
                Arc::make_mut(&mut view.settings_options)
                    .set(option_index(EXTRA_LARGE_UI), enabled);
                let metrics = TextMetrics::for_viewport(
                    size.map(|value| (value * dpi) as u32),
                    DpiScale::new(dpi).unwrap(),
                    Some(2),
                );
                let mut screen = OreUiScreen::default();
                let mut scrolls = MenuScrolls::default();
                let mut reached = [false; 3];
                for _ in 0..32 {
                    let frame = paint_with_scrolls(&mut screen, &view, metrics, size, &mut scrolls);
                    for action in [
                        MenuAction::AddBack,
                        MenuAction::AddSave,
                        MenuAction::AddSaveConnect,
                    ] {
                        let bounds = frame
                            .hits
                            .iter()
                            .find_map(|(at, bounds)| (*at == action).then_some(*bounds))
                            .expect("the close and save actions stay visible");
                        assert!(bounds.min().x() >= 0.0 && bounds.min().y() >= 0.0);
                        assert!(bounds.max().x() <= size[0] && bounds.max().y() <= size[1]);
                    }
                    let areas = screen.take_paint().unwrap().scrolls;
                    let fields = areas
                        .iter()
                        .find(|area| area.key == "add_server_fields")
                        .expect("server fields have a scroll viewport");
                    assert!(fields.viewport.width() > 0.0 && fields.viewport.height() > 0.0);
                    for (index, action) in [
                        MenuAction::AddName,
                        MenuAction::AddAddress,
                        MenuAction::AddPort,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        reached[index] |= frame.hits.iter().any(|(at, bounds)| {
                            *at == action && bounds.width() > 0.0 && bounds.height() > 0.0
                        });
                    }
                    if reached.into_iter().all(|value| value) {
                        break;
                    }
                    let point = crate::test_support::centre(fields.viewport);
                    let delta = -fields.viewport.height() * 0.5;
                    scrolls.set_areas(areas);
                    assert!(
                        scrolls.wheel(point, delta, true),
                        "each field must be reachable: {size:?}, DPI {dpi}, enabled {enabled}"
                    );
                }
                assert!(
                    reached.into_iter().all(|value| value),
                    "each field must be reachable: {size:?}, DPI {dpi}, enabled {enabled}"
                );
            }
        }
    }
}

/// Returns the actual navigation hit region the pointer uses.
fn back(frame: &Painted) -> UiRect {
    frame
        .hits
        .iter()
        .find_map(|(action, bounds)| (*action == MenuAction::AddBack).then_some(*bounds))
        .expect("menu route has a back control")
}

/// Measures the painted title's height using its laid-out node bounds.
fn title_height(frame: &Painted, title: &str) -> f32 {
    frame
        .nodes
        .iter()
        .find_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. }
                if layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect::<String>()
                    == title =>
            {
                Some(node.bounds().height())
            }
            _ => None,
        })
        .expect("menu title is painted")
}

#[test]
fn extra_large_ui_resizes_real_menu_text_controls_and_pointer_targets() {
    for (physical, gui) in [([1280, 720], 2), ([1920, 1080], 4)] {
        for dpi in [1.0, 2.0] {
            for (route, title) in [
                (MenuScreen::Settings, "SETTINGS"),
                (MenuScreen::Profile, "Your Profile"),
            ] {
                let mut view = crate::test_support::settings_view("video_forced_index");
                view.screen = route;
                let index = option_index(EXTRA_LARGE_UI);
                let metrics =
                    TextMetrics::for_viewport(physical, DpiScale::new(dpi).unwrap(), Some(gui));
                let size = physical.map(|value| value as f32 / dpi);
                let mut screen = OreUiScreen::default();
                let normal = paint(&mut screen, &view, metrics, size);
                assert!(Arc::make_mut(&mut view.settings_options).set(index, 1));
                // Loading saved settings exercises the same state as a fresh client session.
                view.settings_options = Arc::new(
                    SettingsOptions::decode(&serde_json::to_vec(&view.settings_options).unwrap())
                        .unwrap(),
                );
                let large = paint(&mut screen, &view, metrics, size);
                let factor = (f32::from(gui) + 1.0) / f32::from(gui);
                assert!(
                    (back(&large).width() / back(&normal).width() - factor).abs() < 0.001,
                    "extra large UI must resize actual controls: {route:?}, DPI {dpi}"
                );
                assert!(
                    (title_height(&large, title) / title_height(&normal, title) - factor).abs()
                        < 0.01,
                    "extra large UI must resize text with its controls"
                );
                let point = UiPoint::new(
                    (back(&normal).max().x() + back(&large).max().x()) * 0.5,
                    back(&normal).height() * 0.5,
                )
                .unwrap();
                assert!(!back(&normal).contains(point));
                assert!(
                    back(&large).contains(point),
                    "enlarged controls need enlarged pointer targets"
                );
                assert!(Arc::make_mut(&mut view.settings_options).set(index, 0));
                let restored = paint(&mut screen, &view, metrics, size);
                assert_eq!(back(&restored), back(&normal));
                assert_eq!(title_height(&restored, title), title_height(&normal, title));
            }
        }
    }
}
