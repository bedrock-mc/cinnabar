//! The crash window lays out every headline, collapsed and expanded, at any window size.

use std::path::PathBuf;

use super::*;
use crate::window::Headline;

fn font() -> Text {
    Text::from_bytes(include_bytes!(
        "../../../../../assets/fonts/CinnanglesSans.ttf"
    ))
    .unwrap()
}

fn summaries() -> [CrashSummary; 3] {
    let crashed = CrashSummary {
        headline: Headline::Crashed {
            activity: "joining a server".into(),
        },
        fault: "EXCEPTION_ACCESS_VIOLATION reading 0x18".into(),
        location: "cinnabar.exe+0x1a2b3c on thread world-2".into(),
        version: "0.1.20".into(),
        graphics: "Vulkan".into(),
        session: "32 min".into(),
        log_tail: (0..80)
            .map(|line| format!("12:00:{line:02}  INFO  line {line}"))
            .collect(),
        last_packets: vec!["+1.0s  in  LevelChunk  4 KiB".into()],
        folder: PathBuf::from("/scratch/crash-reports/one"),
    };
    let could_not_start = CrashSummary {
        headline: Headline::CouldNotStart,
        fault: format!(
            "required carrier is missing: {}\nrun `make assets` to rebuild it",
            "/scratch/install/assets/compiled/".repeat(8)
        ),
        last_packets: Vec::new(),
        ..crashed.clone()
    };
    let stopped = CrashSummary {
        headline: Headline::Stopped {
            activity: "playing on a server".into(),
        },
        fault: "Bevy app exited after a fatal runtime error: network session stopped unexpectedly"
            .into(),
        ..crashed.clone()
    };
    [crashed, could_not_start, stopped]
}

#[test]
fn every_headline_fits_collapsed_and_expanded_at_every_size() {
    let mut text = font();
    let mut discord = DiscordMark::new();
    let logo = bootstrap_ui::decode_logo();
    for summary in summaries() {
        let details = summary.detail_lines();
        for size in [[560, 420], [960, 640], [2048, 1280]] {
            for expanded in [false, true] {
                let mut canvas = Canvas::new(size[0], size[1]);
                canvas.fill(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        w: size[0] as f32,
                        h: size[1] as f32,
                    },
                    [16, 20, 24, 255],
                );
                let screen = CrashScreen {
                    queued: false,
                    expanded,
                    copied: None,
                };
                let style = Style {
                    rem: f32::from(ui::DesktopGuiScale::for_window(size).scale_for_offset(0))
                        * theme::GUI_PIXELS_PER_REM,
                    appearance: Appearance::Default,
                    hovered: None,
                    focused: None,
                    pressed: None,
                    logo: logo.as_ref(),
                    scroll: f32::MAX,
                    thumb_hovered: false,
                    thumb_dragged: false,
                };
                let drawn_rem = layout(&canvas, &mut text, &summary, &screen, &style).rem;
                let frame = draw(
                    &mut canvas,
                    &mut text,
                    &summary,
                    &details,
                    &mut discord,
                    &screen,
                    &style,
                );
                if size == [960, 640]
                    && !expanded
                    && matches!(summary.headline, Headline::Crashed { .. })
                {
                    save_snapshot(&canvas);
                }
                assert_eq!(frame.hits.len(), super::super::ACTIONS.len());
                for (action, rect) in &frame.hits {
                    assert!(rect.w > 0.0 && rect.h > 0.0);
                    if ROW.contains(action) {
                        let label = text.width(
                            screen.label(*action),
                            theme::SECONDARY_BUTTON.size * drawn_rem,
                        );
                        assert!(
                            label + theme::SPACE[2] * drawn_rem <= rect.w,
                            "{} needs room inside its button",
                            screen.label(*action)
                        );
                    }
                    assert!(rect.x >= 0.0 && rect.x + rect.w <= size[0] as f32);
                    assert!(rect.y >= 0.0 && rect.y + rect.h <= size[1] as f32);
                }
                let log = frame.log.filter(|log| log.visible >= 1.0);
                assert_eq!(log.is_some(), expanded);
                if let Some(log) = log {
                    // The view opens at its end: the whole fault for a start failure.
                    assert_eq!(log.scroll, log.max_scroll);
                }
            }
        }
    }
}

/// Optionally exports this synthetic state for window-free visual review.
fn save_snapshot(canvas: &Canvas) {
    let Some(path) = std::env::var_os("CINNABAR_CRASH_SNAPSHOT") else {
        return;
    };
    let mut encoder = png::Encoder::new(
        std::fs::File::create(path).unwrap(),
        canvas.width,
        canvas.height,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&canvas.pixels)
        .unwrap();
}
