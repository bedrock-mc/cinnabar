//! Lays out and paints one data-driven form: panel, header and the control column.

use ui::UiRect;
use ui_engine::engine::{rect, screen::PaintError};

use super::super::icons::{self, Icon};
use super::super::motion::Kind;
use super::super::paint::{Bounds, Canvas};
use super::super::settings::controls::{slider_face, slider_travel, toggle_at};
use super::super::settings::picker::select_face;
use super::super::theme::{
    BEVEL_DARK, BEVEL_LIGHT, BODY, BORDER, CAPTION, EDGE, FIELD_CARET, HEADER5, NEUTRAL, NEUTRAL80,
    OUTLINE, SECONDARY, TEXT, TEXT_DIMMER, TEXT_DIMMEST,
};
use super::super::transitions::switch_target;
use super::super::widgets::{Interaction, Variant, button_face, divider};
use super::{DataDrivenForm, FormEntry, FormHit, FormItem};

const PANEL_WIDTH: f32 = 36.0;
const PANEL_MIN_HEIGHT: f32 = 48.0;
/// The space kept between the panel and the screen edges.
const MARGIN: f32 = 2.4;
const HEADER: f32 = 4.8;
const CLOSE: f32 = 4.0;
const TITLE_INSET: f32 = 1.6;
const SIDE: f32 = 2.4;
const PAD: f32 = 1.6;
/// The gap between consecutive controls; a divider adds it again on either side.
const GAP: f32 = 0.8;
const LABEL_GAP: f32 = 0.8;
const BUTTON: f32 = 4.4;
const FIELD: f32 = 4.8;
const SELECT: f32 = 4.6;
const SLIDER: f32 = 3.2;
const SPACER: f32 = 1.6;
/// One open dropdown option row.
const OPTION: f32 = 4.0;
/// The switch's drawn width, from its rail inset to its far edge.
const SWITCH_WIDTH: f32 = 6.4;

/// The scroll view key of an open dropdown's option list.
pub(super) const OPTIONS_KEY: &str = "ddui_options";

/// What one paint leaves for input: press targets (topmost first), each slider's thumb travel
/// by item, and the open option list's viewport.
#[derive(Default)]
pub(super) struct Drawn {
    pub(super) hits: Vec<(FormHit, UiRect)>,
    pub(super) travels: Vec<(usize, [f32; 2])>,
    pub(super) options: Option<Bounds>,
}

/// Interaction state the controls read this frame.
pub(super) struct State {
    pub(super) hovered: Option<FormHit>,
    pub(super) pressed: Option<FormHit>,
    pub(super) open: Option<usize>,
    pub(super) editing: Option<usize>,
}

impl State {
    fn of(&self, hit: FormHit) -> Interaction {
        Interaction {
            action: None,
            hovered: self.hovered == Some(hit),
            pressed: self.pressed == Some(hit),
            focused: false,
        }
    }
}

/// Paints `form` centred in `size` and returns its press targets, clipped to what shows.
pub(super) fn form(
    canvas: &mut Canvas<'_>,
    form: &DataDrivenForm,
    state: &State,
    size: [f32; 2],
    scroll_key: &str,
) -> Result<Drawn, PaintError> {
    let width = canvas.r(PANEL_WIDTH).min(size[0] - canvas.r(MARGIN) * 2.0);
    let inner = width - canvas.r(SIDE) * 2.0;
    let content = column_height(canvas, &form.items, inner)?;
    let header = canvas.r(HEADER);
    let max_height = (size[1] - canvas.r(MARGIN) * 2.0).max(header);
    let height = (header + content)
        .max(canvas.r(PANEL_MIN_HEIGHT))
        .min(max_height);
    let left = ((size[0] - width) * 0.5).round();
    let top = ((size[1] - height) * 0.5).round();
    let panel = [left, top, left + width, top + height];
    let mut drawn = Drawn::default();

    canvas.fill(panel, BORDER)?;
    let edge = canvas.r(EDGE);
    let face = [
        panel[0] + edge,
        panel[1] + edge,
        panel[2] - edge,
        panel[3] - edge,
    ];
    canvas.fill(face, NEUTRAL.fill)?;
    canvas.specular(face, BEVEL_LIGHT, BEVEL_DARK)?;

    let header_bottom = face[1] + header - edge;
    let title_right = if form.close.is_some() {
        face[2] - canvas.r(CLOSE)
    } else {
        face[2] - canvas.r(TITLE_INSET)
    };
    canvas.text_line(
        &form.title,
        [
            face[0] + canvas.r(TITLE_INSET),
            (face[1] + header_bottom - canvas.r(HEADER5.line)) * 0.5,
        ],
        (title_right - face[0] - canvas.r(TITLE_INSET)).max(1.0),
        HEADER5,
        TEXT,
    )?;
    if let Some(enabled) = form.close {
        let inset = canvas.r(0.4);
        let button = [
            face[2] - canvas.r(CLOSE) - inset * 0.5,
            face[1] + inset * 0.5,
            face[2] - inset * 0.5,
            header_bottom - inset * 0.5,
        ];
        close_button(canvas, button, state.of(FormHit::Close), enabled)?;
        if enabled {
            push(&mut drawn.hits, FormHit::Close, button, None)?;
        }
    }
    divider(canvas, face[0], face[2], header_bottom)?;

    let viewport = [face[0], header_bottom + edge, face[2], face[3]];
    let scroll = canvas.begin_scroll(scroll_key, viewport)?;
    let mut y = viewport[1] - scroll.offset + canvas.r(PAD);
    let x = [panel[0] + canvas.r(SIDE), panel[2] - canvas.r(SIDE)];
    let mut open = None;
    for (position, FormEntry { key, item }) in form.items.iter().enumerate() {
        if position > 0 {
            y += canvas.r(GAP);
        }
        let hit = FormHit::Item(*key);
        let mut item_state = state.of(hit);
        item_state.focused = state.editing == Some(*key);
        let (height, target) = item_at(canvas, item, x, [face[0], face[2]], y, item_state)?;
        if let (Some(target), FormItem::Slider { .. }) = (target, item) {
            drawn.travels.push((*key, slider_travel(canvas, target)));
        }
        if let Some(target) = target {
            if state.open == Some(*key)
                && let FormItem::Dropdown {
                    options, selected, ..
                } = item
            {
                open = Some((target, options, *selected));
            }
            push(&mut drawn.hits, hit, target, Some(viewport))?;
        }
        y += height;
    }
    canvas.end_scroll(scroll, content)?;
    if let Some((anchor, options, selected)) = open {
        let (mut list, viewport) = option_list(canvas, state, anchor, options, selected, size)?;
        // The open list lies over the column, so its options take presses first.
        list.append(&mut drawn.hits);
        drawn.hits = list;
        drawn.options = Some(viewport);
    }
    Ok(drawn)
}

/// Draws an open dropdown's options in a bordered list under its face, or above it when more
/// room lies there. A list taller than that room scrolls; returns the option targets and the
/// list's viewport.
fn option_list(
    canvas: &mut Canvas<'_>,
    state: &State,
    anchor: Bounds,
    options: &[String],
    selected: Option<usize>,
    size: [f32; 2],
) -> Result<(Vec<(FormHit, UiRect)>, Bounds), PaintError> {
    let row = canvas.r(OPTION);
    let edge = canvas.r(EDGE);
    let margin = canvas.r(MARGIN);
    let content = row * options.len() as f32;
    let below = size[1] - margin - anchor[3];
    let above = anchor[1] - margin;
    let room = if content + edge * 2.0 <= below || below >= above {
        below
    } else {
        above
    };
    let height = (content + edge * 2.0).min(room.max(row + edge * 2.0));
    let top = if room == below {
        anchor[3]
    } else {
        anchor[1] - height
    };
    let list = [anchor[0], top, anchor[2], top + height];
    canvas.fill(list, BORDER)?;
    let viewport = [
        list[0] + edge,
        list[1] + edge,
        list[2] - edge,
        list[3] - edge,
    ];
    let scroll = canvas.begin_scroll(OPTIONS_KEY, viewport)?;
    let role = canvas.role(SECONDARY);
    let mut hits = Vec::with_capacity(options.len());
    for (index, label) in options.iter().enumerate() {
        let y = viewport[1] - scroll.offset + row * index as f32;
        if y + row < viewport[1] || y > viewport[3] {
            continue;
        }
        let b = [viewport[0], y, viewport[2], y + row];
        let hit = FormHit::Option(index);
        let motion = canvas.feedback(state.of(hit), true, selected == Some(index), Kind::Surface);
        let fill = if selected == Some(index) {
            role.pressed
        } else {
            motion.color(role.fill, role.hovered, role.pressed)
        };
        canvas.fill(b, fill)?;
        canvas.text_line(
            label,
            [
                b[0] + canvas.r(1.6),
                (b[1] + b[3] - canvas.r(BODY.line)) * 0.5,
            ],
            (b[2] - b[0] - canvas.r(3.2)).max(1.0),
            BODY,
            role.text,
        )?;
        push(&mut hits, hit, b, Some(viewport))?;
    }
    canvas.end_scroll(scroll, content)?;
    Ok((hits, viewport))
}

/// Records a target, trimmed to `clip` when it scrolls.
fn push(
    hits: &mut Vec<(FormHit, UiRect)>,
    hit: FormHit,
    b: Bounds,
    clip: Option<Bounds>,
) -> Result<(), PaintError> {
    let b = clip.map_or(b, |clip| {
        [
            b[0].max(clip[0]),
            b[1].max(clip[1]),
            b[2].min(clip[2]),
            b[3].min(clip[3]),
        ]
    });
    if b[2] > b[0] && b[3] > b[1] {
        hits.push((hit, rect(b[0], b[1], b[2], b[3])?));
    }
    Ok(())
}

/// The scrolling column's full height, with its top and bottom padding.
fn column_height(
    canvas: &mut Canvas<'_>,
    items: &[FormEntry],
    width: f32,
) -> Result<f32, PaintError> {
    let mut height = canvas.r(PAD) * 2.0;
    for (index, FormEntry { item, .. }) in items.iter().enumerate() {
        if index > 0 {
            height += canvas.r(GAP);
        }
        height += item_height(canvas, item, width)?;
    }
    Ok(height)
}

/// A labelled control's caption block: label, then its description in the dimmer caption type.
fn caption_height(
    canvas: &mut Canvas<'_>,
    label: &str,
    description: Option<&str>,
    width: f32,
) -> Result<f32, PaintError> {
    let mut height = text_height(canvas, label, width, BODY)?;
    if let Some(description) = description.filter(|text| !text.is_empty()) {
        height += text_height(canvas, description, width, CAPTION)?;
    }
    Ok(height)
}

fn text_height(
    canvas: &mut Canvas<'_>,
    text: &str,
    width: f32,
    style: super::super::theme::Type,
) -> Result<f32, PaintError> {
    if text.is_empty() {
        return Ok(0.0);
    }
    Ok(canvas
        .measure_height(text, width, style)?
        .max(canvas.r(style.line)))
}

fn item_height(canvas: &mut Canvas<'_>, item: &FormItem, width: f32) -> Result<f32, PaintError> {
    Ok(match item {
        FormItem::Header(text) => text_height(canvas, text, width, HEADER5)?,
        FormItem::Label(text) => text_height(canvas, text, width, BODY)?,
        FormItem::Divider => canvas.r(EDGE + GAP * 2.0),
        FormItem::Spacer => canvas.r(SPACER),
        FormItem::Button { .. } => canvas.r(BUTTON),
        FormItem::Toggle {
            label, description, ..
        } => caption_height(
            canvas,
            label,
            description.as_deref(),
            width - canvas.r(SWITCH_WIDTH + GAP),
        )?
        .max(canvas.r(SLIDER)),
        FormItem::Slider {
            label,
            description,
            value,
            ..
        } => {
            let caption_width = slider_caption_width(canvas, value, width)?;
            caption_height(canvas, label, description.as_deref(), caption_width)?
                + canvas.r(LABEL_GAP + SLIDER)
        }
        FormItem::Dropdown {
            label, description, ..
        } => {
            caption_height(canvas, label, description.as_deref(), width)?
                + canvas.r(LABEL_GAP + SELECT)
        }
        FormItem::TextField {
            label, description, ..
        } => {
            caption_height(canvas, label, description.as_deref(), width)?
                + canvas.r(LABEL_GAP + FIELD)
        }
    })
}

/// A slider caption's width: the column less the value shown beside it.
fn slider_caption_width(
    canvas: &mut Canvas<'_>,
    value: &str,
    width: f32,
) -> Result<f32, PaintError> {
    Ok((width - canvas.measure(value, BODY)? - canvas.r(GAP)).max(1.0))
}

/// Draws a label and description from `y`; returns the block height.
fn caption(
    canvas: &mut Canvas<'_>,
    label: &str,
    description: Option<&str>,
    x: f32,
    y: f32,
    width: f32,
) -> Result<f32, PaintError> {
    let mut height = text_height(canvas, label, width, BODY)?;
    canvas.text(label, [x, y], width, BODY, TEXT, false)?;
    if let Some(description) = description.filter(|text| !text.is_empty()) {
        canvas.text(
            description,
            [x, y + height],
            width,
            CAPTION,
            TEXT_DIMMER,
            false,
        )?;
        height += text_height(canvas, description, width, CAPTION)?;
    }
    Ok(height)
}

/// Draws one control at `y` in the column `x`; returns its height and press target.
fn item_at(
    canvas: &mut Canvas<'_>,
    item: &FormItem,
    x: [f32; 2],
    panel: [f32; 2],
    y: f32,
    state: Interaction,
) -> Result<(f32, Option<Bounds>), PaintError> {
    let width = x[1] - x[0];
    let height = item_height(canvas, item, width)?;
    let target = match item {
        FormItem::Header(text) => {
            canvas.text(text, [x[0], y], width, HEADER5, TEXT, false)?;
            None
        }
        FormItem::Label(text) => {
            canvas.text(text, [x[0], y], width, BODY, TEXT, false)?;
            None
        }
        FormItem::Divider => {
            divider(canvas, panel[0], panel[1], y + canvas.r(GAP))?;
            None
        }
        FormItem::Spacer => None,
        FormItem::Button { label, enabled } => {
            let b = [x[0], y, x[1], y + height];
            button_face(canvas, b, Variant::Secondary, label, state, *enabled)?;
            (*enabled).then_some(b)
        }
        FormItem::Toggle {
            label,
            description,
            on,
            enabled,
        } => {
            let switch = canvas.r(SWITCH_WIDTH);
            caption(
                canvas,
                label,
                description.as_deref(),
                x[0],
                y,
                width - switch - canvas.r(GAP),
            )?;
            let at = [x[1] - switch - canvas.r(0.2), y];
            toggle_at(canvas, at, switch_target(*on), state, *enabled)?;
            (*enabled).then_some([x[0], y, x[1], y + height])
        }
        FormItem::Slider {
            label,
            description,
            fraction,
            value,
            enabled,
        } => {
            let value_width = canvas.measure(value, BODY)?;
            let caption_width = slider_caption_width(canvas, value, width)?;
            caption(
                canvas,
                label,
                description.as_deref(),
                x[0],
                y,
                caption_width,
            )?;
            canvas.text_line(
                value,
                [x[1] - value_width, y],
                value_width + 1.0,
                BODY,
                TEXT,
            )?;
            let track = [x[0], y + height - canvas.r(SLIDER), x[1], y + height];
            // A non-finite fraction would make the thumb's geometry invalid.
            let fraction = if fraction.is_finite() {
                fraction.clamp(0.0, 1.0)
            } else {
                0.0
            };
            slider_face(canvas, track, fraction, 0, state, *enabled)?;
            (*enabled).then_some(track)
        }
        FormItem::Dropdown {
            label,
            description,
            options,
            selected,
            enabled,
        } => {
            caption(canvas, label, description.as_deref(), x[0], y, width)?;
            let b = [x[0], y + height - canvas.r(SELECT), x[1], y + height];
            let shown = selected
                .and_then(|selected| options.get(selected))
                .map_or("", String::as_str);
            select_face(canvas, b, shown, state)?;
            (*enabled).then_some(b)
        }
        FormItem::TextField {
            label,
            description,
            text,
            enabled,
        } => {
            caption(canvas, label, description.as_deref(), x[0], y, width)?;
            let b = [x[0], y + height - canvas.r(FIELD), x[1], y + height];
            field(canvas, b, text, state)?;
            (*enabled).then_some(b)
        }
    };
    Ok((height, target))
}

/// A dark input face showing `text` on one line; a focused field shows its caret after the
/// text and keeps that end in view.
fn field(
    canvas: &mut Canvas<'_>,
    b: Bounds,
    text: &str,
    state: Interaction,
) -> Result<(), PaintError> {
    let motion = canvas.feedback(state, true, false, Kind::Field);
    canvas.fill(b, BORDER)?;
    let edge = canvas.r(EDGE);
    let face = [b[0] + edge, b[1] + edge, b[2] - edge, b[3] - edge];
    let role = canvas.role(NEUTRAL80);
    canvas.fill(face, motion.color(role.fill, role.hovered, role.pressed))?;
    let pad = canvas.r(1.4);
    let viewport = [face[0] + pad, face[1], face[2] - pad, face[3]];
    let top = (face[1] + face[3] - canvas.r(BODY.line)) * 0.5;
    let measured = canvas.measure(text, BODY)?;
    let room = viewport[2] - viewport[0] - edge;
    let left = if state.focused && measured > room {
        viewport[0] + room - measured
    } else {
        viewport[0]
    };
    let clip = canvas.begin_clip(viewport)?;
    canvas.text_line(text, [left, top], measured.max(room) + 1.0, BODY, TEXT)?;
    if state.focused {
        let caret = (left + measured).min(viewport[2] - edge);
        canvas.fill(
            [caret, top, caret + edge, top + canvas.r(BODY.line)],
            FIELD_CARET,
        )?;
    }
    canvas.end_clip(clip);
    if state.focused {
        canvas.frame(b, EDGE, OUTLINE)?;
    }
    Ok(())
}

/// The header's X: a flat square that lightens under the pointer, dimmed when disabled.
fn close_button(
    canvas: &mut Canvas<'_>,
    b: Bounds,
    state: Interaction,
    enabled: bool,
) -> Result<(), PaintError> {
    let motion = canvas.feedback(state, enabled, false, Kind::Surface);
    let role = canvas.role(NEUTRAL);
    if enabled {
        canvas.fill(b, motion.color([0; 4], role.hovered, role.pressed))?;
    }
    let ink = if enabled { TEXT } else { TEXT_DIMMEST };
    icons::draw_centred(canvas, Icon::Cross, b, ink)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui_engine::test_support::fixture_font;
    use ui_engine::text_metrics::TextMetrics;

    #[test]
    fn slider_height_measures_the_caption_beside_its_value() {
        let (mut nodes, mut next) = (Vec::new(), 1);
        let mut layouts = ui::TextLayoutCache::new(16, 65536);
        let font = fixture_font();
        let metrics =
            TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
        let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
        let width = canvas.r(30.0);
        let slider = |value: &str| FormItem::Slider {
            label: "A slider label long enough to wrap onto more lines when narrow".repeat(3),
            description: None,
            fraction: 0.5,
            value: value.to_owned(),
            enabled: true,
        };
        let short = item_height(&mut canvas, &slider("1"), width).unwrap();
        let wide = item_height(&mut canvas, &slider(&"8".repeat(40)), width).unwrap();
        assert!(
            wide > short,
            "a wide value narrows the caption, which wraps taller"
        );
    }
}
