//! Java Edition 1.7 player animation: biped angles, first- and third-person held-item stacks,
//! expressed in our rig and camera frames. Rules, frames and constants:
//! docs/reference/java-1-7-animations.md.

use glam::{Mat4, Quat, Vec3, Vec4};
use std::f32::consts::PI;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod reference_tests;

/// Java's table sine: the angle snapped down to a 65536th of a turn.
#[must_use]
pub fn java_sin(radians: f32) -> f32 {
    sine_table(radians * 10430.378)
}

/// Java's table cosine, a quarter turn ahead of [`java_sin`].
#[must_use]
pub fn java_cos(radians: f32) -> f32 {
    sine_table(radians * 10430.378 + 16384.0)
}

fn sine_table(index: f32) -> f32 {
    let index = (index as i32) & 0xFFFF;
    (f64::from(index) * std::f64::consts::TAU / 65536.0).sin() as f32
}

fn sqrt(value: f32) -> f32 {
    f64::from(value).sqrt() as f32
}

/// Java's model space to our rig frame, both in blocks: X and Y negate about the neck origin.
#[must_use]
pub fn rig_from_java_model() -> Mat4 {
    Mat4::from_translation(Vec3::Y * NECK_Y / 16.0) * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
}

const NECK_Y: f32 = assets::gui_item::SHIELD_MODEL_PART_HEIGHT;

/// One biped part: its rotation point in Java model pixels and Z·Y·X angles in radians.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct JavaPart {
    pub point: Vec3,
    pub angles: Vec3,
}

impl JavaPart {
    const fn at(x: f32, y: f32, z: f32) -> Self {
        Self {
            point: Vec3::new(x, y, z),
            angles: Vec3::ZERO,
        }
    }

    /// The part's model-space bone in our rig frame (pixels): the rig's own pivot moved by this
    /// part's rotation-point offset from `rest`, and the angles conjugated by the X/Y flip.
    #[must_use]
    pub fn rig_bone(self, rest: Self, rig_pivot: Vec3) -> (Quat, Vec3) {
        let offset = self.point - rest.point;
        let rotation = Quat::from_rotation_z(self.angles.z)
            * Quat::from_rotation_y(-self.angles.y)
            * Quat::from_rotation_x(-self.angles.x);
        (
            rotation,
            rig_pivot + Vec3::new(-offset.x, -offset.y, offset.z),
        )
    }
}

/// The posed parts; the hat follows the head.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JavaBiped {
    pub head: JavaPart,
    pub body: JavaPart,
    pub right_arm: JavaPart,
    pub left_arm: JavaPart,
    pub right_leg: JavaPart,
    pub left_leg: JavaPart,
}

impl JavaBiped {
    /// Rotation points as the model is built, before any pose.
    pub const REST: Self = Self {
        head: JavaPart::at(0.0, 0.0, 0.0),
        body: JavaPart::at(0.0, 0.0, 0.0),
        right_arm: JavaPart::at(-5.0, 2.0, 0.0),
        left_arm: JavaPart::at(5.0, 2.0, 0.0),
        right_leg: JavaPart::at(-1.9, 12.0, 0.0),
        left_leg: JavaPart::at(1.9, 12.0, 0.0),
    };

    /// Each part with its rest counterpart, in field order.
    #[must_use]
    pub fn parts(&self) -> [(JavaPart, JavaPart); 6] {
        let rest = Self::REST;
        [
            (self.head, rest.head),
            (self.body, rest.body),
            (self.right_arm, rest.right_arm),
            (self.left_arm, rest.left_arm),
            (self.right_leg, rest.right_leg),
            (self.left_leg, rest.left_leg),
        ]
    }
}

/// Pose inputs at the frame: interpolated limb swing, age in ticks and the head relative to
/// the body in degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct JavaBipedInput {
    pub limb_swing: f32,
    pub limb_amount: f32,
    pub age: f32,
    pub head_yaw: f32,
    pub head_pitch: f32,
    /// 0..1 attack swing.
    pub swing: f32,
    pub sneaking: bool,
    pub riding: bool,
    /// 1 holding anything, 3 blocking with a sword.
    pub held_right: u8,
    pub aimed_bow: bool,
}

/// The biped pose for `input`, assignment for assignment as Java poses it.
#[must_use]
pub fn java_biped(input: &JavaBipedInput) -> JavaBiped {
    let JavaBipedInput {
        limb_swing: swing_phase,
        limb_amount: amount,
        age,
        ..
    } = *input;
    let degrees = 180.0 / PI;
    let mut pose = JavaBiped::REST;
    let (head, body) = (&mut pose.head, &mut pose.body);
    head.angles.y = input.head_yaw / degrees;
    head.angles.x = input.head_pitch / degrees;
    let (right_arm, left_arm) = (&mut pose.right_arm, &mut pose.left_arm);
    right_arm.angles.x = java_cos(swing_phase * 0.6662 + PI) * 2.0 * amount * 0.5;
    left_arm.angles.x = java_cos(swing_phase * 0.6662) * 2.0 * amount * 0.5;
    let (right_leg, left_leg) = (&mut pose.right_leg, &mut pose.left_leg);
    right_leg.angles.x = java_cos(swing_phase * 0.6662) * 1.4 * amount;
    left_leg.angles.x = java_cos(swing_phase * 0.6662 + PI) * 1.4 * amount;
    if input.riding {
        right_arm.angles.x += (-std::f64::consts::PI / 5.0) as f32;
        left_arm.angles.x += (-std::f64::consts::PI / 5.0) as f32;
        right_leg.angles.x = (-std::f64::consts::PI * 2.0 / 5.0) as f32;
        left_leg.angles.x = (-std::f64::consts::PI * 2.0 / 5.0) as f32;
        right_leg.angles.y = (std::f64::consts::PI / 10.0) as f32;
        left_leg.angles.y = (-std::f64::consts::PI / 10.0) as f32;
    }
    let tenth = (std::f64::consts::PI / 10.0) as f32;
    if input.held_right != 0 {
        right_arm.angles.x = right_arm.angles.x * 0.5 - tenth * f32::from(input.held_right);
    }
    let attack = input.swing;
    body.angles.y = java_sin(sqrt(attack) * PI * 2.0) * 0.2;
    let body_yaw = body.angles.y;
    right_arm.point.z = java_sin(body_yaw) * 5.0;
    right_arm.point.x = -java_cos(body_yaw) * 5.0;
    left_arm.point.z = -java_sin(body_yaw) * 5.0;
    left_arm.point.x = java_cos(body_yaw) * 5.0;
    right_arm.angles.y = body_yaw;
    left_arm.angles.y = body_yaw;
    left_arm.angles.x += body_yaw;
    let mut eased = 1.0 - attack;
    eased *= eased;
    eased *= eased;
    eased = 1.0 - eased;
    let lift = java_sin(eased * PI);
    let reach = java_sin(attack * PI) * -(head.angles.x - 0.7) * 0.75;
    right_arm.angles.x =
        (f64::from(right_arm.angles.x) - (f64::from(lift) * 1.2 + f64::from(reach))) as f32;
    right_arm.angles.y += body_yaw * 2.0;
    right_arm.angles.z = java_sin(attack * PI) * -0.4;
    if input.sneaking {
        body.angles.x = 0.5;
        right_arm.angles.x += 0.4;
        left_arm.angles.x += 0.4;
        right_leg.point.z = 4.0;
        left_leg.point.z = 4.0;
        right_leg.point.y = 9.0;
        left_leg.point.y = 9.0;
        head.point.y = 1.0;
    } else {
        right_leg.point.z = 0.1;
        left_leg.point.z = 0.1;
    }
    let idle_roll = java_cos(age * 0.09) * 0.05 + 0.05;
    let idle_pitch = java_sin(age * 0.067) * 0.05;
    right_arm.angles.z += idle_roll;
    left_arm.angles.z -= idle_roll;
    right_arm.angles.x += idle_pitch;
    left_arm.angles.x -= idle_pitch;
    if input.aimed_bow {
        right_arm.angles.y = -0.1 + head.angles.y;
        left_arm.angles.y = 0.1 + head.angles.y + 0.4;
        right_arm.angles.x = (-std::f64::consts::FRAC_PI_2) as f32 + head.angles.x;
        left_arm.angles.x = (-std::f64::consts::FRAC_PI_2) as f32 + head.angles.x;
        right_arm.angles.z = idle_roll;
        left_arm.angles.z = -idle_roll;
        right_arm.angles.x += idle_pitch;
        left_arm.angles.x -= idle_pitch;
    }
    pose
}

/// GL-style stack: every call post-multiplies, so calls read in Java's order.
#[derive(Clone, Copy)]
struct Stack(Mat4);

impl Stack {
    fn translate(&mut self, x: f32, y: f32, z: f32) {
        self.0 *= Mat4::from_translation(Vec3::new(x, y, z));
    }
    fn rotate_x(&mut self, degrees: f32) {
        self.0 *= Mat4::from_rotation_x(degrees.to_radians());
    }
    fn rotate_y(&mut self, degrees: f32) {
        self.0 *= Mat4::from_rotation_y(degrees.to_radians());
    }
    fn rotate_z(&mut self, degrees: f32) {
        self.0 *= Mat4::from_rotation_z(degrees.to_radians());
    }
    fn scale(&mut self, x: f32, y: f32, z: f32) {
        self.0 *= Mat4::from_scale(Vec3::new(x, y, z));
    }
}

/// How the held item's mesh is drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JavaItemMesh {
    /// Our held sprite slab (`held_sprite_vertices`).
    Sprite,
    /// Our centred unit block cube.
    Block,
    /// A raster in image columns/rows with extrusion depth normalized to 0..1.
    Raster { width: u16, height: u16 },
}

/// Places our mesh where Java's item draw puts its own: the flat item's tilt over its mirrored
/// unit slab, or the cube's quarter turn.
fn draw_item(mut stack: Stack, mesh: JavaItemMesh) -> Mat4 {
    if mesh == JavaItemMesh::Block {
        stack.rotate_y(90.0);
        return stack.0;
    }
    stack.translate(0.0, -0.3, 0.0);
    stack.scale(1.5, 1.5, 1.5);
    stack.rotate_y(50.0);
    stack.rotate_z(335.0);
    stack.translate(-0.9375, -0.0625, 0.0);
    match mesh {
        // Our held slab sits one unit to -X of Java's.
        JavaItemMesh::Sprite => stack.translate(1.0, 0.0, 0.0),
        // Column u sits at 1 - u and row v at 1 - v on Java's slab, one sixteenth deep.
        JavaItemMesh::Raster { width, height } => {
            stack.0 *= Mat4::from_cols(
                Vec4::new(-1.0 / f32::from(width), 0.0, 0.0, 0.0),
                Vec4::new(0.0, 0.0, -1.0 / 16.0, 0.0),
                Vec4::new(0.0, -1.0 / f32::from(height), 0.0, 0.0),
                Vec4::new(1.0, 1.0, 0.0, 1.0),
            );
        }
        JavaItemMesh::Block => {}
    }
    stack.0
}

/// An item use in progress, with Java's timing at the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JavaUse {
    /// Eat or drink: in-use count plus one less the frame fraction, and the use duration.
    Consume { remaining: f32, duration: f32 },
    /// Bow draw ticks at the frame.
    Bow { pull: f32 },
    /// Sword block.
    Block,
}

/// The first-person hand at the frame: interpolated swing and equip progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JavaHand {
    pub swing: f32,
    pub equip: f32,
    pub using: Option<JavaUse>,
}

/// Camera-space placement of the held item mesh. `rotate_around` items (rods) turn half a
/// revolution before drawing.
#[must_use]
pub fn first_person_item(hand: JavaHand, mesh: JavaItemMesh, rotate_around: bool) -> Mat4 {
    let mut stack = Stack(Mat4::IDENTITY);
    let swing = hand.swing;
    match hand.using {
        Some(JavaUse::Consume {
            remaining,
            duration,
        }) => {
            let progress = 1.0 - remaining / duration;
            let mut rest = 1.0 - progress;
            rest = rest * rest * rest;
            rest = rest * rest * rest;
            rest = rest * rest * rest;
            let raise = 1.0 - rest;
            let bobbing = if f64::from(progress) > 0.2 { 1.0 } else { 0.0 };
            stack.translate(
                0.0,
                (java_cos(remaining / 4.0 * PI) * 0.1).abs() * bobbing,
                0.0,
            );
            stack.translate(raise * 0.6, -raise * 0.5, 0.0);
            stack.rotate_y(raise * 90.0);
            stack.rotate_x(raise * 10.0);
            stack.rotate_z(raise * 30.0);
        }
        Some(_) => {}
        None => stack.translate(
            -java_sin(sqrt(swing) * PI) * 0.4,
            java_sin(sqrt(swing) * PI * 2.0) * 0.2,
            -java_sin(swing * PI) * 0.2,
        ),
    }
    stack.translate(
        0.7 * 0.8,
        -0.65 * 0.8 - (1.0 - hand.equip) * 0.6,
        -0.9 * 0.8,
    );
    stack.rotate_y(45.0);
    stack.rotate_y(-java_sin(swing * swing * PI) * 20.0);
    stack.rotate_z(-java_sin(sqrt(swing) * PI) * 20.0);
    stack.rotate_x(-java_sin(sqrt(swing) * PI) * 80.0);
    stack.scale(0.4, 0.4, 0.4);
    match hand.using {
        Some(JavaUse::Block) => {
            stack.translate(-0.5, 0.2, 0.0);
            stack.rotate_y(30.0);
            stack.rotate_x(-80.0);
            stack.rotate_y(60.0);
        }
        Some(JavaUse::Bow { pull }) => {
            stack.rotate_z(-18.0);
            stack.rotate_y(-12.0);
            stack.rotate_x(-8.0);
            stack.translate(-0.9, 0.2, 0.0);
            let mut draw = pull / 20.0;
            draw = (draw * draw + draw * 2.0) / 3.0;
            draw = draw.min(1.0);
            if draw > 0.1 {
                stack.translate(0.0, java_sin((pull - 0.1) * 1.3) * 0.01 * (draw - 0.1), 0.0);
            }
            stack.translate(0.0, 0.0, draw * 0.1);
            stack.rotate_z(-335.0);
            stack.rotate_y(-50.0);
            stack.translate(0.0, 0.5, 0.0);
            stack.scale(1.0, 1.0, 1.0 + draw * 0.2);
            stack.translate(0.0, -0.5, 0.0);
            stack.rotate_y(50.0);
            stack.rotate_z(335.0);
        }
        _ => {}
    }
    if rotate_around {
        stack.rotate_y(180.0);
    }
    draw_item(stack, mesh)
}

/// Camera space from our rig frame (blocks) for the empty first-person hand; the arm bone
/// carries the rest pose of [`java_biped`].
#[must_use]
pub fn first_person_arm(swing: f32, equip: f32) -> Mat4 {
    let mut stack = Stack(Mat4::IDENTITY);
    stack.translate(
        -java_sin(sqrt(swing) * PI) * 0.3,
        java_sin(sqrt(swing) * PI * 2.0) * 0.4,
        -java_sin(swing * PI) * 0.4,
    );
    stack.translate(0.8 * 0.8, -0.75 * 0.8 - (1.0 - equip) * 0.6, -0.9 * 0.8);
    stack.rotate_y(45.0);
    stack.rotate_y(java_sin(sqrt(swing) * PI) * 70.0);
    stack.rotate_z(-java_sin(swing * swing * PI) * 20.0);
    stack.translate(-1.0, 3.6, 3.5);
    stack.rotate_z(120.0);
    stack.rotate_x(200.0);
    stack.rotate_y(-135.0);
    stack.translate(5.6, 0.0, 0.0);
    stack.0 * rig_from_java_model().inverse()
}

/// What the third-person hand holds, as Java chooses its grip.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JavaHeldItem {
    Block,
    Bow,
    /// Tools, swords, sticks and rods; `rotate_around` turns rods over.
    Tool {
        rotate_around: bool,
        blocking: bool,
    },
    Flat,
}

/// The held mesh's placement in the right arm bone's frame (rig axes and blocks, about its
/// pivot).
#[must_use]
pub fn third_person_item(item: JavaHeldItem, mesh: JavaItemMesh) -> Mat4 {
    let mut stack = Stack(Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0)));
    stack.translate(-0.0625, 0.4375, 0.0625);
    match item {
        JavaHeldItem::Block => {
            let scale = 0.5 * 0.75;
            stack.translate(0.0, 0.1875, -0.3125);
            stack.rotate_x(20.0);
            stack.rotate_y(45.0);
            stack.scale(-scale, -scale, scale);
        }
        JavaHeldItem::Bow => {
            stack.translate(0.0, 0.125, 0.3125);
            stack.rotate_y(-20.0);
            stack.scale(0.625, -0.625, 0.625);
            stack.rotate_x(-100.0);
            stack.rotate_y(45.0);
        }
        JavaHeldItem::Tool {
            rotate_around,
            blocking,
        } => {
            if rotate_around {
                stack.rotate_z(180.0);
                stack.translate(0.0, -0.125, 0.0);
            }
            if blocking {
                stack.translate(0.05, 0.0, -0.1);
                stack.rotate_y(-50.0);
                stack.rotate_x(-10.0);
                stack.rotate_z(-60.0);
            }
            stack.translate(0.0, 0.1875, 0.0);
            stack.scale(0.625, -0.625, 0.625);
            stack.rotate_x(-100.0);
            stack.rotate_y(45.0);
        }
        JavaHeldItem::Flat => {
            stack.translate(0.25, 0.1875, -0.1875);
            stack.scale(0.375, 0.375, 0.375);
            stack.rotate_z(60.0);
            stack.rotate_x(-90.0);
            stack.rotate_z(20.0);
        }
    }
    draw_item(stack, mesh)
}

/// The cape at the frame: its chasing point less the position (blocks), body yaw (degrees),
/// walk bob amplitude and walked distance.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct JavaCapeInput {
    pub chase: Vec3,
    pub body_yaw: f32,
    pub bob: f32,
    pub walked: f32,
    pub sneaking: bool,
}

/// Java's cape tilt about X and sideways swing in degrees; the swing turns half about Z and
/// half back about Y.
#[must_use]
pub fn java_cape_angles(input: &JavaCapeInput) -> (f32, f32) {
    let yaw = input.body_yaw * PI / 180.0;
    let (sine, cosine) = (f64::from(java_sin(yaw)), -f64::from(java_cos(yaw)));
    let [x, y, z] = input.chase.as_dvec3().to_array();
    let mut lift = (y as f32 * 10.0).clamp(-6.0, 32.0);
    let back = (((x * sine + z * cosine) as f32) * 100.0).max(0.0);
    let side = ((x * cosine - z * sine) as f32) * 100.0;
    lift += java_sin(input.walked * 6.0) * 32.0 * input.bob;
    if input.sneaking {
        lift += 25.0;
    }
    (6.0 + back / 2.0 + lift, side / 2.0)
}

/// The cape bone in our rig frame (blocks) for its bind pivot `pivot`: Java turns the cape about
/// a point two pixels behind the neck, in model space rather than with the body.
#[must_use]
pub fn java_cape_bone(input: &JavaCapeInput, pivot: Vec3) -> (Quat, Vec3) {
    let (tilt, side) = java_cape_angles(input);
    let rotation = Quat::from_rotation_x(-tilt.to_radians())
        * Quat::from_rotation_z(side.to_radians())
        * Quat::from_rotation_y(side.to_radians());
    let hinge = rig_from_java_model().transform_point3(Vec3::Z * 2.0 / 16.0);
    (rotation, hinge + rotation * (pivot - hinge))
}

/// Items Java draws upright in the hand.
#[must_use]
pub fn is_java_tool(identifier: &str) -> bool {
    let name = identifier.strip_prefix("minecraft:").unwrap_or(identifier);
    ["_sword", "_pickaxe", "_axe", "_shovel", "_hoe"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
        || matches!(name, "stick" | "bone")
        || crate::equipment::is_rod(name)
}

/// Swords, which Java blocks with.
#[must_use]
pub fn is_java_sword(identifier: &str) -> bool {
    identifier.ends_with("_sword")
}
