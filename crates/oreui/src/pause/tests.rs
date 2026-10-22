//! Pause action geometry remains consistent and reachable across GUI scales.

use super::*;
use crate::review_tests::paint;
use ui_engine::text_metrics::TextMetrics;

#[test]
fn pause_actions_use_standard_height_and_separators() {
    for (size, scale) in [([854.0, 460.0], 1), ([1280.0, 700.0], 2)] {
        let metrics = TextMetrics::for_viewport(
            size.map(|v| v as u32),
            ui::DpiScale::new(1.0).unwrap(),
            Some(scale),
        );
        let gui = metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
        let (_, hits, _) = paint(Default::default(), |c| {
            c.metrics = metrics;
            c.rem = gui * GUI_PIXELS_PER_REM;
            draw(c, &MenuView::new(true, "Fixture".into()), size, &|_| None).unwrap();
        });
        let mut previous = None;
        for action in [
            MenuAction::PauseResume,
            MenuAction::PauseSettings,
            MenuAction::PauseDisconnect,
        ] {
            let b = hits.iter().find(|(found, _)| *found == action).unwrap().1;
            assert!((b.height() - 28.0 * gui).abs() < 0.01);
            if let Some(y) = previous {
                assert!((b.min().y() - y - 31.0 * gui).abs() < 0.01);
            }
            previous = Some(b.min().y());
            assert!(b.max().x() < size[0] * 0.56);
        }
    }
}

#[test]
fn every_pause_action_fits_without_overlapping_the_preview() {
    for size in [
        ui::MIN_WINDOW_LOGICAL_SIZE,
        [480.0, 270.0],
        [480.0, 640.0],
        [1000.0, 560.0],
        [1280.0, 720.0],
    ] {
        for scale in [1, 2, 4, 8] {
            let metrics = TextMetrics::for_viewport(
                size.map(|v| v as u32),
                ui::DpiScale::new(1.0).unwrap(),
                Some(scale),
            );
            let mut preview = None;
            let (_, hits, _) = paint(Default::default(), |c| {
                c.metrics = metrics;
                c.rem =
                    metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32 * GUI_PIXELS_PER_REM;
                let rem = c.rem;
                c.title_artwork = Some(ui::IconRef {
                    page: 18,
                    uv: [0, 0, 300, 100],
                    glint: false,
                });
                preview = draw(c, &MenuView::new(true, "Fixture".into()), size, &|_| None).unwrap();
                assert_eq!(c.rem, rem);
                assert_eq!(c.metrics.scale, metrics.scale);
            });
            assert_eq!(hits.len(), 4);
            for (i, (_, b)) in hits.iter().enumerate() {
                assert!(
                    b.min().x() >= 0.0 && b.max().x() <= size[0] + 0.01,
                    "{size:?} {b:?}"
                );
                assert!(b.min().y() >= 0.0 && b.max().y() <= size[1] + 0.01);
                for (_, other) in &hits[i + 1..] {
                    assert!(
                        b.max().x() <= other.min().x()
                            || other.max().x() <= b.min().x()
                            || b.max().y() <= other.min().y()
                            || other.max().y() <= b.min().y()
                    );
                }
            }
            if let Some(p) = preview {
                assert!(p.control[3] > p.control[1]);
                let wardrobe = hits
                    .iter()
                    .find(|(action, _)| *action == MenuAction::Navigate(MenuScreen::DressingRoom))
                    .unwrap()
                    .1;
                assert!(p.control[3] < wardrobe.min().y());
            }
        }
    }
}
