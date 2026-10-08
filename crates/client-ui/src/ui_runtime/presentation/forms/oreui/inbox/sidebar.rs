//! Native category spacing, selection bevels and the shared quick icon sweep.

use super::super::motion::{Kind, opacity};
use super::super::paint::Bounds;
use super::super::theme::{BODY, BORDER, CAPTION, EDGE, NEUTRAL80, OUTLINE, TEXT};
use super::{Action, CATEGORIES, Canvas, MenuAction, MenuView, UNREAD, UiPresentationError, inbox};
use crate::ui_runtime::oreui_assets::SETTINGS_ICON_HIGHLIGHT_IMAGE;

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
    narrow: bool,
) -> Result<(), UiPresentationError> {
    let row_height = canvas.r(4.8);
    let pad = canvas.r(1.6);
    let edge = canvas.r(EDGE);
    let content = row_height * CATEGORIES.len() as f32 + pad * 2.0;
    let panel = [
        bounds[0],
        bounds[1],
        bounds[2],
        (bounds[1] + content + edge * 2.0).min(bounds[3]),
    ];
    canvas.fill(panel, NEUTRAL80.fill)?;
    canvas.frame(panel, EDGE, BORDER)?;
    let scroll = canvas.begin_scroll(
        "oreui_inbox_sidebar",
        [
            panel[0] + edge,
            panel[1] + edge,
            panel[2] - edge,
            panel[3] - edge,
        ],
    )?;
    if let Some(transitions) = canvas.transitions.as_deref_mut() {
        transitions.begin_inbox(view.feeds.inbox_state.category as u8);
    }
    let inset = canvas.r(if narrow { 0.8 } else { 1.6 });
    let side = super::super::icons::native_side(canvas);
    let mut focus = None;
    for (index, label) in CATEGORIES.iter().enumerate() {
        let top = panel[1] + edge + pad + index as f32 * row_height - scroll.offset;
        let row = [panel[0] + edge, top, panel[2] - edge, top + row_height];
        let action = MenuAction::Inbox(Action::Category(index));
        let mut state = canvas.interaction(view, Some(action));
        state.pressed &= state.hovered;
        let selected = view.feeds.inbox_state.category == index;
        let motion = canvas.feedback(state, true, selected, Kind::Surface);
        super::super::sidebar::background(canvas, row, motion)?;
        if motion.focus > 0.0 {
            let overlap = edge * if selected || state.hovered { 2.0 } else { 1.0 };
            focus = Some((
                [row[0], row[1] - overlap, row[2], row[3] + overlap],
                motion.focus,
            ));
        }
        let at = [row[0] + inset, (row[1] + row[3] - side) * 0.5];
        super::icons::category_icon(canvas, index, at)?;
        if let Some(frame) = canvas
            .transitions
            .as_deref()
            .and_then(|transitions| transitions.inbox_icon_frame(index as u8))
        {
            canvas.sprite_frame(
                SETTINGS_ICON_HIGHLIGHT_IMAGE,
                [at[0], at[1], at[0] + side, at[1] + side],
                [255; 4],
                frame,
                super::super::transitions::ICON_HIGHLIGHT_FRAMES,
            )?;
        }
        let count = view
            .feeds
            .home
            .inbox_counts
            .get(&index)
            .copied()
            .unwrap_or_else(|| {
                view.feeds
                    .home
                    .inbox
                    .iter()
                    .filter(|item| {
                        item.unread && inbox::category_index(&item.category) == Some(index)
                    })
                    .count() as u32
            });
        let badge_width = if count > 0 {
            canvas.measure(&count.to_string(), CAPTION)? + canvas.r(0.8)
        } else {
            0.0
        };
        let left = at[0] + side + canvas.r(0.8);
        canvas.text_line_vertically_centred(
            label,
            [
                left,
                row[1],
                (row[2] - inset - badge_width - canvas.r(0.8)).max(left + 1.0),
                row[3],
            ],
            BODY,
            TEXT,
        )?;
        if count > 0 {
            let centre = (row[1] + row[3]) * 0.5;
            let badge = [
                row[2] - inset - badge_width,
                centre - canvas.r(1.0),
                row[2] - inset,
                centre + canvas.r(1.0),
            ];
            canvas.fill(badge, UNREAD)?;
            canvas.text_centred(&count.to_string(), badge, CAPTION, BORDER, false)?;
        }
        canvas.hit(action, row)?;
    }
    if let Some((bounds, alpha)) = focus {
        canvas.frame(bounds, EDGE, opacity(OUTLINE, alpha))?;
    }
    canvas.end_scroll(scroll, content)
}
