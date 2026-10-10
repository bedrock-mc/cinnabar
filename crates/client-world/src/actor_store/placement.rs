use protocol::ActorMetadataValue;

use super::ActorStore;

/// Seat metadata streamed on the rider (protocol `EntityDataKeySeat*`).
const KEY_SEAT_OFFSET: u32 = 56;
const KEY_SEAT_LOCK_DEGREES: u32 = 58;
const KEY_SEAT_ROTATION_DEGREES: u32 = 60;
const KEY_BED_POSITION: u32 = 28;

const FLAG_SADDLED: u32 = 8;
pub(crate) const FLAG_BABY: u32 = 11;
const FLAG_TAMED: u32 = 28;
const FLAG_SHEARED: u32 = 31;

/// Head turn, in degrees either side of the body, at or beyond which a seat does not limit it.
const UNLOCKED_HEAD_DEGREES: f32 = 180.0;

/// World offset of a mount-local seat `[right, up, forward]` for a mount facing `yaw_degrees`.
pub(super) fn seat_world_offset(local: [f32; 3], yaw_degrees: f32) -> [f32; 3] {
    let (sin, cos) = yaw_degrees.to_radians().sin_cos();
    [
        local[0] * cos - local[2] * sin,
        local[1],
        local[0] * sin + local[2] * cos,
    ]
}

fn wrap_degrees(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

/// One rider position of a mount type, usable while its rider count is in `min..=max`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RideSeat {
    /// Mount-local `[right, up, forward]`.
    pub position: [f32; 3],
    pub min_riders: u32,
    pub max_riders: u32,
    /// Degrees added to the mount's yaw for the rider's body; `None` when the pack gives an
    /// expression instead of a number.
    pub rotate_by: Option<f32>,
    /// Degrees the rider's head may turn either side of its body; `None` leaves it free.
    pub lock_degrees: Option<f32>,
}

/// Mount state a seat layout applies to, read from the mount's flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeatRequirement {
    Saddled(bool),
    Baby(bool),
    Tamed(bool),
    Sheared(bool),
}

impl SeatRequirement {
    fn holds(self, mount: &super::ActorSnapshot) -> bool {
        let (bit, wanted) = match self {
            Self::Saddled(wanted) => (FLAG_SADDLED, wanted),
            Self::Baby(wanted) => (FLAG_BABY, wanted),
            Self::Tamed(wanted) => (FLAG_TAMED, wanted),
            Self::Sheared(wanted) => (FLAG_SHEARED, wanted),
        };
        mount.flag(bit) == wanted
    }

    /// Requirements a component group's name spells out (`pig_unsaddled`, `cow_baby`, ...).
    pub fn from_group_name(name: &str) -> Vec<Self> {
        let mut found = Vec::new();
        for token in name.split(|ch: char| !ch.is_ascii_alphanumeric()) {
            found.extend(match token {
                "saddled" => Some(Self::Saddled(true)),
                "unsaddled" => Some(Self::Saddled(false)),
                "baby" => Some(Self::Baby(true)),
                "adult" => Some(Self::Baby(false)),
                "sheared" => Some(Self::Sheared(true)),
                "unsheared" => Some(Self::Sheared(false)),
                "tamed" | "tame" => Some(Self::Tamed(true)),
                "wild" => Some(Self::Tamed(false)),
                _ => None,
            });
        }
        found
    }
}

#[derive(Clone, Debug)]
struct SeatLayout {
    requirements: Vec<SeatRequirement>,
    seats: Vec<RideSeat>,
}

/// Seat layouts by mount identifier, used when the server streams no seat offset.
#[derive(Clone, Debug, Default)]
pub struct SeatDefaults {
    layouts: std::collections::HashMap<std::sync::Arc<str>, Vec<SeatLayout>>,
}

impl SeatDefaults {
    /// Adds a layout that applies while every requirement holds; none applies always.
    pub fn insert(
        &mut self,
        identifier: impl Into<std::sync::Arc<str>>,
        requirements: Vec<SeatRequirement>,
        seats: Vec<RideSeat>,
    ) {
        if !seats.is_empty() {
            self.layouts
                .entry(identifier.into())
                .or_default()
                .push(SeatLayout {
                    requirements,
                    seats,
                });
        }
    }

    /// Seat for the `index`th of `riders` riders of `mount`, from its most specific matching
    /// layout; an index past the seats takes the last one.
    fn seat(
        &self,
        mount: &super::ActorSnapshot,
        name: &str,
        riders: u32,
        index: usize,
    ) -> Option<RideSeat> {
        let layout = self
            .layouts
            .get(name)?
            .iter()
            .filter(|layout| layout.requirements.iter().all(|need| need.holds(mount)))
            .reduce(|best, layout| {
                if layout.requirements.len() > best.requirements.len() {
                    layout
                } else {
                    best
                }
            })?;
        let usable: Vec<_> = layout
            .seats
            .iter()
            .filter(|seat| (seat.min_riders..=seat.max_riders).contains(&riders))
            .collect();
        usable.get(index).or(usable.last()).map(|seat| **seat)
    }
}

impl super::movement_interpolation::MotionState {
    /// Holds a rider on its seat: position and body follow the mount, the head within the lock.
    fn seat(&mut self, placement: &Placement) {
        self.received.position = placement.position;
        self.pose.position = placement.position;
        self.pose.yaw = placement.body_yaw;
        self.received.yaw = placement.body_yaw;
        if let Some(limit) = placement.lock_degrees {
            let head = placement.body_yaw
                + wrap_degrees(self.pose.head_yaw - placement.body_yaw).clamp(-limit, limit);
            self.pose.head_yaw = head;
            self.received.head_yaw = head;
        }
        self.remaining = 0;
    }
}

/// Where a rider sits and how its body and head are held.
struct Placement {
    runtime_id: u64,
    position: [f32; 3],
    body_yaw: f32,
    lock_degrees: Option<f32>,
}

impl ActorStore {
    pub(crate) fn set_seat_defaults(&mut self, defaults: std::sync::Arc<SeatDefaults>) {
        self.seat_defaults = defaults;
    }

    /// Streamed seat of a rider, else the mount type's default for its rider index (riders of
    /// one mount are ordered by unique id).
    fn seat_for(
        &self,
        rider_unique_id: i64,
        rider: &super::ActorSnapshot,
        mount: &super::ActorSnapshot,
    ) -> Option<RideSeat> {
        let degrees = |key| match rider.metadata.get(&key) {
            Some(ActorMetadataValue::Float(value)) if value.is_finite() => Some(*value),
            _ => None,
        };
        if let Some(ActorMetadataValue::Vector(seat)) = rider.metadata.get(&KEY_SEAT_OFFSET) {
            return seat.iter().all(|axis| axis.is_finite()).then(|| RideSeat {
                position: *seat,
                min_riders: 0,
                max_riders: u32::MAX,
                rotate_by: degrees(KEY_SEAT_ROTATION_DEGREES),
                lock_degrees: degrees(KEY_SEAT_LOCK_DEGREES),
            });
        }
        let protocol::ActorKind::Entity { identifier } = &mount.kind else {
            return None;
        };
        let mut riders: Vec<i64> = self
            .rider_to_ridden
            .iter()
            .filter(|(_, ridden)| **ridden == mount.unique_id)
            .map(|(rider, _)| *rider)
            .collect();
        riders.sort_unstable();
        let index = riders.iter().position(|rider| *rider == rider_unique_id)?;
        self.seat_defaults
            .seat(mount, identifier.as_ref(), riders.len() as u32, index)
    }

    /// Feet position and body yaw of `rider_unique_id` on its mount's seat, when linked and known.
    pub(crate) fn rider_seat_pose(&self, rider_unique_id: i64) -> Option<([f32; 3], f32)> {
        let ridden = self.rider_to_ridden.get(&rider_unique_id)?;
        let rider = self
            .actors
            .get(self.unique_to_runtime.get(&rider_unique_id)?)?;
        let mount = self.actors.get(self.unique_to_runtime.get(ridden)?)?;
        let seat = self.seat_for(rider_unique_id, rider, mount)?;
        let offset = seat_world_offset(seat.position, mount.yaw);
        Some((
            std::array::from_fn(|axis| mount.position[axis] + offset[axis]),
            wrap_degrees(mount.yaw + seat.rotate_by.unwrap_or(0.0)),
        ))
    }

    /// Seats for every linked rider from the mount poses `mount_pose` reports; the local rig is
    /// client-fed and skipped.
    fn rider_placements(
        &self,
        mount_pose: impl Fn(&super::ActorSnapshot) -> ([f32; 3], f32),
    ) -> Vec<Placement> {
        self.rider_to_ridden
            .iter()
            .filter_map(|(rider_unique_id, ridden)| {
                let rider_id = *self.unique_to_runtime.get(rider_unique_id)?;
                if self.remote_state_excluded_runtime_id == Some(rider_id) {
                    return None;
                }
                let mount = self.actors.get(self.unique_to_runtime.get(ridden)?)?;
                let rider = self.actors.get(&rider_id)?;
                let seat = self.seat_for(*rider_unique_id, rider, mount)?;
                let (mount_position, mount_yaw) = mount_pose(mount);
                let offset = seat_world_offset(seat.position, mount_yaw);
                Some(Placement {
                    runtime_id: rider_id,
                    position: std::array::from_fn(|axis| mount_position[axis] + offset[axis]),
                    body_yaw: wrap_degrees(mount_yaw + seat.rotate_by.unwrap_or(0.0)),
                    // A negative lock is odd server data and is skipped.
                    lock_degrees: seat
                        .lock_degrees
                        .filter(|degrees| (0.0..UNLOCKED_HEAD_DEGREES).contains(degrees)),
                })
            })
            .collect()
    }

    /// Places each linked rider at its seat and turns its body with the mount; riders with no
    /// known seat keep their streamed pose. The local rig is client-fed and skipped.
    pub(super) fn seat_riders(&mut self) {
        if self.rider_to_ridden.is_empty() {
            return;
        }
        for placement in self.rider_placements(|mount| (mount.position, mount.yaw)) {
            let Some(rider) = self.actors.get_mut(&placement.runtime_id) else {
                continue;
            };
            let mut state = rider.motion_state();
            state.seat(&placement);
            rider.apply_motion_state(state);
        }
    }

    /// Seats predicted riders exactly as [`Self::seat_riders`] seats live ones.
    pub(super) fn seat_pick_riders(&mut self) {
        if self.rider_to_ridden.is_empty() {
            return;
        }
        let placements = self.rider_placements(|mount| {
            self.pick_state(mount.runtime_id)
                .map_or((mount.position, mount.yaw), |state| {
                    (state.pose.position, state.pose.yaw)
                })
        });
        for placement in placements {
            if let Ok(index) = self
                .pick_states
                .binary_search_by_key(&placement.runtime_id, |(runtime_id, _)| *runtime_id)
            {
                self.pick_states[index].1.seat(&placement);
            }
        }
    }

    /// Authoritative body boxes for the caller's native liquid-contact sampling.
    pub(crate) fn fluid_probes(&self) -> Vec<ActorFluidProbe> {
        self.actors
            .values()
            .filter_map(|actor| {
                let (min, max) = actor.bounding_box()?;
                Some(ActorFluidProbe {
                    runtime_id: actor.runtime_id,
                    min,
                    max,
                })
            })
            .collect()
    }

    /// Bed block under every sleeping actor: the streamed bed position, else the block it lies in.
    pub(crate) fn bed_sample_points(&self) -> Vec<(u64, [i32; 3])> {
        self.actors
            .values()
            .filter(|actor| actor.is_sleeping())
            .map(|actor| {
                let block = match actor.metadata.get(&KEY_BED_POSITION) {
                    Some(ActorMetadataValue::BlockPosition(block)) => *block,
                    _ => actor.position.map(|axis| axis.floor() as i32),
                };
                (actor.runtime_id, block)
            })
            .collect()
    }

    /// Replaces every actor's sampled bed rotation with `(runtime_id, degrees)` samples.
    pub(crate) fn set_bed_rotations(&mut self, samples: &[(u64, f32)]) {
        for actor in self.actors.values_mut() {
            actor.status.sleep_rotation = None;
        }
        for &(runtime_id, degrees) in samples {
            if let Some(actor) = self.actors.get_mut(&runtime_id) {
                actor.status.sleep_rotation = Some(degrees);
            }
        }
    }

    /// Records the view's `[pitch, yaw]` (degrees) for camera-facing animations.
    pub(crate) fn set_camera_rotation(&mut self, rotation: [f32; 2]) {
        if rotation.iter().all(|value| value.is_finite()) {
            self.camera_rotation = rotation;
        }
    }

    pub(crate) fn set_animation_view(
        &mut self,
        view: Option<crate::actor_animation::ActorAnimationView>,
    ) {
        self.animation_view = view;
    }

    pub(crate) fn set_local_body_enabled(&mut self, enabled: bool) {
        self.local_view_dirty |= self.animation.set_local_body_enabled(enabled);
    }

    /// Records the view's world position for camera-relative animation queries.
    pub(crate) fn set_camera_position(&mut self, position: [f32; 3]) {
        if position.iter().all(|value| value.is_finite()) {
            self.camera_position = position;
        }
    }

    /// Stores `(runtime_id, in_water, in_lava)` samples on their actors.
    pub(crate) fn set_fluids(&mut self, samples: &[(u64, bool, bool)]) {
        for &(runtime_id, water, lava) in samples {
            if let Some(actor) = self.actors.get_mut(&runtime_id) {
                actor.status.fluid = Some((water, lava));
            }
        }
    }
}

/// Feet-anchored body bounds from retained collision metadata, for fluid animation queries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorFluidProbe {
    pub runtime_id: u64,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use protocol::ActorMetadataValue;

    use super::{FLAG_SADDLED, RideSeat, SeatDefaults, SeatRequirement, seat_world_offset};
    use crate::{ActorPose, ActorSnapshot};
    use protocol::ActorKind;

    #[test]
    fn actor_fluid_probes_use_the_streamed_body_bounds() {
        use crate::actor_store::{
            ActorStore, BOUNDING_BOX_HEIGHT_METADATA_KEY, BOUNDING_BOX_WIDTH_METADATA_KEY,
            tests::spawn,
        };
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, spawn(7, 70));
        let actor = store.actors.get_mut(&7).unwrap();
        actor.metadata.insert(
            BOUNDING_BOX_WIDTH_METADATA_KEY,
            ActorMetadataValue::Float(0.4),
        );
        actor.metadata.insert(
            BOUNDING_BOX_HEIGHT_METADATA_KEY,
            ActorMetadataValue::Float(0.4),
        );
        let probes = store.fluid_probes();
        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0].runtime_id, 7);
        assert_eq!(probes[0].min, [0.8, 2.0, 2.8]);
        assert_eq!(probes[0].max, [1.2, 2.4, 3.2]);
    }

    /// A rider whose mount itself rides gets the predicted pick pose a multi-tick frame's live
    /// per-tick seating gives it.
    #[test]
    fn nested_rider_pick_pose_matches_the_live_frame() {
        use super::{KEY_SEAT_OFFSET, KEY_SEAT_ROTATION_DEGREES};
        use crate::actor_store::{ActorStore, tests::spawn};
        let mut store = ActorStore::new(1, 0);
        for (sequence, runtime_id) in [(1, 7), (2, 8), (3, 9)] {
            store.apply(1, sequence, spawn(runtime_id, runtime_id as i64 * 10));
        }
        // 90 rides 80, which rides 70; turned seats make every level depend on the one below.
        store.rider_to_ridden.insert(80, 70);
        store.rider_to_ridden.insert(90, 80);
        for runtime_id in [8, 9] {
            let rider = store.actors.get_mut(&runtime_id).unwrap();
            rider
                .metadata
                .insert(KEY_SEAT_OFFSET, ActorMetadataValue::Vector([0.0, 1.0, 0.5]));
            rider
                .metadata
                .insert(KEY_SEAT_ROTATION_DEGREES, ActorMetadataValue::Float(30.0));
        }
        store.apply(
            1,
            4,
            protocol::ActorEvent::Move(protocol::ActorMoveEvent {
                dimension: 0,
                runtime_id: 7,
                position: [Some(4.0), Some(2.0), Some(6.0)],
                position_origin: protocol::ActorPositionOrigin::Feet,
                pitch: None,
                yaw: Some(90.0),
                head_yaw: Some(90.0),
                on_ground: None,
                teleported: false,
                player_mode: None,
                source_tick: None,
                interpolation: protocol::ActorInterpolation {
                    ticks: 3,
                    force_completion: false,
                },
            }),
        );

        store.predict_remote_motion(3);
        let predicted = [8, 9].map(|runtime_id| store.pick_pose(runtime_id).unwrap());
        store.advance_interpolation_ticks(3);
        let live = [8, 9].map(|runtime_id| {
            let actor = &store.actors[&runtime_id];
            (actor.position, actor.yaw)
        });
        assert_eq!(predicted, live);
    }

    fn mount(flags: u64) -> ActorSnapshot {
        let pose = ActorPose {
            position: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
        };
        ActorSnapshot {
            unique_id: -1,
            runtime_id: 1,
            spawn_revision: 1,
            movement_revision: 0,
            kind: ActorKind::Entity {
                identifier: "minecraft:test".into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            previous_pose: pose,
            received_pose: pose,
            interpolation_ticks_remaining: 0,
            body_yaw: 0.0,
            on_ground: Some(false),
            teleported: false,
            player_mode: None,
            player_game_mode: None,
            source_tick: None,
            metadata: HashMap::from([(0, ActorMetadataValue::Flags(flags))]),
            attributes: HashMap::new(),
            int_properties: HashMap::new(),
            float_properties: HashMap::new(),
            status: Default::default(),
            dragon_animation: None,
            hitboxes: None,
        }
    }

    // Seats follow rider count and index; the layout with the most satisfied requirements wins.
    #[test]
    fn default_seats_follow_rider_count_and_index() {
        let mut defaults = SeatDefaults::default();
        let seat = |z, min, max| RideSeat {
            position: [0.0, 1.0, z],
            min_riders: min,
            max_riders: max,
            rotate_by: None,
            lock_degrees: None,
        };
        defaults.insert(
            "minecraft:camel",
            Vec::new(),
            vec![seat(0.5, 0, 2), seat(-0.5, 1, 2)],
        );
        defaults.insert(
            "minecraft:camel",
            vec![SeatRequirement::Saddled(true)],
            vec![seat(2.0, 0, 2)],
        );
        let plain = mount(0);
        let saddled = mount(1 << FLAG_SADDLED);
        let at = |actor, riders, index| {
            defaults
                .seat(actor, "minecraft:camel", riders, index)
                .map(|seat| seat.position)
        };
        assert_eq!(at(&plain, 1, 0), Some([0.0, 1.0, 0.5]));
        assert_eq!(at(&plain, 2, 1), Some([0.0, 1.0, -0.5]));
        assert_eq!(at(&saddled, 1, 0), Some([0.0, 1.0, 2.0]));
        assert_eq!(defaults.seat(&plain, "minecraft:pig", 1, 0), None);
    }

    #[test]
    fn seat_offset_rotates_about_the_vertical_axis() {
        let world = seat_world_offset([1.0, 0.5, 0.0], 90.0);
        assert!((world[0]).abs() < 1.0e-6 && (world[2] - 1.0).abs() < 1.0e-6);
        assert_eq!(world[1], 0.5);
        assert_eq!(seat_world_offset([0.0, 0.0, 2.0], 0.0), [0.0, 0.0, 2.0]);
    }
}
