//! Home geometry acceptance across display sizes and GUI preferences.

use super::*;
use crate::theme::GUI_PIXELS_PER_REM;
use ui::DpiScale;
use ui_engine::text_metrics::TextMetrics;

/// Draws a complete fixture and checks visible, disjoint targets without carrier dependencies.
fn assert_fits(
    size: [f32; 2],
    dpi: f32,
    preference: Option<u8>,
    event: bool,
    update: bool,
    artwork: Option<IconRef>,
) {
    let metrics = TextMetrics::for_viewport(
        size.map(|v| (v * dpi).round() as u32),
        DpiScale::new(dpi).unwrap(),
        preference,
    );
    let mut view = view();
    if event {
        view.feeds.home.live_event = Some(launcher::menu::LiveEventCard {
            button_text: "Live event".into(),
            ..Default::default()
        });
    }
    if update {
        view.updates.ready = Some(launcher::menu::updates::Ready {
            version: launcher::PRODUCT_VERSION.into(),
            writable: true,
            protocol_required: false,
        });
    }
    let mut targets = Vec::new();
    let mut preview = None;
    let (scrolls, hits, _) = paint(HashMap::new(), |canvas| {
        canvas.metrics = metrics;
        canvas.rem = metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32 * GUI_PIXELS_PER_REM;
        let rem = canvas.rem;
        canvas.title_artwork = artwork;
        canvas.capture_focus = true;
        preview = draw(canvas, &view, size, None, &|_| None).unwrap();
        targets = canvas.focus_hits.clone();
        assert_eq!(canvas.rem, rem);
        assert_eq!(canvas.metrics.scale, metrics.scale);
    });
    assert!(scrolls.is_empty());
    assert_eq!(hits.len(), 9 + usize::from(event) + usize::from(update));
    for (action, bounds) in targets {
        assert_eq!(
            hits.iter().find(|(found, _)| *found == action).unwrap().1,
            bounds,
            "clipped {action:?} at {size:?}"
        );
        assert!(
            bounds.min().x() >= 0.0 && bounds.max().x() <= size[0] + 0.01,
            "horizontal {action:?} {bounds:?} at {size:?}"
        );
        assert!(
            bounds.min().y() >= 0.0 && bounds.max().y() <= size[1] + 0.01,
            "vertical {action:?} {bounds:?} at {size:?}"
        );
    }
    for (i, (action, a)) in hits.iter().enumerate() {
        for (other, b) in &hits[i + 1..] {
            assert!(
                a.max().x() <= b.min().x() + 0.01
                    || b.max().x() <= a.min().x() + 0.01
                    || a.max().y() <= b.min().y() + 0.01
                    || b.max().y() <= a.min().y() + 0.01,
                "overlapping {action:?} and {other:?} at {size:?}, DPI{dpi}, preference{preference:?}"
            );
        }
    }
    if let Some(preview) = preview {
        let wardrobe = hits
            .iter()
            .find(|(action, _)| *action == MenuAction::Navigate(MenuScreen::DressingRoom))
            .unwrap()
            .1;
        assert!(preview.control[3] > preview.control[1]);
        assert!(preview.control[3] < wardrobe.min().y());
    }
}

#[test]
fn main_actions_use_equal_full_width_rows() {
    for (size, scale) in [([854.0, 460.0], 1), ([1280.0, 700.0], 2)] {
        let metrics = TextMetrics::for_viewport(
            size.map(|v| v as u32),
            DpiScale::new(1.0).unwrap(),
            Some(scale),
        );
        let gui = metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
        let (_, hits, _) = paint(HashMap::new(), |canvas| {
            canvas.metrics = metrics;
            canvas.rem = gui * GUI_PIXELS_PER_REM;
            draw(canvas, &view(), size, None, &|_| None).unwrap();
        });
        let mut previous: Option<f32> = None;
        for route in [
            MenuScreen::Play,
            MenuScreen::Settings,
            MenuScreen::Social,
            MenuScreen::Servers,
        ] {
            let b = hits
                .iter()
                .find(|(action, _)| *action == MenuAction::Navigate(route))
                .unwrap()
                .1;
            assert!((b.width() - 148.0 * gui).abs() < 0.01);
            assert!((b.height() - 30.0 * gui).abs() < 0.01);
            assert!(((b.min().x() + b.max().x()) * 0.5 - size[0] * 0.5).abs() < 0.01);
            if let Some(y) = previous {
                assert!((b.min().y() - y - 32.0 * gui).abs() < 0.01);
            }
            previous = Some(b.min().y());
        }
    }
}

#[test]
fn all_actions_fit_small_portrait_and_desktop_windows_at_every_gui_choice() {
    for size in [
        ui::MIN_WINDOW_LOGICAL_SIZE,
        [480.0, 270.0],
        [480.0, 640.0],
        [640.0, 420.0],
        [960.0, 440.0],
        [1000.0, 560.0],
        [1280.0, 720.0],
        [1920.0, 1080.0],
    ] {
        for dpi in [0.5, 1.0, 2.0, 4.0, 8.0] {
            let physical = size.map(|value| (value * dpi).round() as u32);
            let choices = ui::DesktopGuiScale::for_window(physical);
            for preference in [None, Some(1), Some(2)].into_iter().chain(
                choices
                    .choices()
                    .map(|choice| Some(choices.scale_for_offset(choice.offset))),
            ) {
                assert_fits(size, dpi, preference, false, false, None);
                assert_fits(
                    size,
                    dpi,
                    preference,
                    true,
                    true,
                    Some(IconRef {
                        page: 18,
                        uv: [0, 0, 300, 100],
                        glint: false,
                    }),
                );
            }
        }
    }
}

#[test]
fn narrow_square_home_keeps_servers_and_wardrobe_disjoint() {
    for size in [
        [240.0, 240.0],
        [320.0, 240.0],
        [396.0, 230.0],
        [398.0, 230.0],
        [400.0, 240.0],
    ] {
        for artwork in [
            None,
            Some(IconRef {
                page: 18,
                uv: [0, 0, 300, 100],
                glint: false,
            }),
        ] {
            assert_fits(size, 1.0, Some(1), false, false, artwork);
        }
    }
}
