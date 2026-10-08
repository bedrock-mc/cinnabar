//! Bootstrap rendering uses only the shipped font and original logo.

use super::*;

/// Loads the open font directly, before any installed carriers exist.
fn font() -> Text {
    Text::from_bytes(include_bytes!(
        "../../../../../assets/fonts/CinnanglesSans.ttf"
    ))
    .unwrap()
}

/// Representative setup states include long titles, consent, progress and retry.
fn screens() -> Vec<Screen> {
    vec![
        Screen::Consent,
        Screen::Starting,
        Screen::Downloading {
            received: 81_200_000,
            total: Some(162_400_000),
            bytes_per_second: Some(5_300_000.0),
        },
        Screen::Preparing {
            step: 1,
            total: assets::carriers::CARRIERS
                .iter()
                .filter(|carrier| carrier.installed)
                .count()
                + 1,
            label: crate::first_run::prepare::UNPACK_LABEL.into(),
        },
        Screen::Failed {
            message: "Download failed: connection timed out. Check your connection and try again."
                .into(),
        },
        Screen::Done,
    ]
}

#[test]
fn setup_actions_and_wrapped_text_fit_gui_scales_and_dpi() {
    let mut text = font();
    let logo = super::super::decode_logo();
    for size in [[560, 420], [1024, 640], [2048, 1280]] {
        for offset in [-3, 0] {
            for screen in screens() {
                let mut canvas = Canvas::new(size[0], size[1]);
                let style = Style {
                    rem: f32::from(ui::DesktopGuiScale::for_window(size).scale_for_offset(offset))
                        * theme::GUI_PIXELS_PER_REM,
                    appearance: Appearance::Default,
                    hovered: None,
                    focused: None,
                    pressed: None,
                    updating: true,
                    logo: logo.as_ref(),
                    log_hint: "/scratch/install/logs/first-run.log",
                };
                let layout = layout(&canvas, &mut text, &screen, &style);
                let hits = draw(&mut canvas, &mut text, &screen, &style);
                assert_eq!(
                    hits.iter().map(|(action, _)| *action).collect::<Vec<_>>(),
                    screen.actions()
                );
                assert!(layout.panel.y >= 0.0 && layout.panel.y + layout.panel.h <= size[1] as f32);
                for (_, rect) in hits {
                    assert!(rect.w > 0.0 && rect.h > 0.0);
                    assert!(rect.x >= 0.0 && rect.x + rect.w <= size[0] as f32);
                    assert!(rect.y >= 0.0 && rect.y + rect.h <= size[1] as f32);
                }
                let max_width = layout.panel.w - 2.0 * theme::LOADING_PAD * layout.rem;
                for line in layout.title {
                    assert!(text.width(&line, theme::HEADER5.size * layout.rem) <= max_width);
                }
            }
        }
    }
}

#[test]
fn render_bootstrap_states_without_downloaded_assets() {
    let root = std::env::temp_dir().join("cinnabar-first-run-style");
    std::fs::create_dir_all(&root).unwrap();
    let mut text = font();
    let logo = super::super::decode_logo();
    for (name, screen) in [
        "consent",
        "starting",
        "downloading",
        "unpacking",
        "error",
        "done",
    ]
    .into_iter()
    .zip(screens())
    {
        for appearance in [Appearance::Default, Appearance::Dark] {
            let mut canvas = Canvas::new(1024, 640);
            canvas.fill(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 1024.0,
                    h: 640.0,
                },
                theme::NEUTRAL90,
            );
            let style = Style {
                rem: ui::gui_scale([1024, 640], None) as f32 * theme::GUI_PIXELS_PER_REM,
                appearance,
                hovered: None,
                focused: screen.actions().first().copied(),
                pressed: None,
                updating: true,
                logo: logo.as_ref(),
                log_hint: "/scratch/install/logs/first-run.log",
            };
            draw(&mut canvas, &mut text, &screen, &style);
            image::save_buffer(
                root.join(format!("{name}-{appearance:?}.png")),
                &canvas.pixels,
                canvas.width,
                canvas.height,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
}
