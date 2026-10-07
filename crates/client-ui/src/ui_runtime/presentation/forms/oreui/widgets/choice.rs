//! Raised segmented choices share selection, press travel and focus across menus.

use super::super::super::super::UiPresentationError;
use super::super::motion::{Kind, mix};
use super::super::paint::{Bounds, Canvas};
use super::super::theme::{self, BODY, EDGE};
use crate::menu::{MenuAction, MenuView};

const CHOICE_PADDING: f32 = 1.2;

pub(in super::super) fn choice_height(
    canvas: &mut Canvas<'_>,
    label: &str,
    width: f32,
) -> Result<f32, UiPresentationError> {
    let padding = canvas.r(CHOICE_PADDING);
    let text = canvas.measure_height(label, (width - padding * 2.0).max(1.0), BODY)?;
    Ok(text.max(canvas.r(5.2)) + canvas.r(0.8))
}
pub(in super::super) fn choice(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    label: &str,
    on: bool,
    action: MenuAction,
) -> Result<(), UiPresentationError> {
    let state = canvas.interaction(view, Some(action));
    let motion = canvas.feedback(state, true, on, Kind::Button);
    let role = if on {
        theme::PRIMARY_ROLE
    } else {
        theme::SECONDARY
    };
    let role = canvas.role(role);
    let secondary = canvas.role(theme::SECONDARY);
    let down = motion.depression();
    let outer = [b[0], b[1] + canvas.r(0.4) * down, b[2], b[3]];
    canvas.fill(outer, secondary.border)?;
    let edge = canvas.r(EDGE);
    let face = [
        outer[0] + edge,
        outer[1] + edge,
        outer[2] - edge,
        outer[3] - edge - canvas.r(0.4) * (1.0 - down),
    ];
    if down < 1.0 {
        canvas.fill(
            [face[0], face[3], face[2], outer[3] - edge],
            secondary.shadow,
        )?;
    }
    canvas.fill(
        face,
        mix(
            motion.color(role.fill, role.hovered, role.pressed),
            role.fill,
            motion.selected,
        ),
    )?;
    let specular = std::array::from_fn::<_, 2, _>(|index| {
        mix(
            role.specular[index],
            role.specular_hovered[index],
            motion.hover * (1.0 - motion.selected),
        )
    });
    canvas.specular(face, specular[0], specular[1])?;
    let padding = canvas.r(CHOICE_PADDING);
    let width = (b[2] - b[0] - padding * 2.0).max(1.0);
    let height = canvas.measure_height(label, width, BODY)?;
    canvas.centered_wrapped_text(
        label,
        [b[0] + padding, (face[1] + face[3] - height) * 0.5],
        width,
        BODY,
        role.text,
    )?;
    if on {
        let centre = (face[0] + face[2]) * 0.5;
        let half = canvas.r(2.4).min((face[2] - face[0]) * 0.5);
        canvas.fill(
            [centre - half, face[3] - edge, centre + half, face[3]],
            theme::TEXT,
        )?;
    }
    canvas.hit(action, b)?;
    Ok(())
}

pub(in super::super) fn choice_focus(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    on: bool,
    action: MenuAction,
) -> Result<(), UiPresentationError> {
    let state = canvas.interaction(view, Some(action));
    let motion = canvas.feedback(state, true, on, Kind::Button);
    if motion.focus > 0.0 {
        focus_outline(
            canvas,
            [b[0], b[1] + canvas.r(0.4) * motion.depression(), b[2], b[3]],
        )?;
    }
    Ok(())
}

fn focus_outline(canvas: &mut Canvas<'_>, b: Bounds) -> Result<(), UiPresentationError> {
    let outset = canvas.r(0.4);
    canvas.frame(
        [b[0] - outset, b[1] - outset, b[2] + outset, b[3] + outset],
        EDGE,
        theme::OUTLINE,
    )
}

#[cfg(test)]
mod tests;
