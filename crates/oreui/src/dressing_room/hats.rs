//! Compact preview tiles for removable local headwear.

use super::{command, enabled, library};
use crate::{
    icons::{self, Icon},
    motion::{Kind, mix, opacity},
    paint::{Bounds, Canvas},
    theme::{self, EDGE, TEXT},
    widgets::{self, Variant},
};
use launcher::{dressing_room::Action, menu::MenuView};
use ui_engine::engine::screen::PaintError;

/// Paints fixed-size square previews, wrapping without stretching sparse collections.
pub(super) fn collection(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
) -> Result<f32, PaintError> {
    let [left, top, right, _] = b;
    let side = canvas.r(12.8).min(right - left);
    widgets::button(
        canvas,
        view,
        [left, top, left + side, top + canvas.r(4.4)],
        if view.dressing_room.selected_hat.is_none() {
            Variant::Primary
        } else {
            Variant::Secondary
        },
        "No hat",
        enabled(view).then_some(command(Action::SelectHat(None))),
    )?;
    let y = top + canvas.r(6.0);
    let gap = canvas.r(1.2);
    let columns = (((right - left + gap) / (side + gap)).floor() as usize).max(1);
    for (slot, hat) in render_api::Hat::ALL.iter().enumerate() {
        let x = left + (slot % columns) as f32 * (side + gap);
        let y = y + (slot / columns) as f32 * (side + gap);
        tile(canvas, view, [x, y, x + side, y + side], *hat)?;
    }
    Ok(y + render_api::Hat::ALL.len().div_ceil(columns) as f32 * (side + gap))
}

/// Uses the existing surface feedback, selection mark and focus frame around the preview.
fn tile(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    hat: render_api::Hat,
) -> Result<(), PaintError> {
    let action = command(Action::SelectHat(Some(hat)));
    let selected = view.dressing_room.selected_hat == Some(hat);
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
