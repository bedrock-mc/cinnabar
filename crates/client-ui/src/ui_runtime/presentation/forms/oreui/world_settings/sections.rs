//! Neutral form rows fit their wrapped content and keep native divider bevels.

use super::super::theme::{BORDER, DISABLED, EDGE, NEUTRAL};
use super::*;
use crate::ui_runtime::oreui_assets::{HARDCORE_ICON, SWITCH_OFF_IMAGE};

pub(super) fn row(
    canvas: &mut Canvas<'_>,
    area: Bounds,
    y: f32,
    draw: impl FnOnce(&mut Canvas<'_>, Bounds) -> Result<f32, UiPresentationError>,
) -> Result<f32, UiPresentationError> {
    let edge = canvas.r(EDGE);
    let pad = canvas.r(2.4);
    let vertical = canvas.r(1.2);
    let first = canvas.nodes.len();
    canvas.fill([area[0], y, area[2], y + vertical], NEUTRAL.fill)?;
    let end = draw(
        canvas,
        [area[0] + pad, y + vertical, area[2] - pad, area[3]],
    )? + vertical;
    let bounds = [area[0], y, area[2], end + edge * 2.0];
    // Retained siblings paint by ID; resize the earlier background after measuring its content.
    canvas.resize_fill(first, bounds)?;
    canvas.specular(bounds, NEUTRAL.specular[0], NEUTRAL.specular[1])?;
    canvas.fill([bounds[0], bounds[1], bounds[0] + edge, bounds[3]], BORDER)?;
    canvas.fill([bounds[2] - edge, bounds[1], bounds[2], bounds[3]], BORDER)?;
    Ok(bounds[3])
}

pub(super) fn hardcore(canvas: &mut Canvas<'_>, area: Bounds) -> Result<f32, UiPresentationError> {
    let width = canvas.r(6.4);
    let gap = canvas.r(1.6);
    let label_area = [area[0], area[1], area[2] - width - gap, area[3]];
    let y = label(canvas, "Hardcore", label_area, area[1])?;
    let end = caption(
        canvas,
        "You can't respawn if you die. Good luck! You'll need it.",
        label_area,
        y,
    )?;
    let at = [area[2] - width, (area[1] + end - canvas.r(3.2)) * 0.5];
    let rail = [
        at[0] + canvas.r(0.2),
        at[1] + canvas.r(0.2),
        area[2] - canvas.r(0.2),
        at[1] + canvas.r(3.0),
    ];
    canvas.fill(rail, DISABLED.border)?;
    let edge = canvas.r(EDGE);
    let inner = [
        rail[0] + edge,
        rail[1] + edge,
        rail[2] - edge,
        rail[3] - edge,
    ];
    canvas.fill(inner, DISABLED.fill)?;
    let side = canvas.r(1.6);
    canvas.sprite(
        SWITCH_OFF_IMAGE,
        [
            inner[2] - canvas.r(2.0),
            at[1] + canvas.r(0.8),
            inner[2] - canvas.r(0.4),
            at[1] + canvas.r(2.4),
        ],
        [255; 4],
    )?;
    let thumb = [at[0], at[1], at[0] + canvas.r(3.2), at[1] + canvas.r(3.2)];
    canvas.fill(thumb, DISABLED.border)?;
    let face = [
        thumb[0] + edge,
        thumb[1] + edge,
        thumb[2] - edge,
        thumb[3] - canvas.r(0.4) - edge,
    ];
    canvas.fill(face, DISABLED.fill)?;
    canvas.fill(
        [face[0], face[3], face[2], thumb[3] - edge],
        DISABLED.shadow,
    )?;
    let x = (face[0] + face[2] - side) * 0.5;
    let y = (face[1] + face[3] - side) * 0.5;
    canvas.sprite(HARDCORE_ICON, [x, y, x + side, y + side], [255; 4])?;
    Ok(end.max(thumb[3]))
}
