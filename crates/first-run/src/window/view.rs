//! Asset-independent OreUI setup drawing using the shared menu theme and our open font.

use ui::oreui_theme::{self as theme, Appearance};

use super::{
    super::screen::{Action, Screen},
    canvas::{Canvas, Image, Rect, Text},
};

mod controls;
use controls::{button, centred};

const MAX_ERROR_LINES: usize = 10;

pub(super) struct Style<'a> {
    /// Physical pixels per rem, from the same GUI-scale authority as the menus.
    pub rem: f32,
    pub appearance: Appearance,
    pub hovered: Option<Action>,
    pub focused: Option<Action>,
    pub pressed: Option<Action>,
    pub updating: bool,
    pub logo: Option<&'a Image>,
    pub log_hint: &'a str,
}

struct Layout {
    rem: f32,
    panel: Rect,
    logo: Rect,
    title: Vec<String>,
    body: Vec<String>,
    title_line: f32,
    body_line: f32,
}

/// Fits the complete consent and wrapped title before choosing the final physical scale.
fn layout(canvas: &Canvas, text: &mut Text, screen: &Screen, style: &Style<'_>) -> Layout {
    let (width, height) = (canvas.width as f32, canvas.height as f32);
    let mut rem = style.rem.max(1.0);
    loop {
        let margin = theme::SPACE[4] * rem;
        let pad = theme::LOADING_PAD * rem;
        let panel_w = ((theme::LOADING_WIDTH
            + if matches!(screen, Screen::Consent) {
                theme::CANCEL_WIDTH
            } else {
                0.0
            })
            * rem)
            .min(width - margin * 2.0)
            .max(pad * 2.0 + rem);
        let inner = panel_w - pad * 2.0;
        let title_px = theme::HEADER5.size * rem;
        let body_px = theme::CAPTION.size * rem;
        let title = text.wrap(screen.title(style.updating), title_px, inner);
        let detail = screen.body();
        let mut body = text.wrap(&detail, body_px, inner);
        if detail.is_empty() {
            body.clear();
        }
        if matches!(screen, Screen::Failed { .. }) {
            if body.len() > MAX_ERROR_LINES {
                body.truncate(MAX_ERROR_LINES);
                body.push("…".into());
            }
            body.push(String::new());
            body.extend(text.wrap(&format!("Details: {}", style.log_hint), body_px, inner));
        }
        let title_line = (theme::HEADER5.line * rem).max(text.line_height(title_px));
        let body_line = (theme::CAPTION.line * rem).max(text.line_height(body_px));
        let gap = theme::SPACE[1] * rem;
        let panel_h = pad * 2.0
            + title.len() as f32 * title_line
            + if body.is_empty() {
                0.0
            } else {
                gap + body.len() as f32 * body_line
            }
            + if screen.progress().is_some() {
                theme::LOADING_PROGRESS_AREA * rem
            } else {
                0.0
            }
            + if screen.actions().is_empty() {
                0.0
            } else {
                theme::LOADING_FOOTER_AREA * rem
            };
        let logo_w = if style.logo.is_some() {
            (theme::LOADING_WIDTH * 0.75 * rem).min(panel_w * 0.82)
        } else {
            0.0
        };
        let logo_h = style.logo.map_or(0.0, |logo| {
            logo_w * logo.height as f32 / logo.width.max(1) as f32
        });
        let logo_space = if style.logo.is_some() {
            logo_h + theme::SPACE[5] * rem
        } else {
            0.0
        };
        let group_h = panel_h + logo_space;
        if group_h + margin * 2.0 <= height || rem <= 1.0 {
            let top = ((height - group_h) * 0.5).max(0.0);
            return Layout {
                rem,
                panel: Rect {
                    x: (width - panel_w) * 0.5,
                    y: top + logo_space,
                    w: panel_w,
                    h: panel_h,
                },
                logo: Rect {
                    x: (width - logo_w) * 0.5,
                    y: top,
                    w: logo_w,
                    h: logo_h,
                },
                title,
                body,
                title_line,
                body_line,
            };
        }
        rem = (rem * 0.9).max(1.0);
    }
}

/// Draws the bootstrap frame and returns only enabled button hit targets.
pub(super) fn draw(
    canvas: &mut Canvas,
    text: &mut Text,
    screen: &Screen,
    style: &Style<'_>,
) -> Vec<(Action, Rect)> {
    let layout = layout(canvas, text, screen, style);
    let Layout {
        rem,
        panel,
        title_line,
        body_line,
        ..
    } = layout;
    let appearance = style.appearance;
    let edge = theme::EDGE * rem;
    let pad = theme::LOADING_PAD * rem;
    let full = Rect {
        x: 0.0,
        y: 0.0,
        w: canvas.width as f32,
        h: canvas.height as f32,
    };
    canvas.fill(full, appearance.backdrop(theme::OVERLAY_SCREEN));
    if let Some(logo) = style.logo {
        canvas.image(logo, layout.logo);
    }
    canvas.fill(
        Rect {
            x: panel.x + theme::BUTTON_DEPTH * rem,
            y: panel.y + theme::BUTTON_DEPTH * rem,
            ..panel
        },
        theme::TEXT_SHADOW,
    );
    canvas.fill(panel, appearance.surface(theme::NEUTRAL80.fill));
    canvas.frame(panel, edge, appearance.surface(theme::BORDER));
    let inner = panel.inset(pad);
    let mut y = inner.y;
    for line in &layout.title {
        centred(
            canvas,
            text,
            inner,
            y,
            theme::HEADER5.size * rem,
            theme::TEXT,
            line,
        );
        y += title_line;
    }
    if !layout.body.is_empty() {
        y += theme::SPACE[1] * rem;
        for line in &layout.body {
            centred(
                canvas,
                text,
                inner,
                y,
                theme::CAPTION.size * rem,
                theme::TEXT_DIMMER,
                line,
            );
            y += body_line;
        }
    }
    if let Some(fraction) = screen.progress() {
        y += theme::SPACE[3] * rem;
        let track = Rect {
            x: inner.x,
            y,
            w: inner.w,
            h: theme::PROGRESS_HEIGHT * rem,
        };
        canvas.fill(track, appearance.surface(theme::NEUTRAL100));
        let inset = track.inset(edge);
        canvas.fill(inset, appearance.surface(theme::NEUTRAL.fill));
        let fill = Rect {
            w: inset.w * fraction.clamp(0.0, 1.0),
            ..inset
        };
        canvas.fill(fill, theme::PRIMARY_ROLE.fill);
        canvas.fill(
            Rect {
                h: edge.min(fill.h),
                ..fill
            },
            theme::PRIMARY_ROLE.specular[0],
        );
        y += track.h + theme::SPACE[3] * rem;
    }
    let actions = screen.actions();
    if actions.is_empty() {
        return Vec::new();
    }
    y += theme::SPACE[4] * rem;
    let gap = theme::SPACE[1] * rem;
    let widths: Vec<_> = actions
        .iter()
        .map(|action| {
            (text.width(
                screen.button_label(*action),
                theme::SECONDARY_BUTTON.size * rem,
            ) + theme::SPACE[6] * rem)
                .max(theme::CANCEL_WIDTH * rem)
        })
        .collect();
    let row = widths.iter().sum::<f32>() + gap * (actions.len() - 1) as f32;
    let fit = (inner.w / row).min(1.0);
    let mut x = panel.x + (panel.w - row * fit) * 0.5;
    let mut hits = Vec::new();
    for (&action, width) in actions.iter().zip(widths) {
        let rect = Rect {
            x,
            y,
            w: width * fit,
            h: theme::BUTTON_HEIGHT * rem,
        };
        button(canvas, text, screen, style, action, rect, rem);
        hits.push((action, rect));
        x += (width + gap) * fit;
    }
    hits
}

#[cfg(test)]
mod tests;
