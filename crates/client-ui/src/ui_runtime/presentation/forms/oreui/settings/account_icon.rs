//! The Account category uses its signed-in gamerpic with the native image border.

use super::super::super::super::UiPresentationError;
use super::super::paint::{Bounds, Canvas};
use super::super::theme;
use crate::menu::{MenuView, auth::AuthState};
use crate::ui_runtime::oreui_assets::SETTINGS_ICONS;
use crate::ui_runtime::presentation::IconRef;

#[cfg(test)]
mod tests;

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
    gamerpic: Option<IconRef>,
) -> Result<bool, UiPresentationError> {
    if view.auth_state == AuthState::Authenticated && !view.feeds.profile.picture_path.is_empty() {
        if let Some(gamerpic) = gamerpic {
            canvas.icon_ref(square_cover(gamerpic), bounds)?;
        }
        canvas.frame(bounds, theme::EDGE, theme::NEUTRAL.border)?;
        return Ok(true);
    }
    canvas.sprite(SETTINGS_ICONS[8], bounds, [255; 4])
}

fn square_cover(mut icon: IconRef) -> IconRef {
    let [left, top, right, bottom] = icon.uv;
    let side = right.saturating_sub(left).min(bottom.saturating_sub(top));
    if side != 0 {
        let x = ((u32::from(left) + u32::from(right) - u32::from(side)) / 2) as u16;
        let y = ((u32::from(top) + u32::from(bottom) - u32::from(side)) / 2) as u16;
        icon.uv = [x, y, x + side, y + side];
    }
    icon
}
