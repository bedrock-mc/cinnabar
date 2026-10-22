//! The crash window's OreUI drawing: the summary panel, the details log and the actions.

use bootstrap_ui::{
    canvas::{Canvas, Image, Rect, Text},
    view::{ButtonState, backdrop, button_label, panel_surface, raised_button},
};
use ui::oreui_theme::{self as theme, Appearance, Rgba};

use super::{
    Copied, CrashAction, CrashScreen, CrashSummary, ROW,
    discord::DiscordMark,
    summary::{DetailLine, Tone},
};

/// The preferred panel width; measured action labels may require more room.
const PANEL_WIDTH: f32 = 64.0;
/// The logo's width; a third of its width tall.
const LOGO_WIDTH: f32 = 24.0;
/// The header badge's side.
const BADGE: f32 = theme::SPACE[6];
const ROW_HEIGHT: f32 = 2.4;
const LOG_TEXT: f32 = theme::SPACE[2];
const LOG_LINE: f32 = 1.6;
/// The details log's height range; the logo yields its space before the log shrinks below
/// `LOG_WITH_LOGO`.
const LOG_MIN: f32 = 8.0;
const LOG_WITH_LOGO: f32 = 16.0;
const LOG_MAX: f32 = 28.0;
/// The scrollbar's width and its thumb's shortest length.
const SCROLLBAR: f32 = theme::SPACE[1];
const THUMB_MIN: f32 = theme::SPACE[5];
/// The pixel-art chevron and check marks' unit: one GUI pixel.
const GUI_PIXEL: f32 = theme::EDGE;
/// The Discord mark's side next to the community button's label.
const DISCORD_ICON: f32 = 2.0;
/// Discord's brand blurple (#5865F2), darker on hover and press, with the menus' raised edges.
const DISCORD_ROLE: theme::Role = theme::Role {
    fill: [0x58, 0x65, 0xf2, 255],
    hovered: [0x47, 0x52, 0xc4, 255],
    pressed: [0x3c, 0x45, 0xa5, 255],
    shadow: [0x3c, 0x45, 0xa5, 255],
    ..theme::MENU_DESTRUCTIVE
};

pub(super) struct Style<'a> {
    /// Physical pixels per rem, from the same GUI-scale authority as the menus.
    pub rem: f32,
    pub appearance: Appearance,
    pub hovered: Option<CrashAction>,
    pub focused: Option<CrashAction>,
    pub pressed: Option<CrashAction>,
    pub logo: Option<&'a Image>,
    /// Log lines scrolled past the top of the details view.
    pub scroll: f32,
    pub thumb_hovered: bool,
    pub thumb_dragged: bool,
}

/// Where the details log landed, for scrolling and thumb dragging.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct LogFrame {
    /// The scrollbar track.
    pub track: Rect,
    pub thumb: Rect,
    /// Physical pixels per log line.
    pub line: f32,
    /// Lines that fit in the view.
    pub visible: f32,
    pub max_scroll: f32,
    /// `Style::scroll` clamped to the content.
    pub scroll: f32,
}

pub(super) struct Frame {
    pub hits: Vec<(CrashAction, Rect)>,
    pub log: Option<LogFrame>,
}

struct Layout {
    rem: f32,
    panel: Rect,
    logo: Option<Rect>,
    body: Vec<String>,
    /// The details view's height in rem, when expanded.
    log: Option<f32>,
}

/// Fits the panel, then the logo and the details log, before shrinking the scale.
fn layout(
    canvas: &Canvas,
    text: &mut Text,
    summary: &CrashSummary,
    screen: &CrashScreen,
    style: &Style<'_>,
) -> Layout {
    let (width, height) = (canvas.width as f32, canvas.height as f32);
    let body_text = summary.body();
    let mut rem = style.rem.max(1.0);
    loop {
        let margin = theme::SPACE[4] * rem;
        let pad = theme::LOADING_PAD * rem;
        let widest_label = ROW
            .iter()
            .map(|action| text.width(screen.label(*action), theme::SECONDARY_BUTTON.size * rem))
            .fold(0.0_f32, f32::max);
        let actions_width = (widest_label + theme::SPACE[2] * rem * 2.0) * ROW.len() as f32
            + theme::SPACE[1] * rem * (ROW.len() - 1) as f32;
        let required = actions_width + pad * 2.0;
        let available = width - margin * 2.0;
        if required > available && rem > 1.0 {
            rem = (rem - 0.5).max(1.0);
            continue;
        }
        let panel_w = (PANEL_WIDTH * rem)
            .max(required)
            .min(available)
            .max(pad * 2.0 + rem);
        let body = text.wrap(&body_text, theme::CAPTION.size * rem, panel_w - pad * 2.0);
        let fixed = theme::LOADING_PAD * 2.0
            + BADGE
            + theme::SPACE[2]
            + body.len() as f32 * theme::CAPTION.line
            + theme::SPACE[4]
            + card_height(summary)
            + theme::SPACE[3]
            + theme::BUTTON_HEIGHT
            + if screen.expanded {
                theme::SPACE[2]
            } else {
                0.0
            }
            + theme::SPACE[4]
            + theme::BUTTON_HEIGHT;
        let logo_space = style.logo.map_or(0.0, |logo| {
            LOGO_WIDTH * logo.height as f32 / logo.width.max(1) as f32
        }) + theme::SPACE[5];
        let room = height / rem - theme::SPACE[4] * 2.0 - fixed;
        let fit = if screen.expanded {
            if style.logo.is_some() && room - logo_space >= LOG_WITH_LOGO {
                Some((true, Some((room - logo_space).min(LOG_MAX))))
            } else if room >= LOG_MIN {
                Some((false, Some(room.min(LOG_MAX))))
            } else {
                None
            }
        } else if style.logo.is_some() && room >= logo_space {
            Some((true, None))
        } else if room >= 0.0 {
            Some((false, None))
        } else {
            None
        };
        let fit =
            fit.or_else(|| (rem <= 1.0).then_some((false, screen.expanded.then_some(LOG_MIN))));
        if let Some((show_logo, log)) = fit {
            // Whole log lines between the well's top and bottom spacers.
            let log = log.map(|h| {
                ((h - theme::SPACE[1] * 2.0) / LOG_LINE).floor() * LOG_LINE + theme::SPACE[1] * 2.0
            });
            let panel_h = (fixed + log.unwrap_or(0.0)) * rem;
            let group_h = panel_h + if show_logo { logo_space * rem } else { 0.0 };
            let top = ((height - group_h) * 0.5).max(0.0);
            let logo = style.logo.filter(|_| show_logo).map(|logo| {
                let w = (LOGO_WIDTH * rem).min(panel_w);
                Rect {
                    x: (width - w) * 0.5,
                    y: top,
                    w,
                    h: w * logo.height as f32 / logo.width.max(1) as f32,
                }
            });
            return Layout {
                rem,
                panel: Rect {
                    x: ((width - panel_w) * 0.5).round(),
                    y: (top + group_h - panel_h).round(),
                    w: panel_w.round(),
                    h: panel_h.round(),
                },
                logo,
                body,
                log,
            };
        }
        rem = (rem * 0.9).max(1.0);
    }
}

/// The summary card's height in rem: the facts and the report folder between one spacer step
/// above and below.
fn card_height(summary: &CrashSummary) -> f32 {
    theme::SPACE[1] * 2.0 + (summary.facts().len() + 1) as f32 * ROW_HEIGHT
}

/// Draws the crash window and returns its button targets and the details log geometry.
pub(super) fn draw(
    canvas: &mut Canvas,
    text: &mut Text,
    summary: &CrashSummary,
    details: &[DetailLine],
    discord: &mut DiscordMark,
    screen: &CrashScreen,
    style: &Style<'_>,
) -> Frame {
    let layout = layout(canvas, text, summary, screen, style);
    let Layout {
        rem, panel, logo, ..
    } = layout;
    let appearance = style.appearance;
    backdrop(canvas, appearance, style.logo.zip(logo));
    panel_surface(canvas, appearance, panel, rem);
    let inner = panel.inset(theme::LOADING_PAD * rem);
    let mut y = inner.y;

    header(canvas, text, appearance, &summary.title(), inner, y, rem);
    y += BADGE * rem + theme::SPACE[2] * rem;
    let body_px = theme::CAPTION.size * rem;
    let body_offset = (theme::CAPTION.line * rem - text.line_height(body_px)) * 0.5;
    for line in &layout.body {
        text.draw(
            canvas,
            inner.x,
            y + body_offset,
            body_px,
            appearance.ink(theme::TEXT_DIMMER),
            line,
        );
        y += theme::CAPTION.line * rem;
    }
    y += theme::SPACE[4] * rem;

    let card = Rect {
        x: inner.x,
        y,
        w: inner.w,
        h: card_height(summary) * rem,
    };
    summary_card(canvas, text, appearance, summary, card, rem);
    y += card.h + theme::SPACE[3] * rem;

    let mut hits = Vec::new();
    let toggle = details_toggle(canvas, text, screen, style, inner.x, y, rem);
    hits.push((CrashAction::ToggleDetails, toggle));
    let community = discord_button(
        canvas,
        text,
        discord,
        screen,
        style,
        (inner.x + inner.w, y),
        rem,
    );
    hits.push((CrashAction::Discord, community));
    y += toggle.h;

    let log = layout.log.map(|log_h| {
        y += theme::SPACE[2] * rem;
        let view = Rect {
            x: inner.x,
            y,
            w: inner.w,
            h: log_h * rem,
        };
        y += view.h;
        details_log(canvas, text, appearance, details, view, style, rem)
    });

    y += theme::SPACE[4] * rem;
    let gap = theme::SPACE[1] * rem;
    let button_w = (inner.w - gap * (ROW.len() - 1) as f32) / ROW.len() as f32;
    for (index, action) in ROW.into_iter().enumerate() {
        let x0 = (inner.x + index as f32 * (button_w + gap)).round();
        let x1 = (inner.x + index as f32 * (button_w + gap) + button_w).round();
        let rect = Rect {
            x: x0,
            y,
            w: x1 - x0,
            h: theme::BUTTON_HEIGHT * rem,
        };
        action_button(canvas, text, screen, style, action, rect, rem);
        hits.push((action, rect));
    }
    Frame { hits, log }
}

/// The destructive badge with a pixel-art exclamation mark, then `title` beside it.
fn header(
    canvas: &mut Canvas,
    text: &mut Text,
    appearance: Appearance,
    title: &str,
    inner: Rect,
    y: f32,
    rem: f32,
) {
    let badge = Rect {
        x: inner.x,
        y,
        w: BADGE * rem,
        h: BADGE * rem,
    };
    let face = raised_button(
        canvas,
        appearance,
        appearance.role(theme::DESTRUCTIVE),
        ButtonState::default(),
        badge,
        rem,
    );
    // A 2-pixel stroke: five pixels of bar, a one-pixel gap, then a two-pixel dot.
    let px = GUI_PIXEL * rem;
    let mark_x = (face.x + (face.w - 2.0 * px) * 0.5).round();
    let mark_y = (face.y + (face.h - 8.0 * px) * 0.5).round();
    let ink = theme::DESTRUCTIVE.text;
    canvas.fill(
        Rect {
            x: mark_x,
            y: mark_y,
            w: 2.0 * px,
            h: 5.0 * px,
        },
        ink,
    );
    canvas.fill(
        Rect {
            x: mark_x,
            y: mark_y + 6.0 * px,
            w: 2.0 * px,
            h: 2.0 * px,
        },
        ink,
    );
    let title_px = theme::HEADER5.size * rem;
    let title_y = face.y + (face.h - text.line_height(title_px)) * 0.5;
    text.draw(
        canvas,
        badge.x + badge.w + theme::SPACE[3] * rem,
        title_y,
        title_px,
        appearance.ink(theme::TEXT),
        title,
    );
}

/// An inset well: the given surface under a dark top-left and light bottom-right bevel.
fn well(canvas: &mut Canvas, appearance: Appearance, rect: Rect, surface: Rgba, rem: f32) {
    let edge = theme::EDGE * rem;
    canvas.fill(rect, appearance.surface(surface));
    canvas.frame(rect, edge, appearance.surface(theme::BORDER));
    let inner = rect.inset(edge);
    canvas.fill(Rect { h: edge, ..inner }, theme::BEVEL_DARK);
    canvas.fill(
        Rect {
            y: inner.y + edge,
            w: edge,
            h: inner.h - edge,
            ..inner
        },
        theme::BEVEL_DARK,
    );
}

/// The fact rows and the report folder: dim labels in one column, values aligned in the next,
/// split by hairlines. Long values end in an ellipsis; the folder keeps both ends.
fn summary_card(
    canvas: &mut Canvas,
    text: &mut Text,
    appearance: Appearance,
    summary: &CrashSummary,
    card: Rect,
    rem: f32,
) {
    well(canvas, appearance, card, theme::NEUTRAL90, rem);
    let px = theme::CAPTION.size * rem;
    let folder = summary.folder.display().to_string();
    let rows: Vec<(&str, &str, bool)> = summary
        .facts()
        .into_iter()
        .map(|(label, value)| (label, value, false))
        .chain([("Saved to", folder.as_str(), true)])
        .collect();
    let label_w = rows
        .iter()
        .map(|(label, ..)| text.width(label, px))
        .fold(0.0, f32::max);
    let side = theme::SPACE[3] * rem;
    let value_x = card.x + side + label_w + theme::SPACE[4] * rem;
    let value_w = card.x + card.w - side - value_x;
    let row_h = ROW_HEIGHT * rem;
    let offset = (row_h - text.line_height(px)) * 0.5;
    let mut y = card.y + theme::SPACE[1] * rem;
    for (index, (label, value, path)) in rows.into_iter().enumerate() {
        if index > 0 {
            canvas.fill(
                Rect {
                    x: card.x + side,
                    y: y.round(),
                    w: card.w - side * 2.0,
                    h: theme::EDGE * rem,
                },
                appearance.surface(theme::NEUTRAL80.fill),
            );
        }
        text.draw(
            canvas,
            card.x + side,
            y + offset,
            px,
            appearance.ink(theme::TEXT_DIMMEST),
            label,
        );
        let (value, ink) = if path {
            (text.truncate_middle(value, px, value_w), theme::TEXT_DIMMER)
        } else {
            (text.truncate_end(value, px, value_w), theme::TEXT)
        };
        text.draw(canvas, value_x, y + offset, px, appearance.ink(ink), &value);
        y += row_h;
    }
}

/// The secondary Show/Hide details button with its pixel-art chevron.
fn details_toggle(
    canvas: &mut Canvas,
    text: &mut Text,
    screen: &CrashScreen,
    style: &Style<'_>,
    x: f32,
    y: f32,
    rem: f32,
) -> Rect {
    let action = CrashAction::ToggleDetails;
    let label = screen.label(action);
    let px = theme::SECONDARY_BUTTON.size * rem;
    let chevron = 5.0 * GUI_PIXEL * rem;
    let gap = theme::SPACE[1] * rem;
    let content = chevron + gap + text.width(label, px);
    let rect = Rect {
        x,
        y,
        w: (content + theme::SPACE[4] * 2.0 * rem).round(),
        h: theme::BUTTON_HEIGHT * rem,
    };
    let role = style.appearance.role(theme::MENU_SECONDARY);
    let face = raised_button(
        canvas,
        style.appearance,
        role,
        button_state(style, action),
        rect,
        rem,
    );
    let start = face.x + (face.w - content) * 0.5;
    let pixel = GUI_PIXEL * rem;
    let mid = face.y + face.h * 0.5;
    // Three stepped rows (or columns) of 5, 3 and 1 pixels: a solid triangle.
    for step in 0..3 {
        let step_f = step as f32;
        let rect = if screen.expanded {
            Rect {
                x: (start + step_f * pixel).round(),
                y: (mid - 1.5 * pixel + step_f * pixel).round(),
                w: ((5.0 - 2.0 * step_f) * pixel).round(),
                h: pixel.round(),
            }
        } else {
            Rect {
                x: (start + pixel + step_f * pixel).round(),
                y: (mid - 2.5 * pixel + step_f * pixel).round(),
                w: pixel.round(),
                h: ((5.0 - 2.0 * step_f) * pixel).round(),
            }
        };
        canvas.fill(rect, role.text);
    }
    let label_y = face.y + (face.h - text.line_height(px)) * 0.5;
    text.draw(canvas, start + chevron + gap, label_y, px, role.text, label);
    rect
}

/// The community button with its top-right corner at `top_right`: Discord's blurple with the
/// OreUI raised treatment and the white mark before its label.
fn discord_button(
    canvas: &mut Canvas,
    text: &mut Text,
    discord: &mut DiscordMark,
    screen: &CrashScreen,
    style: &Style<'_>,
    top_right: (f32, f32),
    rem: f32,
) -> Rect {
    let (right, y) = top_right;
    let action = CrashAction::Discord;
    let label = screen.label(action);
    let px = theme::SECONDARY_BUTTON.size * rem;
    let icon = (DISCORD_ICON * rem).round();
    let gap = theme::SPACE[1] * rem;
    let content = icon + gap + text.width(label, px);
    let w = (content + theme::SPACE[4] * 2.0 * rem).round();
    let rect = Rect {
        x: (right - w).round(),
        y,
        w,
        h: theme::BUTTON_HEIGHT * rem,
    };
    let face = raised_button(
        canvas,
        style.appearance,
        DISCORD_ROLE,
        button_state(style, action),
        rect,
        rem,
    );
    let x = face.x + (face.w - content) * 0.5;
    if let Some(mark) = discord.image(icon as u32) {
        canvas.image(
            mark,
            Rect {
                x: x.round(),
                y: (face.y + (face.h - icon) * 0.5).round(),
                w: icon,
                h: icon,
            },
        );
    }
    let label_y = face.y + (face.h - text.line_height(px)) * 0.5;
    text.draw(
        canvas,
        x + icon + gap,
        label_y,
        px,
        DISCORD_ROLE.text,
        label,
    );
    rect
}

/// The scrollable details log: wrapped, toned lines clipped to a dark well, with a scrollbar.
fn details_log(
    canvas: &mut Canvas,
    text: &mut Text,
    appearance: Appearance,
    details: &[DetailLine],
    view: Rect,
    style: &Style<'_>,
    rem: f32,
) -> LogFrame {
    well(canvas, appearance, view, theme::NEUTRAL100, rem);
    let edge = theme::EDGE * rem;
    let px = LOG_TEXT * rem;
    let line = LOG_LINE * rem;
    let pad = theme::SPACE[2] * rem;
    let bar = SCROLLBAR * rem;
    let content = Rect {
        x: view.x + pad,
        y: view.y + theme::SPACE[1] * rem,
        w: view.w - pad * 2.0 - bar - edge,
        h: view.h - theme::SPACE[1] * 2.0 * rem,
    };
    let rows = log_rows(text, appearance, details, px, content.w, pad);
    let visible = (content.h / line).round();
    let max_scroll = (rows.len() as f32 - visible).max(0.0);
    let scroll = style.scroll.clamp(0.0, max_scroll);
    // Rest on whole lines so no row is cut at the well's edges.
    let shown = scroll.round();
    let offset = (line - text.line_height(px)) * 0.5;
    canvas.set_clip(Some(Rect {
        x: view.x + edge,
        y: view.y + edge,
        w: content.w + pad,
        h: view.h - edge * 2.0,
    }));
    for (index, row) in rows
        .iter()
        .enumerate()
        .skip(shown as usize)
        .take(visible as usize)
    {
        let y = (content.y + (index as f32 - shown) * line + offset).round();
        for (x, ink, cell) in row {
            text.draw(canvas, content.x + x, y, px, *ink, cell);
        }
    }
    canvas.set_clip(None);

    let track = Rect {
        x: view.x + view.w - edge - bar - edge,
        y: view.y + edge * 2.0,
        w: bar,
        h: view.h - edge * 4.0,
    };
    canvas.fill(track, appearance.surface(theme::NEUTRAL90));
    let thumb_h = if rows.is_empty() {
        track.h
    } else {
        (track.h * (visible / rows.len() as f32).min(1.0)).max(THUMB_MIN * rem)
    };
    let travel = track.h - thumb_h;
    let thumb = Rect {
        y: (track.y
            + if max_scroll > 0.0 {
                travel * shown / max_scroll
            } else {
                0.0
            })
        .round(),
        h: thumb_h.round(),
        ..track
    };
    let role = appearance.role(theme::NEUTRAL);
    canvas.fill(
        thumb,
        if style.thumb_dragged {
            role.pressed
        } else if style.thumb_hovered {
            role.hovered
        } else {
            role.fill
        },
    );
    canvas.fill(Rect { h: edge, ..thumb }, role.specular[0]);
    LogFrame {
        track,
        thumb,
        line,
        visible,
        max_scroll,
        scroll,
    }
}

/// One drawn log row: cells at their x offsets with their ink.
type LogRow = Vec<(f32, Rgba, String)>;

/// Lays the details out as rows. Cells split on runs of two or more spaces line up in columns
/// within each section (a heading starts one), timestamps are dimmed, and each line's last cell
/// wraps under itself.
fn log_rows(
    text: &mut Text,
    appearance: Appearance,
    details: &[DetailLine],
    px: f32,
    width: f32,
    gap: f32,
) -> Vec<LogRow> {
    let mut rows = Vec::new();
    for section in details.chunk_by(|_, next| next.tone != Tone::Heading) {
        let cells: Vec<Vec<&str>> = section.iter().map(|line| cells(&line.text)).collect();
        let mut widths: Vec<f32> = Vec::new();
        for line in &cells {
            for (column, cell) in line.iter().enumerate().take(line.len().saturating_sub(1)) {
                let cell_w = text.width(cell, px);
                match widths.get_mut(column) {
                    Some(width) => *width = width.max(cell_w),
                    None => widths.push(cell_w),
                }
            }
        }
        let columns: Vec<f32> = std::iter::once(0.0)
            .chain(widths.iter().scan(0.0, |x, width| {
                *x += width + gap;
                Some(*x)
            }))
            .collect();
        for (line, cells) in section.iter().zip(cells) {
            let ink = match line.tone {
                Tone::Heading => theme::INFORMATIVE_TINT,
                Tone::Plain => appearance.ink(theme::TEXT_DIMMER),
                Tone::Warning => appearance.ink(theme::TEXT),
                Tone::Error => theme::DESTRUCTIVE_TINT,
            };
            let Some((last, leading)) = cells.split_last() else {
                rows.push(Vec::new());
                continue;
            };
            let mut row: LogRow = leading
                .iter()
                .enumerate()
                .map(|(column, cell)| {
                    let ink = if column == 0 {
                        appearance.ink(theme::TEXT_DIMMEST)
                    } else {
                        ink
                    };
                    (columns[column], ink, (*cell).to_owned())
                })
                .collect();
            let x = columns[leading.len()];
            for (index, part) in text.wrap(last, px, width - x).into_iter().enumerate() {
                if index > 0 {
                    rows.push(std::mem::take(&mut row));
                }
                row.push((x, ink, part));
            }
            rows.push(row);
        }
    }
    rows
}

/// A details line's cells: the text between runs of two or more spaces.
fn cells(line: &str) -> Vec<&str> {
    line.split("  ")
        .map(str::trim)
        .filter(|cell| !cell.is_empty())
        .collect()
}

fn button_state(style: &Style<'_>, action: CrashAction) -> ButtonState {
    ButtonState {
        hovered: style.hovered == Some(action),
        focused: style.focused == Some(action),
        pressed: style.pressed == Some(action),
    }
}

/// One action-row button; Restart is primary, and Copy report draws a check once copied.
fn action_button(
    canvas: &mut Canvas,
    text: &mut Text,
    screen: &CrashScreen,
    style: &Style<'_>,
    action: CrashAction,
    rect: Rect,
    rem: f32,
) {
    let role = style.appearance.role(if action == CrashAction::Restart {
        theme::PRIMARY_ROLE
    } else {
        theme::MENU_NEUTRAL
    });
    let face = raised_button(
        canvas,
        style.appearance,
        role,
        button_state(style, action),
        rect,
        rem,
    );
    let label = screen.label(action);
    let copied =
        action == CrashAction::CopyReport && matches!(screen.copied, Some((Copied::Done, _)));
    if !copied {
        button_label(canvas, text, face, rem, role.text, label);
        return;
    }
    // A pixel-art check (7 by 5 pixels) in the success tint before the label.
    let px = theme::SECONDARY_BUTTON.size * rem;
    let pixel = GUI_PIXEL * rem;
    let gap = theme::SPACE[1] * rem;
    let content = 7.0 * pixel + gap + text.width(label, px);
    let x = face.x + (face.w - content) * 0.5;
    let top = face.y + (face.h - 5.0 * pixel) * 0.5;
    for (column, row) in [(0, 2), (1, 3), (2, 4), (3, 3), (4, 2), (5, 1), (6, 0)] {
        canvas.fill(
            Rect {
                x: (x + column as f32 * pixel).round(),
                y: (top + row as f32 * pixel).round(),
                w: pixel.round(),
                h: (2.0 * pixel).round().min((5.0 - row as f32) * pixel),
            },
            theme::SUCCESS_TINT,
        );
    }
    let label_y = face.y + (face.h - text.line_height(px)) * 0.5;
    text.draw(canvas, x + 7.0 * pixel + gap, label_y, px, role.text, label);
}

#[cfg(test)]
mod tests;
