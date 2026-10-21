//! The Shader Pack Settings screen in the Shaders tab: the pack's option grid, a fixed footer
//! describing the hovered or focused element, and Back, Reset, Cancel, Apply and Done.
//!
//! Cells follow the pack's columns while each stays readable, and fill rows in reading order
//! as in Iris. Switches toggle; value options and the profile step with arrows on either
//! side, and sliders also take a press on any step. A label turns gold while its value differs
//! from the one the pack runs with.

use super::super::paint::{Bounds, Canvas};
use super::super::theme::{self, BODY, CAPTION, EDGE, NEUTRAL, SECTION_HEADER, TEXT, TEXT_DIMMER};
use super::super::widgets;
use super::{Content, button, controls};
use launcher::menu::{MenuAction, MenuView};
use launcher::shader_options::{Action, Control, Element, Input, Status, View};
use ui_engine::engine::screen::PaintError;

#[cfg(test)]
mod tests;

type Rgba = [u8; 4];

/// Iris's colour for the label of a changed option.
const MODIFIED: Rgba = [255, 201, 74, 255];
const CELL_HEIGHT: f32 = 8.4;
const SWITCH_HEIGHT: f32 = 5.2;
/// A row of blank cells only spaces the rows around it.
const EMPTY_HEIGHT: f32 = 1.6;
const GAP: f32 = 0.8;
const PAD: f32 = 1.2;
/// Narrowest cell before the grid drops a column.
const MIN_CELL_WIDTH: f32 = 20.0;
const ARROW_WIDTH: f32 = 4.0;
const CONTROL_HEIGHT: f32 = 3.6;
const SWITCH_WIDTH: f32 = 6.0;
/// Caption lines the footer keeps for a comment.
const COMMENT_LINES: f32 = 3.0;

/// The action pressing element `index` with `input` sends.
fn element_action(view: &View, index: usize, input: Input) -> MenuAction {
    MenuAction::ShaderOptions(Action::Element {
        screen: view.screen,
        index: u16::try_from(index).unwrap_or(u16::MAX),
        input,
    })
}

/// Whether the pointer or visible focus is on one of `actions`.
fn engaged(view: &MenuView, actions: &[MenuAction]) -> bool {
    actions.iter().any(|action| {
        view.hovered == Some(*action)
            || (view.navigation_focus_visible && view.focused_action == Some(*action))
    })
}

/// Draws the title and grid at `content.y`; returns the described element and its index.
pub(super) fn body<'v>(
    content: &mut Content<'_, '_>,
    options: &'v View,
) -> Result<Option<(usize, &'v Element)>, PaintError> {
    let [left, right] = content.inset();
    let width = (right - left).max(1.0);
    let mut top = content.y + content.canvas.r(2.4);
    top += content
        .canvas
        .text(
            &options.title,
            [left, top],
            width,
            SECTION_HEADER,
            TEXT,
            false,
        )?
        .max(content.canvas.r(SECTION_HEADER.line));
    let status = match &options.status {
        Status::Saving => Some("Saving changes…".to_owned()),
        Status::SaveFailed(reason) => Some(format!("Could not save options: {reason}. Try again.")),
        Status::Loading => Some(format!("Reading {}…", options.pack)),
        Status::Failed(reason) => Some(format!("{} has no usable options: {reason}", options.pack)),
        Status::Ready if options.elements.is_empty() => {
            Some(format!("{} has no options.", options.pack))
        }
        Status::Ready => None,
    };
    if let Some(status) = status {
        top += content
            .canvas
            .text(&status, [left, top], width, CAPTION, TEXT_DIMMER, false)?
            .max(content.canvas.r(CAPTION.line));
        if !matches!(options.status, Status::Saving | Status::SaveFailed(_)) {
            content.y = top + content.canvas.r(2.4);
            return Ok(None);
        }
    }
    top += content.canvas.r(1.2);
    let gap = content.canvas.r(GAP);
    let fit = ((width + gap) / (content.canvas.r(MIN_CELL_WIDTH) + gap)).floor() as usize;
    let columns = usize::from(options.columns).min(fit).max(1);
    let cell_width = (width - gap * (columns - 1) as f32) / columns as f32;
    let mut described = None;
    let mut y = top;
    for (row, cells) in options.elements.chunks(columns).enumerate() {
        // A row is as tall as its tallest control; a row of blanks keeps only a spacer.
        let height = cells
            .iter()
            .map(|element| content.canvas.r(cell_height(&element.control)))
            .fold(0.0, f32::max);
        for (column, element) in cells.iter().enumerate() {
            let index = row * columns + column;
            let x = left + column as f32 * (cell_width + gap);
            if cell(
                content.canvas,
                content.view,
                options,
                index,
                element,
                [x, y, x + cell_width, y + height],
            )? {
                described = Some((index, element));
            }
        }
        y += height + gap;
    }
    content.y = y + content.canvas.r(1.6);
    Ok(described)
}

/// The height, in rem, a control needs.
fn cell_height(control: &Control) -> f32 {
    match control {
        Control::Empty => EMPTY_HEIGHT,
        Control::Link | Control::Switch { .. } | Control::Fixed { .. } => SWITCH_HEIGHT,
        Control::Choice { .. } | Control::Profile { .. } | Control::Slider { .. } => CELL_HEIGHT,
    }
}

/// Draws one cell; returns whether it is hovered or focused.
fn cell(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    options: &View,
    index: usize,
    element: &Element,
    b: Bounds,
) -> Result<bool, PaintError> {
    let next = element_action(options, index, Input::Next);
    let previous = element_action(options, index, Input::Previous);
    let label_color = if element.modified { MODIFIED } else { TEXT };
    let pad = canvas.r(PAD);
    let inner = [b[0] + pad, b[1] + pad, b[2] - pad, b[3] - pad];
    match &element.control {
        Control::Empty => Ok(false),
        Control::Link => {
            let face = [b[0], b[1], b[2], b[1] + canvas.r(button::ACTION_HEIGHT)];
            let face = centred(face, b);
            button::action(
                canvas,
                view,
                face,
                &format!("{} >", element.label),
                Some(next),
            )?;
            Ok(engaged(view, &[next]))
        }
        Control::Switch { on } => {
            let face = centred([b[0], b[1], b[2], b[1] + canvas.r(SWITCH_HEIGHT)], b);
            panel(canvas, face)?;
            let width = (inner[2] - inner[0] - canvas.r(SWITCH_WIDTH + 1.2)).max(1.0);
            canvas.text_line(
                &element.label,
                [inner[0], (face[1] + face[3] - canvas.r(BODY.line)) * 0.5],
                width,
                BODY,
                label_color,
            )?;
            if engaged(view, &[next]) {
                canvas.frame(face, EDGE, theme::OUTLINE)?;
            }
            controls::toggle_control(
                canvas,
                view,
                [
                    inner[2] - canvas.r(SWITCH_WIDTH),
                    (face[1] + face[3]) * 0.5 - canvas.r(1.6),
                ],
                *on,
                next,
                true,
            )?;
            canvas.hit(next, face)?;
            Ok(engaged(view, &[next]))
        }
        Control::Fixed { value } => {
            // A value the pack offers no alternative to, such as its name and author: shown
            // as a heading, not a control.
            let width = (inner[2] - inner[0]).max(1.0);
            let top = (b[1] + b[3] - canvas.r(BODY.line + CAPTION.line)) * 0.5;
            canvas.text_line(&element.label, [inner[0], top], width, BODY, label_color)?;
            canvas.text_line(
                value,
                [inner[0], top + canvas.r(BODY.line)],
                width,
                CAPTION,
                TEXT_DIMMER,
            )?;
            Ok(false)
        }
        Control::Choice { value } | Control::Profile { value } => {
            panel(canvas, b)?;
            label(canvas, &element.label, None, inner, label_color)?;
            let row = control_row(canvas, inner);
            arrows(canvas, view, row, previous, next)?;
            let field = [
                row[0] + canvas.r(ARROW_WIDTH + 0.4),
                row[1],
                row[2] - canvas.r(ARROW_WIDTH + 0.4),
                row[3],
            ];
            canvas.fill(field, theme::NEUTRAL90)?;
            canvas.text_centred(value, field, BODY, TEXT, false)?;
            Ok(engaged(view, &[previous, next]))
        }
        Control::Slider {
            value,
            index: selected,
            count,
        } => {
            panel(canvas, b)?;
            label(canvas, &element.label, Some(value), inner, label_color)?;
            let row = control_row(canvas, inner);
            arrows(canvas, view, row, previous, next)?;
            let steps: Vec<MenuAction> = (0..*count)
                .map(|step| element_action(options, index, Input::Select(step)))
                .collect();
            controls::slider(
                canvas,
                view,
                [
                    row[0] + canvas.r(ARROW_WIDTH + 1.2),
                    row[1],
                    row[2] - canvas.r(ARROW_WIDTH + 1.2),
                    row[3],
                ],
                &steps,
                usize::from(*selected),
                steps.len() <= 16,
            )?;
            Ok(engaged(view, &[previous, next]) || engaged(view, &steps))
        }
    }
}

/// `face` moved to the vertical centre of `cell`.
fn centred(face: Bounds, cell: Bounds) -> Bounds {
    let height = face[3] - face[1];
    let top = (cell[1] + cell[3] - height) * 0.5;
    [face[0], top, face[2], top + height]
}

/// A cell's raised background, as settings rows draw it.
fn panel(canvas: &mut Canvas<'_>, b: Bounds) -> Result<(), PaintError> {
    canvas.fill(b, NEUTRAL.fill)?;
    canvas.bevel(b, theme::BEVEL_LIGHT, theme::BEVEL_DARK)
}

/// The label on a cell's first line, with an optional value at its right.
fn label(
    canvas: &mut Canvas<'_>,
    text: &str,
    value: Option<&str>,
    inner: Bounds,
    color: Rgba,
) -> Result<(), PaintError> {
    let value_width = match value {
        Some(value) => canvas
            .measure(value, BODY)?
            .min((inner[2] - inner[0]) * 0.5),
        None => 0.0,
    };
    let gap = if value.is_some() { canvas.r(0.8) } else { 0.0 };
    canvas.text_line(
        text,
        [inner[0], inner[1]],
        (inner[2] - inner[0] - value_width - gap).max(1.0),
        BODY,
        color,
    )?;
    if let Some(value) = value {
        canvas.text_line(
            value,
            [inner[2] - value_width, inner[1]],
            value_width,
            BODY,
            TEXT,
        )?;
    }
    Ok(())
}

/// The control line at the bottom of a cell.
fn control_row(canvas: &Canvas<'_>, inner: Bounds) -> Bounds {
    [
        inner[0],
        inner[3] - canvas.r(CONTROL_HEIGHT),
        inner[2],
        inner[3],
    ]
}

/// The step-backwards and step-forwards buttons at either end of `row`. The glyphs are
/// centred over empty button faces, whose label padding is wider than these buttons.
fn arrows(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    row: Bounds,
    previous: MenuAction,
    next: MenuAction,
) -> Result<(), PaintError> {
    let width = canvas.r(ARROW_WIDTH);
    let color = canvas.role(theme::SECONDARY).text;
    for (bounds, glyph, action) in [
        ([row[0], row[1], row[0] + width, row[3]], "<", previous),
        ([row[2] - width, row[1], row[2], row[3]], ">", next),
    ] {
        button::action(canvas, view, bounds, "", Some(action))?;
        canvas.text_centred(glyph, bounds, BODY, color, false)?;
    }
    Ok(())
}

/// Height of the fixed footer under the grid.
pub(super) fn footer_height(canvas: &Canvas<'_>) -> f32 {
    canvas.r(PAD * 3.0 + BODY.line + CAPTION.line * COMMENT_LINES + button::ACTION_HEIGHT)
}

/// Draws the footer in `b`: the described element's label and comment, then the buttons.
pub(super) fn footer(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    options: &View,
    b: Bounds,
    described: Option<(usize, &Element)>,
) -> Result<(), PaintError> {
    canvas.fill(b, NEUTRAL.fill)?;
    widgets::divider(canvas, b[0], b[2], b[1])?;
    let pad = canvas.r(PAD + 1.2);
    let [left, right] = [b[0] + pad, b[2] - pad];
    let width = (right - left).max(1.0);
    let height = canvas.r(button::ACTION_HEIGHT);
    let row = [
        left,
        b[3] - canvas.r(PAD) - height,
        right,
        b[3] - canvas.r(PAD),
    ];
    let mut top = b[1] + canvas.r(PAD);
    if let Some((index, element)) = described.filter(|(_, element)| !element.label.is_empty()) {
        top += canvas
            .text_line(&element.label, [left, top], width, BODY, TEXT)?
            .max(canvas.r(BODY.line));
        if let Some(comment) = &element.comment {
            let bottom = (row[1] - canvas.r(PAD)).max(top + 1.0);
            let key = format!(
                "shader_option_comment/{}/{}/{}",
                options.pack, options.screen, index
            );
            let layout = canvas.layout(
                comment,
                ((width - canvas.r(1.2)).max(1.0) * 64.0) as u32,
                CAPTION,
            )?;
            let content = layout.size_64()[1] as f32 / 64.0;
            let mut scroll = canvas.begin_scroll(&key, [left, top, right, bottom])?;
            scroll.offset = scroll.offset.min((content - (bottom - top)).max(0.0));
            canvas.place_text(
                layout,
                [left, top - scroll.offset],
                width,
                TEXT_DIMMER,
                false,
            )?;
            canvas.end_scroll(scroll, content)?;
        }
    }
    let ready = matches!(options.status, Status::Ready | Status::SaveFailed(_));
    let available = options.status != Status::Saving;
    let buttons = [
        ("Reset", ready.then_some(Action::Reset)),
        ("Cancel", available.then_some(Action::Cancel)),
        ("Apply", (ready && options.changed).then_some(Action::Apply)),
        ("Done", available.then_some(Action::Done)),
    ];
    let gap = canvas.r(GAP);
    let slots = buttons.len() + usize::from(options.nested);
    let button_width =
        ((width - gap * (slots - 1) as f32) / slots as f32).min(canvas.r(button::ACTION_MIN_WIDTH));
    if options.nested {
        button::action(
            canvas,
            view,
            [row[0], row[1], row[0] + button_width, row[3]],
            "Back",
            available.then_some(MenuAction::ShaderOptions(Action::Back)),
        )?;
    }
    for (slot, (text, action)) in buttons.iter().rev().enumerate() {
        let x = row[2] - slot as f32 * (button_width + gap);
        button::action(
            canvas,
            view,
            [x - button_width, row[1], x, row[3]],
            text,
            action.map(MenuAction::ShaderOptions),
        )?;
    }
    Ok(())
}
