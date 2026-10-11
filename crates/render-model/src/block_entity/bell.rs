//! Bell body, lip and supporting frame; the body swings after a ring cue.
//!
//! Body and lip boxes come from the bell texture's unwrap; the frame geometry, its textures
//! and the swing curve are provisional and need native measurement.

use glam::{Mat4, Vec3};

use super::{
    atlas::BlockEntityAtlas,
    mesh::{BoxSpec, Layer, MeshBuilder, WHITE, model_matrix},
};

/// Swing amplitude right after a ring, in degrees.
const SWING_DEGREES: f32 = 30.0;
const SWING_HERTZ: f32 = 1.5;
const SWING_DECAY_SECONDS: f32 = 1.0;
/// Height of the bell's pivot on the frame bar, in pixels.
const PIVOT_HEIGHT: f32 = 13.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BellAttachment {
    Standing,
    Hanging,
    Side,
    Multiple,
}

impl BellAttachment {
    #[must_use]
    pub fn from_state(name: &str) -> Option<Self> {
        Some(match name {
            "standing" => Self::Standing,
            "hanging" => Self::Hanging,
            "side" => Self::Side,
            "multiple" => Self::Multiple,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BellModel {
    pub attachment: BellAttachment,
    /// Bedrock `direction` state; odd values run the frame bar along Z.
    pub direction: u8,
    /// Seconds since the last ring cue; large values mean at rest.
    pub seconds_since_ring: f32,
}

/// Swing angle in degrees `seconds` after a ring: a decaying sine.
#[must_use]
pub fn swing_degrees(seconds: f32) -> f32 {
    if !seconds.is_finite() || seconds < 0.0 {
        return 0.0;
    }
    SWING_DEGREES
        * (-seconds / SWING_DECAY_SECONDS).exp()
        * (std::f32::consts::TAU * SWING_HERTZ * seconds).sin()
}

pub(super) fn emit(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    block: [i32; 3],
    model: &BellModel,
) {
    // Frame bar axis: X for even directions, Z for odd; the model turns a quarter for Z.
    let yaw = if model.direction.is_multiple_of(2) {
        0.0
    } else {
        90.0
    };
    let base = model_matrix(block, [0.5, 0.0, 0.5], yaw);
    emit_frame(builder, atlas, base, model.attachment);
    let Some(texture) = atlas.texture("textures/entity/bell/bell", [32.0, 32.0]) else {
        return;
    };
    // The body swings about the bar, so it tilts in the plane across the bar.
    let pivot = Vec3::new(0.0, PIVOT_HEIGHT, 0.0);
    let swing = base
        * Mat4::from_translation(pivot)
        * Mat4::from_rotation_x(swing_degrees(model.seconds_since_ring).to_radians())
        * Mat4::from_translation(-pivot);
    builder.cuboid(
        Layer::Solid,
        &texture,
        swing,
        BoxSpec::new([-3.0, 6.0, -3.0], [6.0, 7.0, 6.0], [0.0, 0.0]),
        WHITE,
    );
    builder.cuboid(
        Layer::Solid,
        &texture,
        swing,
        BoxSpec::new([-4.0, 4.0, -4.0], [8.0, 2.0, 8.0], [0.0, 13.0]),
        WHITE,
    );
}

fn emit_frame(
    builder: &mut MeshBuilder,
    atlas: &BlockEntityAtlas,
    base: Mat4,
    attachment: BellAttachment,
) {
    let (Some(side), Some(top), Some(bottom)) = (
        atlas.texture("textures/blocks/bell_side", [16.0, 16.0]),
        atlas.texture("textures/blocks/bell_top", [16.0, 16.0]),
        atlas.texture("textures/blocks/bell_bottom", [16.0, 16.0]),
    ) else {
        return;
    };
    let rects = [
        side.rect,
        side.rect,
        bottom.rect,
        top.rect,
        side.rect,
        side.rect,
    ];
    // Boxes as `[min, max]` in pixels about the block center, bar along X.
    let bar = |from: f32, to: f32| [[from, 13.0, -1.0], [to, 15.0, 1.0]];
    let post = |x: f32| [[x, 0.0, -1.0], [x + 2.0, 13.0, 1.0]];
    let boxes: Vec<[[f32; 3]; 2]> = match attachment {
        BellAttachment::Standing => vec![bar(-8.0, 8.0), post(-8.0), post(6.0)],
        BellAttachment::Hanging => vec![[[-1.0, 13.0, -1.0], [1.0, 16.0, 1.0]]],
        BellAttachment::Side => vec![bar(-8.0, 1.0)],
        BellAttachment::Multiple => vec![bar(-8.0, 8.0)],
    };
    for [min, max] in boxes {
        builder.tile_cuboid(Layer::Solid, base, min, max, rects, WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swing_starts_at_rest_decays_and_ignores_bad_times() {
        assert_eq!(swing_degrees(0.0), 0.0);
        let early = (0..100)
            .map(|step| swing_degrees(step as f32 * 0.01).abs())
            .fold(0.0, f32::max);
        let late = (0..100)
            .map(|step| swing_degrees(4.0 + step as f32 * 0.01).abs())
            .fold(0.0, f32::max);
        assert!(early > 10.0 && late < 1.0);
        assert_eq!(swing_degrees(f32::NAN), 0.0);
        assert_eq!(swing_degrees(-1.0), 0.0);
    }

    #[test]
    fn attachment_names_parse() {
        assert_eq!(
            BellAttachment::from_state("multiple"),
            Some(BellAttachment::Multiple)
        );
        assert_eq!(BellAttachment::from_state("floor"), None);
    }
}
