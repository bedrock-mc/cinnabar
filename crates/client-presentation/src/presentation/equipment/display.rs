//! Composition of an item's display placement with the hand bone's pose.

use bevy::math::{Mat4, Quat, Vec3};
use render::RenderBoneTransform;
pub(super) use render::equipment::sprite_item_transform;
pub use render::equipment::{
    ItemDisplay, attach_to_bone, held_block_display, held_sprite_display, is_hand_equipped,
};

pub(super) const LAYER_MAIN_HAND: u8 = 1;
pub(super) const LAYER_OFF_HAND: u8 = 2;
pub(super) const LAYER_HELMET: u8 = 3;
pub(super) const LAYER_CHESTPLATE: u8 = 4;
pub(super) const LAYER_LEGGINGS: u8 = 5;
pub(super) const LAYER_BOOTS: u8 = 6;

fn degrees(value: f32) -> f32 {
    value.to_radians()
}

/// `ItemInHandRenderer::_applyDefaultItemTransforms` for a flat sprite in hand: the 1.5 scale
/// and tilt that seat vanilla's held-sprite mesh (`held_sprite_vertices`) in the grip.
fn item_default() -> Mat4 {
    sprite_item_transform()
}

/// How the first-person pass lays out a held item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FirstPersonShape {
    /// A flat sprite; `mirrored_art` items (rods on a stick) turn half a revolution.
    Sprite { mirrored_art: bool },
    /// A block's centred unit cube.
    Block,
}

/// The arm's state the first-person item follows, interpolated to the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FirstPersonHand {
    /// 0..1 swing progress.
    pub swing: f32,
    /// 0..1 equip progress.
    pub equip: f32,
    /// Ticks into an eat or drink use and its duration, while one runs.
    pub consume: Option<(f32, f32)>,
}

impl From<client_world::ItemAnimationState> for FirstPersonHand {
    fn from(state: client_world::ItemAnimationState) -> Self {
        Self {
            swing: state.attack_time,
            equip: state.arm_height,
            consume: None,
        }
    }
}

/// Camera-space placement of the first-person held item, from `renderFirstPerson`'s own item
/// transforms: the swing offset (or the eat/drink raise), the equip dip, the swing turns and
/// the 0.4 hand scale, then the item's default transforms.
pub(super) fn first_person_display(shape: FirstPersonShape, hand: FirstPersonHand) -> ItemDisplay {
    use std::f32::consts::PI;
    let swing = hand.swing;
    let (sine, root_sine) = ((swing * PI).sin(), (swing.sqrt() * PI).sin());
    let lead = match hand.consume {
        Some((elapsed, duration)) if duration > 0.0 => {
            let remaining = duration - elapsed + 1.0;
            let progress = 1.0 - remaining / duration;
            let bob = if progress > 0.2 {
                (remaining * 0.25 * PI).cos().abs() * 0.1
            } else {
                0.0
            };
            let raise = 1.0 - (1.0 - progress).clamp(0.0, 1.0).powi(27);
            Mat4::from_translation(Vec3::new(0.0, bob, 0.0))
                * Mat4::from_translation(Vec3::new(raise * 0.55, raise * -0.5, 0.0))
                * Mat4::from_rotation_y(degrees(raise * 90.0))
                * Mat4::from_rotation_x(degrees(raise * 10.0))
                * Mat4::from_rotation_z(degrees(raise * 30.0))
        }
        _ => Mat4::from_translation(Vec3::new(
            root_sine * -0.4,
            (swing.sqrt() * PI * 2.0).sin() * 0.2,
            sine * -0.2,
        )),
    };
    let held = lead
        * Mat4::from_translation(Vec3::new(0.56, -0.52, -0.72))
        * Mat4::from_translation(Vec3::new(0.0, (1.0 - hand.equip) * -0.6, 0.0))
        * Mat4::from_rotation_y(degrees(45.0))
        * Mat4::from_rotation_y(degrees((swing * swing * PI).sin() * -20.0))
        * Mat4::from_rotation_z(degrees(root_sine * -20.0))
        * Mat4::from_rotation_x(degrees(root_sine * -80.0))
        * Mat4::from_scale(Vec3::splat(0.4));
    ItemDisplay::from_matrix(match shape {
        FirstPersonShape::Sprite { mirrored_art } => {
            let turn = if mirrored_art {
                Mat4::from_rotation_y(PI)
            } else {
                Mat4::IDENTITY
            };
            held * turn * item_default()
        }
        FirstPersonShape::Block => held,
    })
}

/// A camera-space item bone from `display`; `None` for a non-finite placement.
pub fn view_bone(display: ItemDisplay) -> Option<RenderBoneTransform> {
    let bone = RenderBoneTransform {
        rotation: display.rotation.to_array(),
        translation_scale: [
            display.translation.x,
            display.translation.y,
            display.translation.z,
            display.scale,
        ],
        axis_scale: render::UNIT_AXIS_SCALE,
    };
    bone.is_finite().then_some(bone)
}

/// A block worn on the head: a cube just larger than the head, centred on it. Provisional.
pub(super) fn head_block_display() -> ItemDisplay {
    ItemDisplay {
        rotation: Quat::IDENTITY,
        translation: Vec3::new(0.0, 0.25, 0.0),
        scale: 0.5625,
    }
}

/// Items whose icon vanilla turns half a revolution in first person (`isMirroredArt`).
pub(super) fn is_mirrored_art(identifier: &str) -> bool {
    matches!(
        identifier.strip_prefix("minecraft:").unwrap_or(identifier),
        "fishing_rod" | "carrot_on_a_stick" | "warped_fungus_on_a_stick"
    )
}
