//! Independent cosmetic followers with terrain supplied by the caller.
use super::clip_playback::{Clip, Definition as ClipDefinition};
use glam::{Quat, Vec3};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Asset-owned clip selection and stride lengths for a ground companion.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    idle: String,
    sit: String,
    trot: String,
    gallop: String,
    trot_stride: f32,
    gallop_stride: f32,
    #[serde(default)]
    tricks: Vec<String>,
}

/// Immutable animation controller admitted with the item's verified assets.
pub struct Rig {
    clips: Vec<Clip>,
    strides: [f32; 2],
    pub(super) clearance: Box<[Clearance]>,
}

pub(super) mod clearance;
pub use clearance::Clearance;
mod navigation;
use navigation::{ARRIVAL, GRAVITY, Hop, placement};

/// Locomotion state selects authored clips without relying on item or bone names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gait {
    Idle,
    Sit,
    Trot,
    Gallop,
    Trick(u8),
}
impl Gait {
    /// Stable position in the admitted controller's clip table.
    fn index(self) -> usize {
        match self {
            Self::Idle => 0,
            Self::Sit => 1,
            Self::Trot => 2,
            Self::Gallop => 3,
            Self::Trick(i) => 4 + usize::from(i),
        }
    }
}

/// One bounded sample of an authored companion clip.
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub gait: Gait,
    pub seconds: f32,
}

impl Rig {
    /// Resolves every controller state to a validated clip; rejects unused animation data.
    pub(super) fn build(
        definition: Definition,
        mut definitions: BTreeMap<String, ClipDefinition>,
        names: &[&str],
    ) -> Option<Self> {
        let strides = [definition.trot_stride, definition.gallop_stride];
        if strides
            .iter()
            .any(|s| !s.is_finite() || !(0.25..=8.0).contains(s))
            || definition.tricks.len() > 4
        {
            return None;
        }
        let mut clips = Vec::new();
        let states = [
            definition.idle,
            definition.sit,
            definition.trot,
            definition.gallop,
        ]
        .into_iter()
        .chain(definition.tricks);
        for (index, name) in states.enumerate() {
            let definition = definitions.remove(&name)?;
            if definition.r#loop != (index < 4) {
                return None;
            }
            clips.push(Clip::build(definition, names)?);
        }
        definitions.is_empty().then_some(Self {
            clips,
            strides,
            clearance: Box::new([]),
        })
    }

    /// Rotation-independent body sections derived from the item's locomotion poses.
    pub fn clearance(&self) -> &[Clearance] {
        &self.clearance
    }

    /// Samples authored degrees in the same mirrored coordinate system as other cosmetics.
    pub(super) fn sample(&self, bone: usize, pose: Pose) -> (Quat, Vec3) {
        let Some(clip) = self.clips.get(pose.gait.index()) else {
            return (Quat::IDENTITY, Vec3::ZERO);
        };
        if !pose.seconds.is_finite() {
            return (Quat::IDENTITY, Vec3::ZERO);
        }
        let time = if matches!(pose.gait, Gait::Trick(_)) {
            pose.seconds.min(clip.length())
        } else {
            pose.seconds.rem_euclid(clip.length())
        };
        let (angles, position) = clip.sample_at(bone, time);
        let angles = angles * (std::f32::consts::PI / 180.0);
        (
            Quat::from_euler(glam::EulerRot::ZYX, -angles.z, -angles.y, angles.x),
            Vec3::new(-position.x, position.y, position.z) / 16.0,
        )
    }
}

/// Per-wearer movement history; no world or rendering resources are retained here.
#[derive(Default)]
pub struct Follower {
    feet: Option<Vec3>,
    owner: Option<Vec3>,
    velocity: Vec3,
    vertical: f32,
    yaw: f32,
    idle: f32,
    goal: Option<Vec3>,
    goal_owner: Option<(Vec3, f32)>,
    hop: Option<Hop>,
    hop_retry: f32,
    recovery: Option<Vec3>,
    phase: f32,
    clock: f32,
    trick: Option<(u8, f32)>,
    cooldown: f32,
    next_trick: u8,
}

/// Evaluated world placement and locomotion sample for one visible follower.
pub struct Frame {
    pub feet: Vec3,
    pub yaw: f32,
    pub pose: Pose,
}

impl Follower {
    /// Current simulated position, retained while changed terrain makes drawing unavailable.
    pub fn position(&self) -> Option<Vec3> {
        self.feet
    }

    /// Advances using real owner motion. Terrain returns a supported, clear floor under the
    /// candidate footprint, at most `rise` above or `drop` below the candidate's feet.
    /// `clear` validates the full artwork at newly initialized or recovered standing positions.
    /// Missing terrain freezes movement and never becomes an invented air or floor cell.
    #[allow(clippy::too_many_arguments)]
    pub fn advance(
        &mut self,
        rig: &Rig,
        owner: Vec3,
        yaw: f32,
        seconds: f32,
        mut terrain: impl FnMut(Vec3, f32, f32) -> Option<f32>,
        mut clip: impl FnMut(Vec3, f32, Vec3) -> Vec3,
        mut clear: impl FnMut(Vec3, f32) -> bool,
    ) -> Option<Frame> {
        if !owner.is_finite() || !yaw.is_finite() || !seconds.is_finite() || seconds < 0.0 {
            return None;
        }
        let forward = Vec3::new(-yaw.sin(), 0.0, yaw.cos());
        let right = Vec3::new(forward.z, 0.0, -forward.x);
        let target = owner - forward * 1.5 + right * 1.05;
        let teleported = self.owner.is_some_and(|old| old.distance(owner) > 8.0);
        self.owner = Some(owner);
        if teleported {
            self.feet = None;
            self.goal = None;
            self.goal_owner = None;
            self.velocity = Vec3::ZERO;
            self.vertical = 0.0;
            self.hop = None;
            self.recovery = None;
            self.trick = None;
        }
        if self.goal_owner.is_none_or(|(position, heading)| {
            position.distance(owner) > 0.15 || (heading - yaw).abs() > 0.1
        }) || self.goal.is_none_or(|goal| {
            !clear(goal, yaw)
                || !terrain(goal, 0.05, 0.1).is_some_and(|floor| (floor - goal.y).abs() < 0.01)
        }) {
            self.goal = placement(target, yaw, &mut terrain, &mut clear);
            self.goal_owner = Some((owner, yaw));
            self.hop_retry = 0.0;
        }
        if self.feet.is_none() {
            self.feet = self.goal;
            self.feet?;
            self.yaw = yaw;
        }
        let mut feet = self.feet?;
        let lost_support = self.hop.is_none()
            && self.vertical == 0.0
            && !terrain(feet, 0.05, 0.1).is_some_and(|floor| (floor - feet.y).abs() < 0.01);
        if !clear(feet, self.yaw) || self.recovery.is_some() || lost_support {
            self.hop = None;
            self.velocity = Vec3::ZERO;
            self.vertical = 0.0;
            self.idle = 0.0;
            self.goal_owner = None;
            if self.recovery.is_none_or(|goal| {
                !clear(goal, self.yaw)
                    || !terrain(goal, 0.05, 0.1).is_some_and(|floor| (floor - goal.y).abs() < 0.01)
            }) {
                self.recovery = placement(feet, self.yaw, &mut terrain, &mut clear);
            }
            if let Some(goal) = self.recovery {
                let movement = (goal - feet).clamp_length_max(3.0 * seconds.min(0.15));
                feet += clip(feet, self.yaw, movement);
                self.feet = Some(feet);
            }
            if !clear(feet, self.yaw) {
                return None;
            }
            if self.recovery.is_some_and(|goal| feet.distance(goal) < 0.01)
                && terrain(feet, 0.05, 0.1).is_some_and(|floor| (floor - feet.y).abs() < 0.01)
            {
                self.recovery = None;
            }
            return Some(Frame {
                feet,
                yaw: self.yaw,
                pose: Pose {
                    gait: Gait::Idle,
                    seconds: self.clock,
                },
            });
        }
        self.recovery = None;
        let target = self.goal.unwrap_or(self.feet?);
        let dt = seconds.min(0.15);
        let steps = (dt * 60.0).ceil().max(1.0) as usize;
        let dt = dt / steps as f32;
        let mut travelled = 0.0;
        let mut speed = 0.0;
        for _ in 0..steps {
            let mut feet = self.feet?;
            let old = feet;
            self.hop_retry = (self.hop_retry - dt).max(0.0);
            let mut airborne = self.hop.is_some();
            if let Some(hop) = &mut self.hop {
                let time = (hop.elapsed + dt).min(hop.duration);
                let next = hop.point(time);
                let movement = next - feet;
                if clear(next, hop.yaw) && clip(feet, hop.yaw, movement).distance(movement) <= 0.001
                {
                    feet = next;
                    self.yaw = hop.yaw;
                    hop.elapsed = time;
                    if time == hop.duration {
                        self.hop = None;
                    }
                } else {
                    self.hop = None;
                    self.hop_retry = 0.5;
                }
                self.velocity = Vec3::ZERO;
                self.vertical = 0.0;
            } else {
                let delta = (target - feet) * Vec3::new(1.0, 0.0, 1.0);
                let distance = delta.length();
                let desired =
                    delta.normalize_or_zero() * ((distance - ARRIVAL).max(0.0) * 3.2).min(9.0);
                self.velocity += (desired - self.velocity).clamp_length_max(20.0 * dt);
                if distance > 0.8 {
                    self.trick = None;
                }
                let grounded =
                    terrain(feet, 0.05, 0.1).is_some_and(|floor| (floor - feet.y).abs() < 0.06);
                if self.velocity.length() > 0.05 && self.trick.is_none() {
                    let direction = self.velocity.normalize();
                    for angle in [0.0_f32, 0.65, -0.65, 1.3, -1.3] {
                        let direction = Quat::from_rotation_y(angle) * direction;
                        let heading = (-direction.x).atan2(direction.z);
                        let turn = (heading - self.yaw + std::f32::consts::PI)
                            .rem_euclid(std::f32::consts::TAU)
                            - std::f32::consts::PI;
                        let heading = self.yaw + turn.clamp(-8.0 * dt, 8.0 * dt);
                        if !clear(feet, heading) {
                            continue;
                        }
                        let requested =
                            direction * (self.velocity.length().min(7.5) * dt).min(distance);
                        let movement = clip(feet, heading, requested);
                        let candidate = feet + movement;
                        let supported = terrain(candidate, 0.05, 3.0)
                            .is_some_and(|floor| !grounded || floor >= feet.y - 0.05);
                        if (!supported || movement.distance(requested) > 0.001)
                            && grounded
                            && self.hop_retry == 0.0
                        {
                            if let Some(hop) = navigation::hop(
                                feet,
                                target,
                                direction,
                                heading,
                                &mut terrain,
                                &mut clip,
                                &mut clear,
                            ) {
                                self.hop = Some(hop);
                                self.trick = None;
                                break;
                            }
                        }
                        if supported
                            && movement.length_squared() > 0.0000001
                            && (target - candidate).length_squared()
                                < (target - feet).length_squared()
                            && clear(candidate, heading)
                        {
                            feet = candidate;
                            self.yaw = heading;
                            break;
                        }
                    }
                    if self.hop.is_none() && feet == old && self.hop_retry == 0.0 {
                        self.hop_retry = 0.25;
                    }
                }
                if self.hop.is_none() {
                    self.vertical -= GRAVITY * dt;
                    let requested = Vec3::Y * self.vertical * dt;
                    let movement = clip(feet, self.yaw, requested);
                    if (movement.y - requested.y).abs() > 0.0001 {
                        self.vertical = 0.0;
                    }
                    let next_y = feet.y + movement.y;
                    if let Some(ground) = terrain(feet, 0.05, 3.0) {
                        if next_y <= ground {
                            feet.y = ground;
                            self.vertical = 0.0;
                        } else {
                            feet.y = next_y;
                        }
                    } else {
                        self.vertical = 0.0;
                    }
                    airborne = !grounded;
                } else {
                    airborne = true;
                }
            }
            let displacement = ((feet - old) * Vec3::new(1.0, 0.0, 1.0)).length();
            travelled += displacement;
            speed = displacement / dt.max(0.0001);
            if speed > 0.08 || airborne {
                self.idle = 0.0;
            } else {
                self.idle += dt;
            }
            self.feet = Some(feet);
            self.clock = (self.clock + dt).rem_euclid(60.0);
            self.cooldown = (self.cooldown - dt).max(0.0);
            if self.idle > 7.0
                && self.cooldown == 0.0
                && rig.clips.len() > 4
                && self.trick.is_none()
            {
                self.trick = Some((self.next_trick, 0.0));
                self.next_trick = (self.next_trick + 1) % (rig.clips.len() - 4) as u8;
                self.cooldown = 18.0;
            }
            if let Some((index, time)) = &mut self.trick {
                *time += dt;
                if *time >= rig.clips[4 + usize::from(*index)].length() {
                    self.trick = None;
                    self.idle = 0.0;
                }
            }
        }
        let gait = if speed > 4.6 {
            Gait::Gallop
        } else if speed > 0.08 {
            Gait::Trot
        } else if self.idle > 1.0 {
            Gait::Sit
        } else {
            Gait::Idle
        };
        let seconds = if matches!(gait, Gait::Trot | Gait::Gallop) {
            let index = usize::from(gait == Gait::Gallop);
            self.phase = (self.phase + travelled / rig.strides[index]).rem_euclid(1.0);
            self.phase * rig.clips[gait.index()].length()
        } else {
            self.clock
        };
        let pose = self
            .trick
            .map_or(Pose { gait, seconds }, |(index, seconds)| Pose {
                gait: Gait::Trick(index),
                seconds,
            });
        let feet = self.feet?;
        clear(feet, self.yaw).then_some(Frame {
            feet,
            yaw: self.yaw,
            pose,
        })
    }
}

#[cfg(test)]
mod tests;
