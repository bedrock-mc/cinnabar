//! Compact preview tiles for removable local cosmetics.

use super::{command, enabled, library};
use crate::{
    icons::{self, Icon},
    motion::{Kind, mix, opacity},
    paint::{Bounds, Canvas},
    theme::{self, CAPTION, EDGE, TEXT, TEXT_DIMMER},
    widgets::{self, Variant},
};
use launcher::{dressing_room::Action, menu::MenuView};
use ui_engine::engine::screen::PaintError;

/// Paints fixed-size square previews, wrapping without stretching sparse collections.
pub(super) fn collection(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    viewport: Bounds,
    preview: &mut super::PreviewArea,
) -> Result<f32, PaintError> {
    let [left, top, right, _] = b;
    let side = canvas.r(12.8).min(right - left);
    let gap = canvas.r(1.2);
    let control_width = side.min((right - left - gap) * 0.5);
    let anchor = [left, top, left + control_width, top + canvas.r(4.4)];
    super::filter::button(canvas, view, anchor)?;
    preview.filter_anchor = Some(anchor);
    let remove_left = anchor[2] + gap;
    widgets::button(
        canvas,
        view,
        [
            remove_left,
            top,
            remove_left + control_width,
            top + canvas.r(4.4),
        ],
        if view
            .dressing_room
            .selected_cosmetics
            .values()
            .all(Option::is_none)
        {
            Variant::Primary
        } else {
            Variant::Secondary
        },
        "Remove all",
        enabled(view).then_some(command(Action::SelectCosmetic(None))),
    )?;
    let y = top + canvas.r(6.0);
    let columns = (((right - left + gap) / (side + gap)).floor() as usize).max(1);
    let items = cosmetics::Item::all();
    let mut count: usize = 0;
    for (slot, (index, hat)) in items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            view.dressing_room
                .cosmetic_category
                .includes_slot(&item.descriptor().slot)
        })
        .enumerate()
    {
        let x = left + (slot % columns) as f32 * (side + gap);
        let y = y + (slot / columns) as f32 * (side + gap);
        let bounds = [x, y, x + side, y + side];
        if bounds[0] < viewport[2]
            && bounds[2] > viewport[0]
            && bounds[1] < viewport[3]
            && bounds[3] > viewport[1]
        {
            preview.visible_cosmetics.push(index);
        }
        tile(canvas, view, bounds, *hat)?;
        count += 1;
    }
    if count == 0 {
        let height = canvas.text(
            "No cosmetics in this category.",
            [left, y],
            right - left,
            CAPTION,
            TEXT_DIMMER,
            false,
        )?;
        return Ok(y + height);
    }
    Ok(y + count.div_ceil(columns) as f32 * (side + gap))
}

/// Uses the existing surface feedback, selection mark and focus frame around the preview.
fn tile(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    hat: cosmetics::Item,
) -> Result<(), PaintError> {
    let action = command(Action::SelectCosmetic(Some(hat)));
    let selected = view
        .dressing_room
        .selected_cosmetics
        .get(&hat.descriptor().slot)
        .and_then(|value| value.as_deref())
        == Some(hat.id());
    let state = canvas.interaction(view, Some(action));
    let motion = canvas.feedback(state, enabled(view), selected, Kind::Surface);
    let role = canvas.role(theme::NEUTRAL);
    canvas.fill(
        b,
        motion.color(role.fill, role.hovered, canvas.role(theme::NEUTRAL80).fill),
    )?;
    canvas.frame(
        b,
        EDGE,
        mix(
            canvas.appearance.surface(theme::BORDER),
            theme::PRIMARY_ROLE.fill,
            motion.selected,
        ),
    )?;
    canvas.specular(b, theme::BEVEL_LIGHT, theme::BEVEL_DARK)?;
    let pad = canvas.r(1.2);
    if let Some(icon) = canvas
        .artwork
        .and_then(|art| art.get(&hat.thumbnail_key()))
        .copied()
    {
        library::thumbnail(
            canvas,
            icon,
            [b[0] + pad, b[1] + pad, b[2] - pad, b[3] - pad],
        )?;
    }
    if selected {
        let badge = [b[2] - canvas.r(2.8), b[1], b[2], b[1] + canvas.r(2.8)];
        canvas.fill(badge, theme::PRIMARY_ROLE.fill)?;
        icons::draw(
            canvas,
            Icon::Check,
            [badge[0] + canvas.r(0.6), badge[1] + canvas.r(0.6)],
            TEXT,
        )?;
    }
    if motion.focus > 0.0 {
        canvas.frame(b, EDGE, opacity(theme::OUTLINE, motion.focus))?;
    }
    if enabled(view) {
        canvas.hit(action, b)?;
    }
    Ok(())
}
