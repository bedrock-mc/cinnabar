use bevy::prelude::{EulerRot, Quat, Vec3};

use super::runtime::{ActorView, facing_rotation};

/// Prepared target settings retain the camera definition defaults for omitted options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TargetSettings {
    pub rotation_speed: f32,
    pub distance: f32,
    pub snap_to_target: bool,
    pub continue_targeting: bool,
    pub horizontal_limit: [f32; 2],
    pub vertical_limit: [f32; 2],
}

impl Default for TargetSettings {
    fn default() -> Self {
        Self {
            rotation_speed: 0.0,
            distance: 50.0,
            snap_to_target: false,
            continue_targeting: false,
            horizontal_limit: [0.0, 360.0],
            vertical_limit: [0.0, 180.0],
        }
    }
}

/// A target keeps its original orientation for limits and returning out of range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TargetFocus {
    pub actor: i64,
    offset: Vec3,
    initial: Quat,
    current: Quat,
    found: bool,
    acquired: bool,
    pub settings: TargetSettings,
}

impl TargetFocus {
    /// Starts tracking without changing the pre-target orientation.
    pub fn new(actor: i64, offset: Vec3, rotation: Quat, settings: TargetSettings) -> Self {
        Self {
            actor,
            offset,
            initial: rotation,
            current: rotation,
            found: false,
            acquired: false,
            settings,
        }
    }

    /// Samples instant tracking, or the orientation advanced by the speed-limited update.
    pub fn sample(&self, position: Vec3, actor: Option<ActorView>) -> Quat {
        if self.settings.rotation_speed > 0.0 {
            return self.current;
        }
        actor
            .and_then(|actor| self.desired(position, actor))
            .map_or(self.current, |(rotation, in_range)| {
                if in_range || self.settings.continue_targeting {
                    rotation
                } else {
                    self.initial
                }
            })
    }

    /// Advances at a bounded angular speed and reports when an acquired actor disappears.
    pub fn advance(&mut self, seconds: f32, position: Vec3, actor: Option<ActorView>) -> bool {
        let Some(actor) = actor else {
            return !self.found;
        };
        self.found = true;
        let Some((rotation, in_range)) = self.desired(position, actor) else {
            return false;
        };
        let destination = if in_range || self.settings.continue_targeting {
            rotation
        } else {
            self.initial
        };
        if self.settings.rotation_speed <= 0.0
            || (self.settings.snap_to_target && !self.acquired && in_range)
        {
            self.current = destination;
        } else {
            let angle = self.current.angle_between(destination);
            let fraction = if angle > 0.0 {
                (self.settings.rotation_speed.to_radians() * seconds.max(0.0) / angle).min(1.0)
            } else {
                1.0
            };
            self.current = self.current.slerp(destination, fraction).normalize();
        }
        self.acquired = in_range;
        true
    }

    /// Acquisition and range changes can cut instantly; established tracking stays continuous.
    pub fn reanchored_since(&self, previous: Self) -> bool {
        self.current != previous.current
            && if self.settings.rotation_speed <= 0.0 {
                !previous.found
                    || (!self.settings.continue_targeting && self.acquired != previous.acquired)
            } else {
                self.settings.snap_to_target && !previous.acquired && self.acquired
            }
    }

    /// Preserves the last rendered target orientation after the actor is removed.
    pub fn last_rotation(&self) -> Quat {
        self.current
    }

    /// Resolves the yaw-relative target center and the definition's asymmetric angle bounds.
    fn desired(&self, position: Vec3, actor: ActorView) -> Option<(Quat, bool)> {
        let yaw = f64::from(actor.yaw_degrees.to_radians());
        let sin = sim::minecraft_sin(yaw) as f32;
        let cos = sim::minecraft_cos(yaw) as f32;
        let offset = Vec3::new(
            self.offset.x * cos - self.offset.z * sin,
            self.offset.y,
            self.offset.z * cos + self.offset.x * sin,
        );
        let center = actor.position + offset;
        let distance = position.distance(center);
        if distance > 1024.0 {
            return None;
        }
        let desired = facing_rotation(position, center, self.current);
        let (yaw, pitch, _) = desired.to_euler(EulerRot::YXZ);
        let (initial_yaw, _, _) = self.initial.to_euler(EulerRot::YXZ);
        let [left, right] = self.settings.horizontal_limit;
        let center_yaw = initial_yaw - ((right - left) * 0.5).to_radians();
        let half_width = ((left + right) * 0.5).to_radians();
        let relative = (yaw - center_yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        let limited_yaw = relative.clamp(-half_width, half_width);
        let [low, high] = self.settings.vertical_limit;
        let limited_pitch = pitch.clamp((low - 90.0).to_radians(), (high - 90.0).to_radians());
        let in_range =
            distance <= self.settings.distance && relative == limited_yaw && pitch == limited_pitch;
        Some((
            Quat::from_euler(EulerRot::YXZ, center_yaw + limited_yaw, limited_pitch, 0.0),
            in_range,
        ))
    }
}

#[cfg(test)]
mod tests;
