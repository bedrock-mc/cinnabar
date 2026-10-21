//! A compact cosmetic category control and its anchored choice menu.

use super::{command, enabled};
use crate::{
    icons::{self, Icon},
    paint::{Bounds, Canvas},
    theme::{self, BODY, EDGE},
    widgets::{self, MenuItem, Variant},
};
use launcher::{
    dressing_room::{Action, CosmeticCategory},
    menu::MenuView,
};
use ui_engine::engine::{rect, screen::PaintError};

/// Shows the filter glyph and current category in a single secondary button.
pub(super) fn button(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
) -> Result<(), PaintError> {
    let action = command(Action::ToggleCosmeticFilter);
    let state = canvas.interaction(view, Some(action));
    widgets::button_face(canvas, bounds, Variant::Secondary, "", state, enabled(view))?;
    let colour = if enabled(view) {
        canvas.role(Variant::Secondary.role(canvas.bundle)).text
    } else {
        theme::DISABLED.text
    };
    icons::draw(
        canvas,
        Icon::Filter,
        [bounds[0] + canvas.r(1.2), bounds[1] + canvas.r(0.8)],
        colour,
    )?;
    canvas.text_centred(
        view.dressing_room.cosmetic_category.label(),
        [
            bounds[0] + canvas.r(4.0),
            bounds[1],
            bounds[2] - canvas.r(0.8),
            bounds[3] - canvas.r(0.4),
        ],
        BODY,
        colour,
        false,
    )?;
    if enabled(view) {
        canvas.hit(action, bounds)?;
    }
    Ok(())
}

/// Owns pointer and keyboard input until a category, outside click or Escape closes it.
pub(super) fn popup(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    anchor: Bounds,
) -> Result<(), PaintError> {
    canvas.hits.clear();
    canvas.clear_focus_geometry();
    canvas.scrolls.clear();
    canvas.slider_tracks.clear();
    canvas.spots.clear();
    let close = command(Action::ToggleCosmeticFilter);
    canvas.hits.push((close, rect(0.0, 0.0, size[0], size[1])?));
    canvas.hit(close, anchor)?;
    let margin = canvas.r(0.8);
    let width = canvas.r(18.0).min(size[0] - margin * 2.0);
    let row_height = canvas
        .r(4.8)
        .min((size[1] - margin * 2.0) / CosmeticCategory::ALL.len() as f32);
    let height = row_height * CosmeticCategory::ALL.len() as f32;
    let left = anchor[0].clamp(margin, (size[0] - width - margin).max(margin));
    let below = anchor[3] + canvas.r(0.4);
    let top = if below + height <= size[1] - margin {
        below
    } else {
        (anchor[1] - height - canvas.r(0.4)).max(margin)
    };
    let top = top.min((size[1] - height - margin).max(margin));
    for (index, category) in CosmeticCategory::ALL.into_iter().enumerate() {
        let y = top + index as f32 * row_height;
        widgets::menu_item(
            canvas,
            view,
            [left, y, left + width, y + row_height],
            &MenuItem {
                label: category.label(),
                picture_slot: false,
                picture: None,
                selected: category == view.dressing_room.cosmetic_category,
                enabled: true,
                action: Some(command(Action::SetCosmeticCategory(category))),
            },
        )?;
    }
    canvas.fill(
        [
            left,
            top + height,
            left + width,
            top + height + canvas.r(EDGE),
        ],
        theme::BORDER,
    )
}
