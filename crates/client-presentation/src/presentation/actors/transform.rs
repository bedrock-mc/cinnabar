//! World placement and body rotations for actor presentations.

use actor_animation::ActorRigSnapshot;
use client_world::ActorSnapshot;

/// Places authored frame scales with the actor's metadata scale, feet, facing and tilts.
pub(crate) fn sampled_rig_placement(
    rig: &ActorRigSnapshot<'_>,
    actor: &ActorSnapshot,
    alpha: f32,
    authored: [f32; 4],
) -> Option<([[f32; 4]; 3], f32)> {
    let alpha = alpha.clamp(0.0, 1.0);
    let position = interpolated_position(actor, alpha)?;
    let yaw = if rig.placement_frame.placement() == Some(assets::placement::ActorYawFrame::Absolute)
    {
        0.0
    } else {
        lerp_degrees(rig.previous_body_yaw, rig.body_yaw, alpha)
    };
    let scale = authored[0] * actor.render_scale();
    if !yaw.is_finite()
        || !scale.is_finite()
        || scale <= 0.0
        || authored.iter().any(|x| !x.is_finite())
    {
        return None;
    }
    let rows = glide_tilted(
        death_tilted(
            scaled_axes(
                rig_world_from_actor(position, yaw, scale),
                [authored[1], authored[2], authored[3]],
            ),
            actor.death_rotation_progress(alpha),
        ),
        spin_rotation(actor, alpha).or_else(|| glide_rotation(actor, alpha)),
    );
    Some((rows, yaw))
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

/// Scales the model's own axes (`scaleX`, `scaleY`, `scaleZ`) about its feet.
pub(super) fn scaled_axes(mut rows: [[f32; 4]; 3], axis_scale: [f32; 3]) -> [[f32; 4]; 3] {
    for row in &mut rows {
        for (value, scale) in row.iter_mut().zip(axis_scale) {
            *value *= scale;
        }
    }
    rows
}

/// Tips the rig sideways about its feet by `progress` of a right angle.
pub fn death_tilted(mut rows: [[f32; 4]; 3], progress: Option<f32>) -> [[f32; 4]; 3] {
    let Some(progress) = progress else {
        return rows;
    };
    let angle = progress.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
    let (sine, cosine) = angle.sin_cos();
    for row in &mut rows {
        let (x, y) = (row[0], row[1]);
        row[0] = x * cosine + y * sine;
        row[1] = -x * sine + y * cosine;
    }
    rows
}

/// Vanilla's single-precision degree/radian factors for the glide tilt.
const DEGREES_TO_RADIANS: f32 = 0.017_453_292;
const RADIANS_TO_DEGREES: f32 = 57.295_776;
/// Cross products below this leave the gliding body unturned.
const GLIDE_TURN_DEAD_ZONE: f32 = 0.0625;

/// A spin pitches immediately along the view and rotates around the actor's own up axis.
pub fn spin_rotation(actor: &ActorSnapshot, alpha: f32) -> Option<[f32; 2]> {
    actor.is_spinning().then_some([
        -90.0 - actor.pitch,
        (actor.status.age_ticks as f32 + alpha) * -75.0,
    ])
}

/// A gliding actor's `[pitch, yaw]` body tilt in degrees: pitch eases in over its first ten
/// gliding ticks, and yaw turns it toward its horizontal motion.
#[must_use]
pub fn glide_rotation(actor: &ActorSnapshot, alpha: f32) -> Option<[f32; 2]> {
    if !actor.is_gliding() {
        return None;
    }
    let ticks = actor.status.fall_fly_ticks as f32 + alpha;
    let ease = ticks * ticks / 100.0;
    let ease = if ease > 1.0 { 1.0 } else { ease.max(0.0) };
    Some([(-90.0 - actor.pitch) * ease, glide_turn(actor, alpha)])
}

/// Signed angle from the interpolated view to the horizontal motion, without normalising the
/// view's own horizontal length, as vanilla measures it.
fn glide_turn(actor: &ActorSnapshot, alpha: f32) -> f32 {
    let lerp = |previous: f32, current: f32| {
        let wrapped = (current - previous + 180.0) % 360.0;
        let wrapped = if wrapped < 0.0 {
            wrapped + 360.0
        } else {
            wrapped
        };
        (wrapped - 180.0) * alpha + previous
    };
    let yaw = lerp(actor.previous_pose.yaw, actor.yaw);
    let pitch = lerp(actor.previous_pose.pitch, actor.pitch);
    let yaw_angle = f64::from(-std::f32::consts::PI - yaw * DEGREES_TO_RADIANS);
    let horizontal = -(sim::minecraft_cos(f64::from(-(pitch * DEGREES_TO_RADIANS))) as f32);
    let view_z = sim::minecraft_cos(yaw_angle) as f32 * horizontal;
    let view_x = horizontal * sim::minecraft_sin(yaw_angle) as f32;
    let [delta_x, _, delta_z] = actor.native_velocity();
    let motion = delta_z * delta_z + delta_x * delta_x;
    if view_z * view_z + view_x * view_x <= 0.0 || motion <= 0.0 {
        return 0.0;
    }
    let cross = view_z * delta_x - view_x * delta_z;
    let side = if cross.abs() < GLIDE_TURN_DEAD_ZONE {
        0.0
    } else {
        cross.signum()
    };
    let turn = ((view_z * delta_z + view_x * delta_x) / motion.sqrt()).acos() * side;
    // Rounding can push the cosine just past one; that frame keeps the body unturned.
    if turn.is_finite() {
        turn * RADIANS_TO_DEGREES
    } else {
        0.0
    }
}

/// Pitches the rig about its feet, then turns it about its own up axis.
pub fn glide_tilted(mut rows: [[f32; 4]; 3], rotation: Option<[f32; 2]>) -> [[f32; 4]; 3] {
    let Some([pitch, yaw]) = rotation else {
        return rows;
    };
    let (sine, cosine) = (pitch * DEGREES_TO_RADIANS).sin_cos();
    for row in &mut rows {
        let (y, z) = (row[1], row[2]);
        row[1] = y * cosine + z * sine;
        row[2] = -y * sine + z * cosine;
    }
    let (sine, cosine) = (yaw * DEGREES_TO_RADIANS).sin_cos();
    for row in &mut rows {
        let (x, z) = (row[0], row[2]);
        row[0] = x * cosine - z * sine;
        row[2] = x * sine + z * cosine;
    }
    rows
}

/// Samples the actor's feet from its retained movement poses.
pub(super) fn interpolated_position(actor: &ActorSnapshot, partial_tick: f32) -> Option<[f32; 3]> {
    actor.interpolated_position(partial_tick)
}

/// Interpolates angles across the shortest wrapped arc.
pub(crate) fn lerp_degrees(start: f32, end: f32, alpha: f32) -> f32 {
    wrap_degrees(start + wrap_degrees(end - start) * alpha)
}

/// Wraps an angle into the signed half-turn range.
pub(crate) fn wrap_degrees(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

/// Converts local Euler angles into the rig's quaternion rotation.
pub(super) fn quaternion_from_euler_degrees(rotation: [f32; 3]) -> [f32; 4] {
    let [x, y, z] = rotation.map(|value| value.to_radians() * 0.5);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    [
        sx * cy * cz - cx * sy * sz,
        cx * sy * cz + sx * cy * sz,
        cx * cy * sz - sx * sy * cz,
        cx * cy * cz + sx * sy * sz,
    ]
}
