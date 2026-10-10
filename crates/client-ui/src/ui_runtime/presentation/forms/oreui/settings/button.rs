//! Settings actions use elevated artwork; compact reset and picker faces use role colours.

use crate::ui_runtime::oreui_assets::{EXTERNAL_LINK_ICON, RESET_ICON};

use super::super::super::super::UiPresentationError;
use super::super::motion::{Kind, mix, opacity};
use super::super::paint::{Bounds, Canvas};
use super::super::theme::{self, BODY, EDGE};
use super::super::widgets::{self, Interaction, Variant};
use crate::menu::{MenuAction, MenuView};

pub(super) const ACTION_HEIGHT: f32 = 4.8;
pub(super) const ACTION_MIN_WIDTH: f32 = 14.0;
const ELEVATION: f32 = 0.4;
const ICON_SIZE: f32 = 2.4;
const ICON_GAP: f32 = 0.8;

pub(super) fn icon_width(action: Option<MenuAction>) -> f32 {
    use crate::menu::settings_support::SupportAction;
    if matches!(
        action,
        Some(MenuAction::SettingsSupport(SupportAction::Open(_)))
    ) {
        ICON_SIZE + ICON_GAP
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests;

pub(super) fn action(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
    label: &str,
    action: Option<MenuAction>,
) -> Result<(), UiPresentationError> {
    let mut state = canvas.interaction(view, action);
    if state.focused {
        state.hovered = false;
    }
    widgets::button_face(
        canvas,
        bounds,
        Variant::Secondary,
        "",
        state,
        action.is_some(),
    )?;
    let motion = canvas.feedback(state, action.is_some(), false, Kind::Button);
    let content_top = bounds[1] + canvas.r(ELEVATION) * motion.press;
    let content_bottom = bounds[3] - canvas.r(ELEVATION) * (1.0 - motion.press);
    let border = canvas.r(if action.is_some() { 0.4 } else { 0.2 });
    let available = (bounds[2] - bounds[0] - 2.0 * (border + canvas.r(2.0))).max(1.0);
    let icon_width = canvas.r(icon_width(action));
    let width = canvas
        .measure(label, BODY)?
        .min((available - icon_width).max(1.0));
    let left = (bounds[0] + bounds[2] - width - icon_width) * 0.5;
    if icon_width > 0.0 {
        let size = canvas.r(ICON_SIZE);
        let top = (content_top + content_bottom - size) * 0.5;
        canvas.masked_sprite(
            EXTERNAL_LINK_ICON,
            [left, top, left + size, top + size],
            canvas.role(theme::SECONDARY).text,
        )?;
    }
    canvas.text_line(
        label,
        [
            left + icon_width,
            (content_top + content_bottom - canvas.r(BODY.line)) * 0.5,
        ],
        width,
        BODY,
        if action.is_some() {
            canvas.role(theme::SECONDARY).text
        } else {
            canvas.role(theme::DISABLED).text
        },
    )?;
    if let Some(action) = action {
        canvas.hit(action, bounds)?;
    }
    Ok(())
}

/// Returns the visible outer box and its content face for an elevated role control.
pub(super) fn procedural_face(
    canvas: &mut Canvas<'_>,
    flow: Bounds,
    state: Interaction,
    enabled: bool,
) -> Result<(Bounds, Bounds), UiPresentationError> {
    let role = if enabled {
        theme::SECONDARY
    } else {
        theme::DISABLED
    };
    let role = canvas.role(role);
    let motion = canvas.feedback(state, enabled, false, Kind::Button);
    let shadow = canvas.r(ELEVATION) * (1.0 - motion.press);
    let outer = [flow[0], flow[1] - shadow, flow[2], flow[3]];
    canvas.fill(outer, role.border)?;
    let edge = canvas.r(EDGE);
    let face = [
        outer[0] + edge,
        outer[1] + edge,
        outer[2] - edge,
        outer[3] - edge - shadow,
    ];
    canvas.fill([face[0], face[3], face[2], outer[3] - edge], role.shadow)?;
    canvas.fill(face, motion.color(role.fill, role.hovered, role.pressed))?;
    if enabled {
        let specular = std::array::from_fn::<_, 2, _>(|index| {
            mix(
                role.specular[index],
                role.specular_hovered[index],
                motion
                    .hover
                    .max(motion.press * u8::from(state.hovered) as f32),
            )
        });
        canvas.specular(face, specular[0], specular[1])?;
    }
    if motion.focus > 0.0 {
        let outset = canvas.r(0.4);
        canvas.frame(
            [
                outer[0] - outset,
                outer[1] - outset,
                outer[2] + outset,
                outer[3] + outset,
            ],
            EDGE,
            opacity(theme::OUTLINE, motion.focus),
        )?;
    }
    Ok((outer, face))
}

pub(super) fn binding_reset(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    flow: Bounds,
    action: MenuAction,
) -> Result<(), UiPresentationError> {
    let state = canvas.interaction(view, Some(action));
    let (_, face) = procedural_face(canvas, flow, state, true)?;
    let half = canvas.r(1.2);
    let centre = [(face[0] + face[2]) * 0.5, (face[1] + face[3]) * 0.5];
    canvas.masked_sprite(
        RESET_ICON,
        [
            centre[0] - half,
            centre[1] - half,
            centre[0] + half,
            centre[1] + half,
        ],
        canvas.role(theme::SECONDARY).text,
    )?;
    stationary_hit(canvas, action, flow)
}

pub(super) fn stationary_hit(
    canvas: &mut Canvas<'_>,
    action: MenuAction,
    flow: Bounds,
) -> Result<(), UiPresentationError> {
    canvas.hit(
        action,
        [flow[0], flow[1] - canvas.r(ELEVATION), flow[2], flow[3]],
    )
}
