//! Geometry regressions for home windows and GUI scales without installed assets.

use super::*;
use crate::theme::GUI_PIXELS_PER_REM;
use ui::DpiScale;
use ui_engine::text_metrics::TextMetrics;

/// A title fixture with a chosen aspect ratio.
fn title(width: u16, height: u16) -> IconRef {
    IconRef {
        page: 18,
        uv: [0, 0, width, height],
        glint: false,
    }
}

/// Checks the header, full action extent and unclipped hit areas at one viewport and text scale.
fn assert_fits(
    size: [f32; 2],
    dpi: f32,
    preference: Option<u8>,
    artwork: Option<IconRef>,
    event: bool,
    update_ready: bool,
) {
    let physical = size.map(|value| (value * dpi).round() as u32);
    let metrics = TextMetrics::for_viewport(physical, DpiScale::new(dpi).unwrap(), preference);
    let rem = metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32 * GUI_PIXELS_PER_REM;
    let layout = layout::Layout::new(rem, size, artwork, event, update_ready);
    let action_width = layout.width * if layout.compact { 1.0 } else { 0.59 };
    let content_height = rem
        * layout.scale
        * (layout::action_group_height(layout.compact)
            + if event {
                layout::LIVE_EVENT_HEIGHT
            } else {
                0.0
            })
        + layout::action_padding(rem * layout.scale, action_width, layout.compact) * 2.0;
    assert!(
        content_height <= layout.height,
        "content {content_height} exceeds panel {} at {size:?}",
        layout.height
    );
    assert!(layout.height + layout.outside <= size[1] + 0.001);

    let mut view = view();
    if update_ready {
        view.updates.ready = Some(launcher::menu::updates::Ready {
            version: launcher::PRODUCT_VERSION.into(),
            writable: true,
            protocol_required: false,
        });
    }
    if event {
        view.feeds.home.live_event = Some(launcher::menu::LiveEventCard {
            button_text: "Live event".into(),
            ..Default::default()
        });
    }
    let mut targets = Vec::new();
    let mut preview = None;
    let (scrolls, hits, nodes) =
        paint(HashMap::from([("oreui_home/actions".into(), 200.0)]), |c| {
            c.metrics = metrics;
            c.rem = rem;
            c.title_artwork = artwork;
            c.capture_focus = true;
            preview = draw(c, &view, size, None, &|_| None).unwrap();
            targets = c.focus_hits.clone();
            assert_eq!(c.rem, rem, "fit scaling must not affect the next group");
            assert_eq!(c.metrics.scale, metrics.scale);
        });
    assert!(scrolls.is_empty(), "home needs no scroll range at {size:?}");
    let header = nodes
        .iter()
        .find(|node| matches!(node.visual(), UiVisual::Text { layout, .. }
            if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>().replace(' ', "") == "LET'SPLAY"))
        .expect("home keeps its action header");
    let viewport = nodes
        .iter()
        .find(|node| Some(node.id()) == header.parent())
        .expect("action header has a clipping viewport")
        .bounds();
    assert!(
        header.bounds().min().y() >= 0.0 && header.bounds().max().y() <= viewport.height() + 0.001,
        "action header is clipped at {size:?}, DPI {dpi}, preference {preference:?}"
    );
    assert_eq!(
        hits.len(),
        9 + usize::from(event) + usize::from(update_ready)
    );
    if update_ready {
        assert!(
            hits.iter().any(|(action, _)| *action
                == MenuAction::Update(launcher::menu::updates::Action::Restart))
        );
    }
    for (action, bounds) in targets {
        let visible = hits.iter().find(|(found, _)| *found == action).unwrap().1;
        assert_eq!(visible, bounds, "{action:?} is clipped at {size:?}");
        assert!(bounds.min().x() >= 0.0 && bounds.max().x() <= size[0] + 0.001);
        assert!(bounds.min().y() >= 0.0 && bounds.max().y() <= size[1] + 0.001);
    }
    if let Some(preview) = preview {
        let wardrobe = hits
            .iter()
            .find(|(action, _)| *action == MenuAction::Navigate(MenuScreen::DressingRoom))
            .unwrap()
            .1;
        let bounds = ui_engine::engine::rect(
            preview.control[0],
            preview.control[1],
            preview.control[2],
            preview.control[3],
        )
        .unwrap();
        assert!(bounds.height() > 0.0);
        assert!(bounds.max().y() < wardrobe.min().y());
    }
}

#[test]
fn reported_short_window_fits_every_action_without_scrolling() {
    for artwork in [None, Some(title(400, 80)), Some(title(300, 100))] {
        for event in [false, true] {
            assert_fits([1000.0, 560.0], 1.0, None, artwork, event, false);
        }
    }
}

#[test]
fn smallest_supported_window_fits_every_action_without_scrolling() {
    for artwork in [None, Some(title(400, 80)), Some(title(300, 100))] {
        for event in [false, true] {
            assert_fits(
                ui::MIN_WINDOW_LOGICAL_SIZE,
                1.0,
                None,
                artwork,
                event,
                false,
            );
        }
    }
}

#[test]
fn fit_survives_compact_breakpoints_and_display_scales() {
    for size in [
        ui::MIN_WINDOW_LOGICAL_SIZE,
        [480.0, 270.0],
        [640.0, 420.0],
        [960.0, 440.0],
        [1000.0, 560.0],
        [1920.0, 1080.0],
    ] {
        for dpi in [0.5, 1.0, 2.0, 4.0, 8.0] {
            let physical = size.map(|value| (value * dpi).round() as u32);
            let desktop = ui::DesktopGuiScale::for_window(physical);
            let preferences = [None, Some(1), Some(2)].into_iter().chain(
                desktop
                    .choices()
                    .map(|choice| Some(desktop.scale_for_offset(choice.offset))),
            );
            for preference in preferences {
                assert_fits(size, dpi, preference, Some(title(300, 100)), true, false);
            }
        }
    }
}

#[test]
fn every_native_gui_scale_choice_fits_common_window_sizes() {
    for size in [
        [1280.0_f32, 720.0],
        [1366.0, 768.0],
        [1600.0, 900.0],
        [1920.0, 1080.0],
    ] {
        for dpi in [1.0, 1.25, 1.5] {
            let physical = size.map(|value| (value * dpi).round() as u32);
            let desktop = ui::DesktopGuiScale::for_window(physical);
            for choice in desktop.choices() {
                let preference = Some(desktop.scale_for_offset(choice.offset));
                for artwork in [None, Some(title(400, 80)), Some(title(300, 100))] {
                    for event in [false, true] {
                        for update_ready in [false, true] {
                            assert_fits(size, dpi, preference, artwork, event, update_ready);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn update_notice_and_live_event_fit_without_clipping_or_scrolling() {
    for size in [
        ui::MIN_WINDOW_LOGICAL_SIZE,
        [1000.0, 560.0],
        [1920.0, 1080.0],
    ] {
        for artwork in [None, Some(title(400, 80)), Some(title(300, 100))] {
            for event in [false, true] {
                for dpi in [1.0, 2.0, 8.0] {
                    assert_fits(size, dpi, None, artwork, event, true);
                }
            }
        }
    }
}

#[test]
fn normal_window_keeps_panel_logo_footer_and_action_geometry() {
    let size = [1920.0, 1080.0];
    let artwork = title(400, 80);
    let layout = layout::Layout::new(10.0, size, Some(artwork), false, false);
    assert_eq!(layout.scale, 1.0);
    assert!(!layout.compact && !layout.compact_footer);
    assert!((layout.width - 860.0).abs() < 0.001);
    assert!((layout.height - 368.0).abs() < 0.001);
    assert!((layout.logo_width - 560.0).abs() < 0.001);
    assert!((layout.logo_height - 112.0).abs() < 0.001);
    assert!((layout.outside - 220.0).abs() < 0.001);
    let (_, hits, _) = paint(HashMap::new(), |c| {
        c.title_artwork = Some(artwork);
        draw(c, &view(), size, None, &|_| None).unwrap();
    });
    let bounds = |action| hits.iter().find(|(found, _)| *found == action).unwrap().1;
    let play = bounds(MenuAction::Navigate(MenuScreen::Play));
    assert!((play.min().x() - 562.0).abs() < 0.001);
    assert!((play.min().y() - 506.0).abs() < 0.001);
    assert!((play.height() - 64.0).abs() < 0.001);
    let settings = bounds(MenuAction::Navigate(MenuScreen::Settings));
    assert!((settings.min().y() - 582.0).abs() < 0.001);
    assert!((settings.height() - 48.0).abs() < 0.001);
    let account = bounds(MenuAction::OpenAccounts);
    assert!((account.min().y() - 758.0).abs() < 0.001);
    assert!((account.height() - 48.0).abs() < 0.001);
}
