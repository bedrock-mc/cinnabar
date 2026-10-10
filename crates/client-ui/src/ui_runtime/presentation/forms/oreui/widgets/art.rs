//! Pressable artwork uses the installed state's native border-image slices.

use crate::ui_runtime::oreui_assets::{
    BUTTON_DESTRUCTIVE_IMAGES, BUTTON_DISABLED_IMAGE, BUTTON_NEUTRAL_IMAGES, BUTTON_PRIMARY_IMAGES,
    BUTTON_SECONDARY_IMAGES,
};

use super::super::motion::{Feedback, opacity};
use super::super::theme::{BUTTON_DEPTH, GUI_PIXELS_PER_REM};
use super::{Bounds, Canvas, Interaction, UiPresentationError, Variant};

#[cfg(test)]
mod tests;

fn artwork(variant: Variant) -> [&'static str; 4] {
    match variant {
        Variant::Hero | Variant::Primary => BUTTON_PRIMARY_IMAGES,
        Variant::Secondary => BUTTON_SECONDARY_IMAGES,
        Variant::Neutral => BUTTON_NEUTRAL_IMAGES,
        Variant::Destructive => BUTTON_DESTRUCTIVE_IMAGES,
    }
}

pub(super) fn elevated_motion(
    canvas: &mut Canvas<'_>,
    bounds: Bounds,
    variant: Variant,
    state: Interaction,
    enabled: bool,
    motion: Feedback,
) -> Result<bool, UiPresentationError> {
    if canvas.appearance == super::super::theme::Appearance::Dark
        && (!enabled || matches!(variant, Variant::Secondary | Variant::Neutral))
    {
        return Ok(false);
    }
    if motion == Feedback::immediate(state, enabled, false) || !enabled {
        return elevated(canvas, bounds, variant, state, enabled);
    }
    let keys = artwork(variant);
    if canvas
        .originals
        .is_none_or(|art| keys.iter().any(|key| !art.sprites.contains_key(*key)))
    {
        return Ok(false);
    }
    let mut face = bounds;
    face[1] += canvas.r(BUTTON_DEPTH) * motion.press;
    let widths = [0.4, 0.4, 0.8 - BUTTON_DEPTH * motion.press, 0.4];
    canvas.nine_slice(keys[0], face, [2, 2, 4, 2], widths, true, [255; 4])?;
    if motion.hover > 0.0 {
        canvas.nine_slice(
            keys[1],
            face,
            [2, 2, 4, 2],
            widths,
            true,
            opacity([255; 4], motion.hover),
        )?;
    }
    if motion.focus > 0.0 {
        let outset = canvas.r(0.2) * motion.focus;
        let focused = [
            face[0] - outset,
            face[1] - outset,
            face[2] + outset,
            face[3] + outset,
        ];
        canvas.nine_slice(
            keys[2],
            focused,
            [3, 3, 5, 3],
            [0.6, 0.6, 1.0 - BUTTON_DEPTH * motion.press, 0.6],
            true,
            opacity([255; 4], motion.focus * (1.0 - motion.press)),
        )?;
    }
    if motion.press > 0.0 {
        canvas.nine_slice(
            keys[3],
            face,
            [2; 4],
            [0.4; 4],
            true,
            opacity([255; 4], motion.press),
        )?;
    }
    Ok(true)
}

pub(super) fn elevated(
    canvas: &mut Canvas<'_>,
    mut bounds: Bounds,
    variant: Variant,
    state: Interaction,
    enabled: bool,
) -> Result<bool, UiPresentationError> {
    if canvas.appearance == super::super::theme::Appearance::Dark
        && (!enabled || matches!(variant, Variant::Secondary | Variant::Neutral))
    {
        return Ok(false);
    }
    let selected = if !enabled {
        4
    } else if state.pressed {
        3
    } else if state.focused {
        2
    } else if state.hovered {
        1
    } else {
        0
    };
    let keys = artwork(variant);
    let slices = match selected {
        2 => {
            let outset = canvas.r(0.2);
            bounds = [
                bounds[0] - outset,
                bounds[1] - outset,
                bounds[2] + outset,
                bounds[3] + outset,
            ];
            [3, 3, 5, 3]
        }
        3 => {
            bounds[1] += canvas.r(BUTTON_DEPTH);
            [2, 2, 2, 2]
        }
        4 => [1, 1, 3, 1],
        _ => [2, 2, 4, 2],
    };
    let key = if selected == 4 {
        BUTTON_DISABLED_IMAGE
    } else {
        keys[selected]
    };
    canvas.nine_slice(
        key,
        bounds,
        slices,
        slices.map(|slice| f32::from(slice) / GUI_PIXELS_PER_REM),
        true,
        [255; 4],
    )
}
