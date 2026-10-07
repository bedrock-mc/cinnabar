//! Pressable artwork uses the installed state's native border-image slices.

use super::super::motion::{Feedback, opacity};
use super::{Bounds, Canvas, Interaction, UiPresentationError, Variant};

#[cfg(test)]
mod tests;

const DISABLED_IMAGE: &str = "assets/pressable_elevated_disabled-d849d4185836b2229b5a.png";

fn artwork(variant: Variant) -> [&'static str; 4] {
    match variant {
        Variant::Hero | Variant::Primary => [
            "assets/pressable_elevated_primary_default-7a6e2edf98a626b182c2.png",
            "assets/pressable_elevated_primary_hovered-bcb21ab092446cbcecb6.png",
            "assets/pressable_elevated_primary_focused-cc7df0bf833f92bb08dd.png",
            "assets/pressable_primary_pressed-13b0f01c6c00e07322ca.png",
        ],
        Variant::Secondary => [
            "assets/pressable_elevated_secondary_default-0aacffbfa726a8184fc5.png",
            "assets/pressable_elevated_secondary_hovered-758970b376da9eb299da.png",
            "assets/pressable_elevated_secondary_focused-05d59cd4654230c9b01d.png",
            "assets/pressable_secondary_pressed-87bba1faba891ebfa7fc.png",
        ],
        Variant::Neutral => [
            "assets/pressable_elevated_neutral_default-48cdf8535bfd48b47c2a.png",
            "assets/pressable_elevated_neutral_hovered-dcbd873ad9e83d24dc7a.png",
            "assets/pressable_elevated_neutral_focused-b28fc482cadde858740c.png",
            "assets/pressable_neutral_pressed-aad8b64fc0f155676218.png",
        ],
        Variant::Destructive => [
            "assets/pressable_elevated_destructive_default-a5fb3f173720065fcc13.png",
            "assets/pressable_elevated_destructive_hovered-63736c555d7fc6479755.png",
            "assets/pressable_elevated_destructive_focused-cfa496a1b2e3a06a9db4.png",
            "assets/pressable_destructive_pressed-d5d4943d1b774ef42f85.png",
        ],
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
    face[1] += canvas.r(0.4) * motion.press;
    let widths = [0.4, 0.4, 0.8 - 0.4 * motion.press, 0.4];
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
            [0.6, 0.6, 1.0 - 0.4 * motion.press, 0.6],
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
            bounds[1] += canvas.r(0.4);
            [2, 2, 2, 2]
        }
        4 => [1, 1, 3, 1],
        _ => [2, 2, 4, 2],
    };
    let key = if selected == 4 {
        DISABLED_IMAGE
    } else {
        keys[selected]
    };
    canvas.nine_slice(
        key,
        bounds,
        slices,
        slices.map(|slice| f32::from(slice) / 5.0),
        true,
        [255; 4],
    )
}
