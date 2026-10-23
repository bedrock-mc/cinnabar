use protocol::ActorMetadataValue;
use protocol_ids::ActorDataKey;

use super::ActorStore;

/// Head turn, in degrees either side of the body, at or beyond which a seat does not limit it.
const UNLOCKED_HEAD_DEGREES: f32 = 180.0;

/// World offset of a mount-local seat `[right, up, forward]` for a mount facing `yaw_degrees`.
pub fn seat_world_offset(local: [f32; 3], yaw_degrees: f32) -> [f32; 3] {
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
    /// Admitted literal body rotation; unresolved expressions do not produce a seat.
    pub rotate_by: Option<f32>,
    /// Degrees the rider's head may turn either side of its body; `None` leaves it free.
    pub lock_degrees: Option<f32>,
}

impl super::movement_interpolation::MotionState {
    /// Holds a rider on its seat: position and body follow the mount, the head within the lock.
    fn seat(&mut self, placement: &Placement) {
        self.received.position = placement.position;
        self.received.y_origin = Default::default();
        self.pose.position = placement.position;
        self.pose.y_origin = Default::default();
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

/// How a locked seat holds its rider's view, in degrees: the vehicle's render yaw, the
/// seat's facing, and how far either side of that facing the view may turn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeatViewLock {
    pub vehicle_yaw: f32,
    /// The seat's facing relative to the vehicle.
    pub offset: f32,
    pub limit: f32,
}

impl SeatViewLock {
    /// The seat's facing in the world.
    #[must_use]
    pub fn facing(&self) -> f32 {
        wrap_degrees(self.vehicle_yaw + self.offset)
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
    /// Snaps local render endpoints and optionally reanchors animation on a presentation transition.
    pub(crate) fn set_local_presentation_position(
        &mut self,
        runtime_id: u64,
        position: [f32; 3],
        reanchor: bool,
    ) {
        if !position.iter().all(|value| value.is_finite()) {
            return;
        }
        let Some(actor) = self.actors.get_mut(&runtime_id) else {
            return;
        };
        actor.position = position;
        actor.previous_pose.position = position;
        actor.previous_pose.y_origin = Default::default();
        actor.received_pose.position = position;
        actor.received_pose.y_origin = Default::default();
        actor.interpolation_ticks_remaining = 0;
        actor.status.movement_interpolation = Default::default();
        self.effect_members.collidable.update(actor);
        if reanchor {
            self.animation.reanchor_position(runtime_id, position);
        }
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
        if let Some(ActorMetadataValue::Vector(seat)) =
            rider.metadata.get(&ActorDataKey::SeatOffset.wire_value())
        {
            return seat.iter().all(|axis| axis.is_finite()).then(|| RideSeat {
                // Streamed offsets join native actor origins; the store keeps collision feet.
                position: [
                    seat[0],
                    seat[1] + mount.network_position_offset() - rider.network_position_offset(),
                    seat[2],
                ],
                min_riders: 0,
                max_riders: u32::MAX,
                rotate_by: degrees(ActorDataKey::SeatRotationOffsetDegrees.wire_value()),
                lock_degrees: degrees(ActorDataKey::SeatLockPassengerRotationDegrees.wire_value()),
            });
        }
        let descriptor = self.seat_bindings.get(&mount.runtime_id)?.as_ref()?;
        let admission = self.entity_assets.as_ref()?.seat_descriptor(*descriptor)?;
        let mut riders = 0_u32;
        let mut index = 0_usize;
        let mut linked = false;
        for (&id, &ridden) in &self.rider_to_ridden {
            if ridden == mount.unique_id {
                riders += 1;
                index += usize::from(id < rider_unique_id);
                linked |= id == rider_unique_id;
            }
        }
        linked
            .then(|| compiled_seat(admission, riders, index))
            .flatten()
    }

    /// Feet position and body yaw of `rider_unique_id` on its mount's seat, when linked and known.
    pub(crate) fn rider_seat_pose(&self, rider_unique_id: i64) -> Option<([f32; 3], f32)> {
        let ridden = self.rider_to_ridden.get(&rider_unique_id)?;
        let mount = self.actors.get(self.unique_to_runtime.get(ridden)?)?;
        self.rider_seat_on(rider_unique_id, mount.position, mount.yaw)
    }

    /// Feet position and body yaw of `rider_unique_id` on its mount's seat, were the mount's
    /// feet at `mount_feet` facing `mount_yaw`.
    pub(crate) fn rider_seat_on(
        &self,
        rider_unique_id: i64,
        mount_feet: [f32; 3],
        mount_yaw: f32,
    ) -> Option<([f32; 3], f32)> {
        let ridden = self.rider_to_ridden.get(&rider_unique_id)?;
        let rider = self
            .actors
            .get(self.unique_to_runtime.get(&rider_unique_id)?)?;
        let mount = self.actors.get(self.unique_to_runtime.get(ridden)?)?;
        let seat = self.seat_for(rider_unique_id, rider, mount)?;
        let offset = seat_world_offset(seat.position, mount_yaw);
        Some((
            std::array::from_fn(|axis| mount_feet[axis] + offset[axis]),
            wrap_degrees(mount_yaw + seat.rotate_by.unwrap_or(0.0)),
        ))
    }

    /// The view lock of `rider_unique_id`'s seat, with its vehicle's yaw `alpha` of the way
    /// through the current tick; `None` unless the server locks the seat's rotation.
    pub(crate) fn rider_view_lock(&self, rider_unique_id: i64, alpha: f32) -> Option<SeatViewLock> {
        let rider = self.snapshot_by_unique(rider_unique_id)?;
        let locked = match rider
            .metadata
            .get(&ActorDataKey::SeatLockPassengerRotation.wire_value())
        {
            Some(ActorMetadataValue::Byte(value)) => *value != 0,
            Some(ActorMetadataValue::Int(value)) => *value != 0,
            _ => false,
        };
        let degrees = |key: ActorDataKey| match rider.metadata.get(&key.wire_value()) {
            Some(ActorMetadataValue::Float(value)) if value.is_finite() => Some(*value),
            _ => None,
        };
        let limit = degrees(ActorDataKey::SeatLockPassengerRotationDegrees)?;
        if !locked || limit < 0.0 {
            return None;
        }
        let offset = degrees(ActorDataKey::SeatRotationOffsetDegrees).unwrap_or(0.0);
        let mount = self.snapshot_by_unique(*self.rider_to_ridden.get(&rider_unique_id)?)?;
        // Before the live actors advance this frame, the mount turns as picks predict it.
        let (previous, current) = self
            .pick_ahead(mount.runtime_id)
            .map_or((mount.previous_pose.yaw, mount.yaw), |pick| {
                (pick.previous_yaw, pick.state.pose.yaw)
            });
        let vehicle_yaw = previous + wrap_degrees(current - previous) * alpha.clamp(0.0, 1.0);
        vehicle_yaw.is_finite().then(|| SeatViewLock {
            vehicle_yaw,
            offset,
            limit,
        })
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
            self.effect_members.collidable.update(rider);
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
                .binary_search_by_key(&placement.runtime_id, |pick| pick.runtime_id)
            {
                self.pick_states[index].state.seat(&placement);
            }
        }
    }

    /// Authoritative body boxes for the caller's native liquid-contact sampling.
    pub(crate) fn fluid_probes(&self) -> impl Iterator<Item = ActorFluidProbe> + '_ {
        self.actors.values().filter_map(|actor| {
            let (min, max) = actor.bounding_box()?;
            Some(ActorFluidProbe {
                runtime_id: actor.runtime_id,
                min,
                max,
            })
        })
    }

    /// The streamed bed block of every sleeping actor; sleepers without a bed position are skipped.
    pub(crate) fn bed_sample_points(&self) -> Vec<(u64, [i32; 3])> {
        self.effect_members
            .sleepers
            .iter()
            .map(|(&runtime, &bed)| (runtime, bed))
            .collect()
    }

    /// Replaces current sleepers' headings; unavailable beds lose their previous sample.
    pub(crate) fn set_bed_rotations(&mut self, samples: &[(u64, f32)]) {
        for runtime in self.effect_members.sleepers.keys() {
            if let Some(actor) = self.actors.get_mut(runtime) {
                actor.status.sleep_rotation = None;
            }
        }
        for &(runtime_id, degrees) in samples {
            if self.effect_members.sleepers.contains_key(&runtime_id)
                && let Some(actor) = self.actors.get_mut(&runtime_id)
            {
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

    pub(crate) fn set_animation_view(&mut self, view: Option<actor_animation::ActorAnimationView>) {
        self.animation_view = view;
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
    use protocol::ActorMetadataValue;
    use protocol_ids::ActorDataKey;

    use super::seat_world_offset;

    #[test]
    fn actor_fluid_probes_use_the_streamed_body_bounds() {
        use crate::actor_store::{ActorStore, tests::spawn};
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, spawn(7, 70));
        let actor = store.actors.get_mut(&7).unwrap();
        actor.metadata.insert(
            ActorDataKey::Width.wire_value(),
            ActorMetadataValue::Float(0.4),
        );
        actor.metadata.insert(
            ActorDataKey::Height.wire_value(),
            ActorMetadataValue::Float(0.4),
        );
        let probes: Vec<_> = store.fluid_probes().collect();
        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0].runtime_id, 7);
        assert_eq!(probes[0].min, [0.8, 2.0, 2.8]);
        assert_eq!(probes[0].max, [1.2, 2.4, 3.2]);
    }

    /// A rider whose mount itself rides gets the predicted pick pose a multi-tick frame's live
    /// per-tick seating gives it.
    #[test]
    fn nested_rider_pick_pose_matches_the_live_frame() {
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
            rider.metadata.insert(
                ActorDataKey::SeatOffset.wire_value(),
                ActorMetadataValue::Vector([0.0, 1.0, 0.5]),
            );
            rider.metadata.insert(
                ActorDataKey::SeatRotationOffsetDegrees.wire_value(),
                ActorMetadataValue::Float(30.0),
            );
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
                force_local_move: false,
                player_mode: None,
                source_tick: None,
                interpolation: protocol::ActorInterpolation {
                    ticks: 3,
                    force_completion: false,
                },
            }),
        );

        store.predict_remote_motion(3);
        let predicted = [8, 9].map(|runtime_id| {
            let pose = store.pick_pose(runtime_id).unwrap();
            (pose.position, pose.yaw)
        });
        store.advance_interpolation_ticks(3);
        let live = [8, 9].map(|runtime_id| {
            let actor = &store.actors[&runtime_id];
            (actor.position, actor.yaw)
        });
        assert_eq!(predicted, live);
    }

    #[test]
    fn seat_offset_rotates_about_the_vertical_axis() {
        let world = seat_world_offset([1.0, 0.5, 0.0], 90.0);
        assert!((world[0]).abs() < 1.0e-6 && (world[2] - 1.0).abs() < 1.0e-6);
        assert_eq!(world[1], 0.5);
        assert_eq!(seat_world_offset([0.0, 0.0, 2.0], 0.0), [0.0, 0.0, 2.0]);
    }
}

/// Selects only admitted static layouts; unresolved activation or rotation retains streamed pose.
fn compiled_seat(
    admission: &assets::entity_seats::EntitySeatAdmission,
    riders: u32,
    index: usize,
) -> Option<RideSeat> {
    use assets::entity_seats::{EntitySeatAdmission, SeatRotation, SeatSelector};
    let EntitySeatAdmission::Provisional { layouts, .. } = admission else {
        return None;
    };
    if layouts
        .iter()
        .any(|layout| !matches!(layout.selector, SeatSelector::Base))
    {
        return None;
    }
    let layout = layouts
        .iter()
        .find(|layout| layout.selector == SeatSelector::Base)?;
    let mut usable = layout
        .seats
        .iter()
        .filter(|seat| (seat.min_riders..=seat.max_riders).contains(&riders));
    let seat = usable.clone().nth(index).or_else(|| usable.next_back())?;
    let rotate_by = match seat.rotation {
        SeatRotation::Absent => None,
        SeatRotation::Literal(degrees) => Some(degrees.get()),
        SeatRotation::Expression { .. } | SeatRotation::Unresolved => return None,
    };
    Some(RideSeat {
        position: seat.position.map(|value| value.get()),
        min_riders: seat.min_riders,
        max_riders: seat.max_riders,
        rotate_by,
        lock_degrees: seat.lock_degrees.map(|value| value.get()),
    })
}

impl ActorStore {
    /// Resolves one actor's base once at spawn; placement reads only the numerical binding.
    pub(super) fn bind_actor_seat(&mut self, actor: &super::ActorSnapshot) {
        let descriptor = match &actor.kind {
            protocol::ActorKind::Entity { identifier } => self
                .seat_bases
                .get(identifier.as_ref())
                .copied()
                .unwrap_or_else(|| {
                    self.entity_assets
                        .as_ref()?
                        .seat_descriptor_index(identifier)
                }),
            protocol::ActorKind::Player { .. } => None,
        };
        self.seat_bindings.insert(actor.runtime_id, descriptor);
    }

    /// Retains the first advertised base and refreshes actors that preceded the registry.
    pub(super) fn apply_seat_bases(&mut self, registry: &protocol::ActorIdentifierRegistry) {
        for entry in registry.entries.iter() {
            if self.seat_bases.contains_key(entry.identifier.as_ref())
                || self.seat_bases.len() >= protocol::MAX_ACTOR_IDENTIFIERS
            {
                continue;
            }
            let base = if entry.base_identifier.is_empty() {
                &entry.identifier
            } else {
                &entry.base_identifier
            };
            let descriptor = self
                .entity_assets
                .as_ref()
                .and_then(|assets| assets.seat_descriptor_index(base));
            self.seat_bases
                .insert(std::sync::Arc::clone(&entry.identifier), descriptor);
        }
        for actor in self.actors.values() {
            let descriptor = match &actor.kind {
                protocol::ActorKind::Entity { identifier } => self
                    .seat_bases
                    .get(identifier.as_ref())
                    .copied()
                    .unwrap_or_else(|| {
                        self.entity_assets
                            .as_ref()?
                            .seat_descriptor_index(identifier)
                    }),
                protocol::ActorKind::Player { .. } => None,
            };
            self.seat_bindings.insert(actor.runtime_id, descriptor);
        }
    }
}

#[cfg(test)]
mod compiled_seat_tests {
    use super::*;
    use assets::entity_seats::{
        CompiledRideSeat, CompiledSeatLayout, EntitySeatAdmission, SeatRotation, SeatSelector,
    };

    /// Creates an original static count interval without installed carrier data.
    fn seat(z: f32, min: u32, max: u32) -> CompiledRideSeat {
        CompiledRideSeat {
            position: [0.0, 1.0, z].map(|value| assets::ItemDisplayScalar::new(value).unwrap()),
            min_riders: min,
            max_riders: max,
            rotation: SeatRotation::Absent,
            lock_degrees: None,
        }
    }

    /// Gives the store an original checked literal seat instead of an installed behavior pack.
    fn store() -> ActorStore {
        let mut compiled = assets::carriers::startup::test_carriers::synthetic_entity_assets(1);
        compiled.sources = Box::from([
            assets::EntityAssetSource {
                path: "behavior/entities/fixture.json".into(),
                source_bytes: 1,
                source_sha256: [2; 32],
            },
            compiled.sources[0].clone(),
        ]);
        compiled.symbols[0].source_index = 1;
        compiled.seat_layouts[0].admission = EntitySeatAdmission::Provisional {
            source: 0,
            layouts: Box::from([CompiledSeatLayout {
                selector: SeatSelector::Base,
                seats: Box::from([seat(0.5, 0, 2)]),
            }]),
        };
        let bytes = assets::encode_entity_blob(&compiled).unwrap();
        ActorStore::new_with_entity_assets(
            1,
            0,
            std::sync::Arc::new(assets::RuntimeEntityAssets::decode(&bytes).unwrap()),
        )
    }

    /// Admits one authored custom base through the real streamed registry path.
    fn advertise(store: &mut ActorStore, sequence: u64, base: &str) {
        store.apply(
            1,
            sequence,
            protocol::ActorEvent::Identifiers(protocol::ActorIdentifierRegistry {
                entries: std::sync::Arc::from([protocol::ActorIdentifier {
                    identifier: "fixture:mount".into(),
                    base_identifier: base.into(),
                }]),
                skipped: 0,
            }),
        );
    }

    /// Spawns the same network identity with an unrelated display name.
    fn spawn_mount(store: &mut ActorStore, sequence: u64) {
        let protocol::ActorEvent::Spawn(mut event) = super::super::tests::spawn(8, 80) else {
            unreachable!();
        };
        event.kind = protocol::ActorKind::Entity {
            identifier: "fixture:mount".into(),
        };
        store.apply(1, sequence, protocol::ActorEvent::Spawn(event));
    }

    #[test]
    fn seat_base_admission_refreshes_early_actors_and_keeps_the_first_definition() {
        let mut store = store();
        spawn_mount(&mut store, 1);
        assert_eq!(store.seat_bindings.get(&8), Some(&None));
        advertise(&mut store, 2, "minecraft:allay");
        assert_eq!(store.seat_bindings.get(&8), Some(&Some(0)));
        advertise(&mut store, 3, "fixture:unadmitted");
        assert_eq!(store.seat_bindings.get(&8), Some(&Some(0)));
        spawn_mount(&mut store, 4);
        assert_eq!(store.seat_bindings.get(&8), Some(&Some(0)));
        store.begin_session(2, 0);
        assert!(store.seat_bindings.is_empty());
        assert!(store.seat_bases.is_empty());
    }

    #[test]
    fn an_unknown_declared_seat_base_stays_unresolved_after_actor_admission() {
        let mut store = store();
        advertise(&mut store, 1, "fixture:unadmitted");
        spawn_mount(&mut store, 2);
        assert_eq!(store.seat_bindings.get(&8), Some(&None));
        advertise(&mut store, 3, "minecraft:allay");
        assert_eq!(store.seat_bindings.get(&8), Some(&None));
    }

    #[test]
    fn compiled_base_seats_preserve_count_intervals_and_compatibility_order() {
        let admission = EntitySeatAdmission::Provisional {
            source: 0,
            layouts: Box::from([CompiledSeatLayout {
                selector: SeatSelector::Base,
                seats: Box::from([seat(0.5, 0, 2), seat(-0.5, 2, 2)]),
            }]),
        };
        assert_eq!(
            compiled_seat(&admission, 1, 0).unwrap().position,
            [0.0, 1.0, 0.5]
        );
        assert_eq!(
            compiled_seat(&admission, 2, 1).unwrap().position,
            [0.0, 1.0, -0.5]
        );
        assert_eq!(
            compiled_seat(&admission, 2, 7).unwrap().position,
            [0.0, 1.0, -0.5]
        );
        assert_eq!(compiled_seat(&admission, 3, 0), None);
    }

    #[test]
    fn unknown_group_or_rotation_does_not_silently_use_a_base_or_zero_angle() {
        let admission = EntitySeatAdmission::Provisional {
            source: 0,
            layouts: Box::from([
                CompiledSeatLayout {
                    selector: SeatSelector::Base,
                    seats: Box::from([seat(0.5, 0, 2)]),
                },
                CompiledSeatLayout {
                    selector: SeatSelector::UnresolvedComponentGroup { group: 2 },
                    seats: Box::from([seat(2.0, 0, 2)]),
                },
            ]),
        };
        assert_eq!(compiled_seat(&admission, 1, 0), None);
        let mut expression_seat = seat(0.5, 0, 2);
        expression_seat.rotation = SeatRotation::Expression { expression: 0 };
        let admission = EntitySeatAdmission::Provisional {
            source: 0,
            layouts: Box::from([CompiledSeatLayout {
                selector: SeatSelector::Base,
                seats: Box::from([expression_seat]),
            }]),
        };
        assert_eq!(compiled_seat(&admission, 1, 0), None);
        assert_eq!(compiled_seat(&EntitySeatAdmission::Unresolved, 1, 0), None);
    }
}

impl super::ActorSnapshot {
    /// The bed block the server streams for this actor.
    #[must_use]
    pub fn bed_position(&self) -> Option<[i32; 3]> {
        match self.metadata.get(&ActorDataKey::BedPosition.wire_value()) {
            Some(ActorMetadataValue::BlockPosition(block)) => Some(*block),
            _ => None,
        }
    }

    /// The bed this actor sleeps in and its sampled sleep rotation in degrees, while sleeping. A
    /// bed not yet sampled (unloaded) reads 0 degrees, as the native rotation does for a
    /// position holding no bed.
    #[must_use]
    pub fn sleeping_bed(&self) -> Option<([i32; 3], f32)> {
        self.is_sleeping()
            .then(|| self.bed_position())
            .flatten()
            .map(|bed| (bed, self.status.sleep_rotation.unwrap_or(0.0)))
    }
}
