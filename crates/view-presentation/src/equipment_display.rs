//! Composition of an item's display placement with the hand bone's pose.

use bevy::math::{Mat4, Quat, Vec3};
use render_model::RenderBoneTransform;
use render_model::equipment::ItemDisplay;
use render_model::equipment::sprite_item_transform;

pub const LAYER_MAIN_HAND: u8 = 1;
pub const LAYER_OFF_HAND: u8 = 2;
pub const LAYER_HELMET: u8 = 3;
pub const LAYER_CHESTPLATE: u8 = 4;
pub const LAYER_LEGGINGS: u8 = 5;
pub const LAYER_BOOTS: u8 = 6;

fn degrees(value: f32) -> f32 {
    value.to_radians()
}

/// Vanilla's default in-hand transform for a flat sprite: the 1.5 scale
/// and tilt that seat vanilla's held-sprite mesh (`held_sprite_vertices`) in the grip.
fn item_default() -> Mat4 {
    sprite_item_transform()
}

/// How the first-person pass lays out a held item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirstPersonShape {
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

/// Camera-space placement of the first-person held item, from vanilla's first-person item
/// transforms: the swing offset (or the eat/drink raise), the equip dip, the swing turns and
/// the 0.4 hand scale, then the item's default transforms.
pub fn first_person_display(shape: FirstPersonShape, hand: FirstPersonHand) -> ItemDisplay {
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
        axis_scale: render_model::UNIT_AXIS_SCALE,
    };
    bone.is_finite().then_some(bone)
}

/// A block worn on the head: a cube just larger than the head, centred on it. Provisional.
pub fn head_block_display() -> ItemDisplay {
    ItemDisplay {
        rotation: Quat::IDENTITY,
        translation: Vec3::new(0.0, 0.25, 0.0),
        scale: 0.5625,
    }
}

/// Which first-person arms the player render controller shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirstPersonArms {
    pub right: bool,
    pub left: bool,
}

const FILLED_MAP: &str = "minecraft:filled_map";
const SHIELD: &str = "minecraft:shield";

impl FirstPersonArms {
    /// The right arm shows for an empty hand or a map; the left for a map in either hand (a shield
    /// in the off hand keeps it hidden). The use-item conditions await the item-use queries.
    pub fn for_hands(main: Option<&str>, off: Option<&str>) -> Self {
        Self {
            right: main.is_none_or(|main| main == FILLED_MAP),
            left: (main == Some(FILLED_MAP) && off != Some(SHIELD)) || off == Some(FILLED_MAP),
        }
    }

    /// Shows the right arm when the main-hand item drew nothing, so the rig still swings.
    pub fn with_undrawn_main(self, main_drawn: bool) -> Self {
        Self {
            right: self.right || !main_drawn,
            ..self
        }
    }
}

/// Places a rig-frame model, which faces -Z with its right side at +X, so it faces the
/// Minecraft `yaw_degrees` direction at `position`, scaled about the feet.
pub fn rig_world_from_actor(position: [f32; 3], yaw_degrees: f32, scale: f32) -> [[f32; 4]; 3] {
    let (sine, cosine) = yaw_degrees.to_radians().sin_cos();
    [
        [-cosine * scale, 0.0, sine * scale, position[0]],
        [0.0, scale, 0.0, position[1]],
        [-sine * scale, 0.0, -cosine * scale, position[2]],
    ]
}

/// Red damage overlay over the authoritative actor fade alpha.
pub fn hurt_overlay_rgba(alpha: f32) -> [f32; 4] {
    [1.0, 0.0, 0.0, alpha]
}

/// Zero-yaw first-person rig placement, including its native model-space lift.
pub fn hand_camera_from_rig(scale: f32, eye_height: f32, motion: Mat4) -> [[f32; 4]; 3] {
    let rows = rig_world_from_actor([0.0, -eye_height, 0.0], 0.0, scale);
    let placement = Mat4::from_cols_array_2d(&[
        [rows[0][0], rows[1][0], rows[2][0], 0.0],
        [rows[0][1], rows[1][1], rows[2][1], 0.0],
        [rows[0][2], rows[1][2], rows[2][2], 0.0],
        [rows[0][3], rows[1][3], rows[2][3], 1.0],
    ]);
    hand_view_placement(motion * placement * Mat4::from_translation(Vec3::Y / 128.0))
}

/// Camera-space item instance rows.
pub fn hand_view_placement(matrix: Mat4) -> [[f32; 4]; 3] {
    let rows = matrix.transpose().to_cols_array_2d();
    [rows[0], rows[1], rows[2]]
}

/// Visible native arm and sleeve bones, preserving compiled pose order.
pub fn mask_first_person_bones(
    names: &[Box<str>],
    pose: &[RenderBoneTransform],
    arms: FirstPersonArms,
) -> Vec<RenderBoneTransform> {
    let visible = |name: &str| {
        let is = |wanted: &str| name.eq_ignore_ascii_case(wanted);
        (arms.right && (is("rightArm") || is("rightSleeve")))
            || (arms.left && (is("leftArm") || is("leftSleeve")))
    };
    pose.iter()
        .zip(names)
        .map(|(bone, name)| {
            if visible(name) {
                *bone
            } else {
                crate::armor_pose::hidden_bone()
            }
        })
        .collect()
}

/// Per-tick hand facts admitted by the caller's animation owner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HandProgress {
    pub attack_time: f32,
    pub arm_height: f32,
    pub use_ticks: u32,
}

pub fn hand_progress(
    hand: [HandProgress; 2],
    consume_ticks: Option<u32>,
    partial_tick: f32,
) -> FirstPersonHand {
    let [previous, current] = hand;
    let mut swing = current.attack_time - previous.attack_time;
    if swing < 0.0 {
        swing += 1.0;
    }
    FirstPersonHand {
        swing: previous.attack_time + swing * partial_tick,
        equip: previous.arm_height + (current.arm_height - previous.arm_height) * partial_tick,
        consume: consume_ticks
            .filter(|_| current.use_ticks > 0)
            .map(|ticks| (current.use_ticks as f32 - 1.0 + partial_tick, ticks as f32)),
    }
}
