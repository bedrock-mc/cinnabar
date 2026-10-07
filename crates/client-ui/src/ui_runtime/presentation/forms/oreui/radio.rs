//! Radio diamonds rotate the complete bordered square, including its speculars and centre.

use super::super::super::UiPresentationError;
use super::motion::{Kind, mix, opacity};
use super::paint::{Bounds, Canvas};
use super::theme::{self, Rgba};
use super::widgets::Interaction;

#[cfg(test)]
mod tests;

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    centre: [f32; 2],
    checked: bool,
    state: Interaction,
    enabled: bool,
) -> Result<(), UiPresentationError> {
    let motion = canvas.feedback(state, enabled, checked, Kind::Radio);
    let secondary = canvas.role(theme::SECONDARY);
    let disabled = canvas.role(theme::DISABLED);
    let mut square = Square { canvas, centre };
    if motion.focus > 0.0 {
        square.frame(
            [-1.4, -1.4, 1.4, 1.4],
            0.2,
            opacity(theme::OUTLINE, motion.focus),
        )?;
    }
    let border = if enabled {
        theme::BORDER
    } else if checked {
        theme::DISABLED.border
    } else {
        theme::DISABLED.shadow
    };
    square.fill([-1.0, -1.0, 1.0, 1.0], border)?;
    let fill = if !enabled {
        theme::DISABLED.fill
    } else {
        mix(
            motion.color(disabled.shadow, secondary.hovered, secondary.shadow),
            motion.color(
                theme::PRIMARY_ROLE.fill,
                theme::PRIMARY_ROLE.hovered,
                theme::PRIMARY_ROLE.pressed,
            ),
            motion.selected,
        )
    };
    square.fill([-0.8, -0.8, 0.8, 0.8], fill)?;
    square.fill([-0.8, 0.6, 0.8, 0.8], theme::NEUTRAL.specular[1])?;
    square.fill([0.6, -0.8, 0.8, 0.6], theme::NEUTRAL.specular[1])?;
    square.fill([-0.8, -0.8, 0.8, -0.6], theme::NEUTRAL.specular[0])?;
    square.fill([-0.8, -0.6, -0.6, 0.8], theme::NEUTRAL.specular[0])?;
    if motion.selected > 0.0 {
        let scale = 0.75 + 0.25 * motion.selected;
        for (bounds, color) in [
            ([-0.4, -0.4, 0.0, 0.0], theme::TEXT),
            ([0.0, -0.4, 0.4, 0.0], theme::NEUTRAL20.fill),
            ([-0.4, 0.0, 0.0, 0.4], theme::NEUTRAL20.fill),
            ([0.0, 0.0, 0.4, 0.4], theme::SECONDARY.fill),
        ] {
            let color = if square.canvas.appearance == theme::Appearance::Dark {
                theme::TEXT
            } else {
                color
            };
            square.fill(
                bounds.map(|value| value * scale),
                opacity(color, motion.selected),
            )?;
        }
    }
    Ok(())
}

struct Square<'a, 'b> {
    canvas: &'a mut Canvas<'b>,
    centre: [f32; 2],
}

impl Square<'_, '_> {
    fn fill(&mut self, b: Bounds, color: Rgba) -> Result<(), UiPresentationError> {
        let dx = self.canvas.r((b[0] + b[2]) * 0.5);
        let dy = self.canvas.r((b[1] + b[3]) * 0.5);
        let x = self.centre[0] + (dx - dy) * std::f32::consts::FRAC_1_SQRT_2;
        let y = self.centre[1] + (dx + dy) * std::f32::consts::FRAC_1_SQRT_2;
        let half_width = self.canvas.r(b[2] - b[0]) * 0.5;
        let half_height = self.canvas.r(b[3] - b[1]) * 0.5;
        self.canvas.rotated_fill(
            [
                x - half_width,
                y - half_height,
                x + half_width,
                y + half_height,
            ],
            color,
            std::f32::consts::FRAC_PI_4,
        )
    }

    fn frame(&mut self, b: Bounds, width: f32, color: Rgba) -> Result<(), UiPresentationError> {
        for side in [
            [b[0], b[1], b[2], b[1] + width],
            [b[0], b[3] - width, b[2], b[3]],
            [b[0], b[1] + width, b[0] + width, b[3] - width],
            [b[2] - width, b[1] + width, b[2], b[3] - width],
        ] {
            self.fill(side, color)?;
        }
        Ok(())
    }
}
