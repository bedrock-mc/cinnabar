//! Shared flat category faces keep press and selection brightness continuous.

use super::super::super::UiPresentationError;
use super::motion::{Feedback, mix};
use super::paint::{Bounds, Canvas};
use super::theme::{EDGE, NEUTRAL80};

pub(super) fn background(
    canvas: &mut Canvas<'_>,
    row: Bounds,
    motion: Feedback,
) -> Result<(), UiPresentationError> {
    background_on(canvas, row, motion, NEUTRAL80.fill)
}

pub(super) fn transparent_background(
    canvas: &mut Canvas<'_>,
    row: Bounds,
    motion: Feedback,
) -> Result<(), UiPresentationError> {
    background_on(canvas, row, motion, [0; 4])
}

fn background_on(
    canvas: &mut Canvas<'_>,
    row: Bounds,
    motion: Feedback,
    idle: super::theme::Rgba,
) -> Result<(), UiPresentationError> {
    let role = canvas.role(NEUTRAL80);
    let idle = if idle[3] == 0 {
        [role.hovered[0], role.hovered[1], role.hovered[2], 0]
    } else {
        canvas.appearance.surface(idle)
    };
    let fill = mix(
        idle,
        role.hovered,
        motion.hover.max(motion.press).max(motion.selected),
    );
    let base = [
        mix(
            mix([0; 4], role.specular[0], motion.hover),
            [0, 0, 0, 204],
            motion.press,
        ),
        mix(
            mix([0; 4], role.specular[1], motion.hover),
            role.specular[0],
            motion.press,
        ),
    ];
    let bevel = [
        mix(base[0], role.specular[1], motion.selected),
        mix(base[1], role.specular[0], motion.selected),
    ];
    canvas.fill(row, fill)?;
    let [upper, lower] = bevel;
    if upper[3] > 0 || lower[3] > 0 {
        let edge = canvas.r(EDGE);
        canvas.fill([row[0], row[1], row[2], row[1] + edge], upper)?;
        canvas.fill([row[0], row[3] - edge, row[2], row[3]], lower)?;
    }
    Ok(())
}
