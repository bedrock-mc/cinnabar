//! Shared item placement for world equipment and inventory previews.

use crate::RenderBoneTransform;
use glam::{Mat3, Mat4, Quat, Vec3};

/// Item-space to hand-bone placement: rotation, translation in blocks and uniform scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ItemDisplay {
    pub rotation: Quat,
    pub translation: Vec3,
    pub scale: f32,
}

impl ItemDisplay {
    /// Decomposes a rotation + uniform scale + translation matrix.
    pub fn from_matrix(matrix: Mat4) -> Self {
        let linear = Mat3::from_mat4(matrix);
        let scale = linear.determinant().cbrt();
        Self {
            rotation: Quat::from_mat3(&(linear * scale.recip())).normalize(),
            translation: matrix.w_axis.truncate(),
            scale,
        }
    }
}

/// Converts authored degrees to the radians used by matrix rotations.
fn degrees(value: f32) -> f32 {
    value.to_radians()
}

/// The reference's hand-bone frame (Y and X negated) turned into the rig bone frame.
fn rig_from_reference_bone() -> Mat4 {
    Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
}

/// Third-person main-hand placement of a flat sprite item on the `rightItem` bone, from the
/// 26.30 reference's held-item and default item transforms. `hand_equipped` items (tools,
/// weapons, rods) are held upright like a sword.
pub fn held_sprite_display(hand_equipped: bool) -> ItemDisplay {
    held_sprite_display_for_hand(hand_equipped, false)
}

/// Vanilla's off-hand item has its own bone-frame offset and
/// hand-equipped X translation, rather than reflecting the main-hand grip.
pub fn held_sprite_display_for_hand(hand_equipped: bool, off_hand: bool) -> ItemDisplay {
    let grip = if hand_equipped {
        Mat4::from_rotation_y(degrees(180.0))
            * Mat4::from_translation(Vec3::new(if off_hand { 0.0 } else { 0.1 }, 0.265, 0.0))
            * Mat4::from_scale(Vec3::splat(0.625))
            * Mat4::from_rotation_x(degrees(80.0))
            * Mat4::from_rotation_y(degrees(45.0))
    } else {
        Mat4::from_translation(Vec3::new(0.3125, 0.1875, -0.1875))
            * Mat4::from_scale(Vec3::splat(0.375))
            * Mat4::from_rotation_z(degrees(60.0))
            * Mat4::from_rotation_x(degrees(-90.0))
            * Mat4::from_rotation_z(degrees(20.0))
    };
    let hand_offset = if off_hand {
        Mat4::from_translation(Vec3::new(-0.125, 0.0, 0.0))
    } else {
        Mat4::IDENTITY
    };
    ItemDisplay::from_matrix(
        rig_from_reference_bone() * hand_offset * grip * sprite_item_transform(),
    )
}

/// Legacy icon transform shared by the item renderer, before its per-view placement.
pub fn sprite_item_transform() -> Mat4 {
    Mat4::from_scale(Vec3::splat(1.5))
        * Mat4::from_rotation_y(degrees(50.0))
        * Mat4::from_rotation_z(degrees(335.0))
        * Mat4::from_translation(Vec3::new(0.075, -0.245, -0.1))
}

/// The item's single bone: the hand bone's pose with `display` applied in the hand frame, so
/// item-space vertices (bind pivot at the origin) land where the hand holds them. `None` for a
/// non-finite pose.
pub fn attach_to_bone(
    hand: RenderBoneTransform,
    display: ItemDisplay,
) -> Option<RenderBoneTransform> {
    let [rx, ry, rz, rw] = hand.rotation;
    let hand_rotation = Quat::from_vec4(glam::Vec4::new(rx, ry, rz, rw).try_normalize()?);
    // A non-uniform hand scale would shear the item; the first axis stands in for it.
    let hand_scale = hand.translation_scale[3] * hand.axis_scale[0];
    let origin = Vec3::new(
        hand.translation_scale[0],
        hand.translation_scale[1],
        hand.translation_scale[2],
    );
    let rotation = (hand_rotation * display.rotation).normalize();
    let translation = origin + hand_rotation * (display.translation * hand_scale);
    let bone = RenderBoneTransform {
        rotation: rotation.to_array(),
        translation_scale: [
            translation.x,
            translation.y,
            translation.z,
            hand_scale * display.scale,
        ],
        axis_scale: crate::UNIT_AXIS_SCALE,
    };
    bone.is_finite().then_some(bone)
}

/// Legacy block grip, applied to the centred cube vanilla emits through
/// the (-.5,-.5,-.5) mesh offset.
pub fn held_block_display() -> ItemDisplay {
    held_block_display_for_hand(false)
}

/// Offhand rendering adds its own reference-bone X offset before the shared
/// legacy block display. Custom block presentations use a different native path.
pub fn held_block_display_for_hand(off_hand: bool) -> ItemDisplay {
    let hand_offset = if off_hand {
        Mat4::from_translation(Vec3::new(-0.125, 0.0, 0.0))
    } else {
        Mat4::IDENTITY
    };
    ItemDisplay::from_matrix(
        rig_from_reference_bone()
            * hand_offset
            * Mat4::from_translation(Vec3::new(0.0, 0.1875, -0.3125))
            * Mat4::from_rotation_x(degrees(200.0))
            * Mat4::from_rotation_y(degrees(225.0))
            * Mat4::from_scale(Vec3::splat(0.375)),
    )
}

/// Rods on a stick, whose art both editions turn half a revolution in first person.
pub fn is_rod(identifier: &str) -> bool {
    matches!(
        identifier.strip_prefix("minecraft:").unwrap_or(identifier),
        "fishing_rod" | "carrot_on_a_stick" | "warped_fungus_on_a_stick"
    )
}

/// Items vanilla holds upright: tools, weapons and rod-like items. The
/// reference keeps this per item in code; the list mirrors vanilla's hand-equipped items.
pub fn is_hand_equipped(identifier: &str) -> bool {
    let name = identifier.strip_prefix("minecraft:").unwrap_or(identifier);
    ["_sword", "_axe", "_pickaxe", "_shovel", "_hoe", "_spear"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
        || matches!(
            name,
            "stick"
                | "bone"
                | "blaze_rod"
                | "breeze_rod"
                | "fishing_rod"
                | "carrot_on_a_stick"
                | "warped_fungus_on_a_stick"
                | "mace"
                | "debug_stick"
        )
}
