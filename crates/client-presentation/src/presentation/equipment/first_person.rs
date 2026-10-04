//! Camera-space ordinary items, separate from attachables on the player's arm skeleton.
//!
//! The mesh uses the native icon basis (X in [-1, 0], Y in [0, 1]); centering or
//! mirroring it again would duplicate the correction already in held_sprite_vertices.

use bevy::math::{Mat4, Vec3};
use client_world::ItemAnimationState;
use render::RenderBoneTransform;

use super::display::{FirstPersonShape, ItemDisplay, first_person_display, view_bone};

#[cfg(test)]
mod tests;

/// Native TextureTessellator pixel-to-model conversion (current PE VA14ffa90e0).
const NATIVE_ICON_TEXELS_PER_UNIT: u16 = 16;

#[cfg(test)]
const FIRST_PERSON_ITEM_SCALE: f32 = 0.4;
#[cfg(test)]
const CAMERA_ANCHOR: Vec3 = Vec3::new(0.56, -0.52, -0.72);

/// Current native camera stack followed by legacy icon placement.
pub(super) fn sprite_pose(state: ItemAnimationState) -> Option<RenderBoneTransform> {
    pose(
        state,
        FirstPersonShape::Sprite {
            mirrored_art: false,
        },
    )
}

/// Presentation-mode-1 correction and default display matrix cancel the block Y turns.
pub(super) fn block_pose(state: ItemAnimationState) -> Option<RenderBoneTransform> {
    pose(state, FirstPersonShape::Block)
}

/// Native renderOffhandItem, not the main-hand swing stack. Blocks use
/// the default presentation-type-2 matrix; sprites here have a square native icon.
pub(super) fn offhand_pose(hand_equipped: bool, block: bool) -> Option<RenderBoneTransform> {
    if block {
        // The type-2 default has Y=-135; icon placement negates that component
        // for the left-hand presentation. Native block geometry is already centered.
        return matrix_bone(
            Mat4::from_translation(Vec3::new(-0.56, -0.52, -0.72))
                * Mat4::from_rotation_y(135.0_f32.to_radians())
                * Mat4::from_scale(Vec3::splat(0.4)),
        );
    }
    offhand_sprite_pose(hand_equipped, [NATIVE_ICON_TEXELS_PER_UNIT; 2])
}

/// The same native offhand sprite route with the actual icon dimensions. Flat items
/// normalize to their longer side; hand-equipped legacy items retain the native pixel size.
pub(super) fn offhand_sprite_pose(
    hand_equipped: bool,
    [width, height]: [u16; 2],
) -> Option<RenderBoneTransform> {
    if width == 0 || height == 0 {
        return None;
    }
    let long_side = f32::from(width.max(height));
    let height = f32::from(height) / long_side;
    // TextureTessellator writes positive column X, depth Y,
    // row Z. held_sprite_vertices stores [-column, height-row, -depth]/long_side.
    // This proper rotation + translation preserves the UV-labelled front/back corners.
    let native_from_held = Mat4::from_translation(Vec3::new(0.0, 0.0, height))
        * Mat4::from_rotation_y(std::f32::consts::PI)
        * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);
    let placement = if hand_equipped {
        Mat4::from_translation(Vec3::new(-0.6875, -0.125, -1.53125))
            * Mat4::from_rotation_y(-10.0_f32.to_radians())
            * Mat4::from_rotation_x(70.0_f32.to_radians())
            * Mat4::from_rotation_z(80.0_f32.to_radians())
            * Mat4::from_scale(Vec3::splat(
                long_side / f32::from(NATIVE_ICON_TEXELS_PER_UNIT),
            ))
    } else {
        // S(1/16)*S(16/long_side) was absorbed by the held mesh normalization.
        Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2)
            * Mat4::from_translation(Vec3::new(-1.25, -1.125, 0.0))
            * Mat4::from_rotation_z(-80.0_f32.to_radians())
            * Mat4::from_rotation_y(-20.0_f32.to_radians())
            * Mat4::from_translation(Vec3::new(-0.3125, 0.25, -0.03125))
    };
    matrix_bone(placement * native_from_held)
}

fn matrix_bone(matrix: Mat4) -> Option<RenderBoneTransform> {
    let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
    view_bone(ItemDisplay {
        rotation: rotation.normalize(),
        translation,
        scale: scale.x,
    })
}

fn pose(state: ItemAnimationState, shape: FirstPersonShape) -> Option<RenderBoneTransform> {
    if !state.attack_time.is_finite() || !state.arm_height.is_finite() {
        return None;
    }
    view_bone(first_person_display(
        shape,
        ItemAnimationState {
            attack_time: state.attack_time.clamp(0.0, 1.0),
            arm_height: state.arm_height.clamp(0.0, 1.0),
        }
        .into(),
    ))
}
