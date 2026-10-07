use protocol::{ActorMetadataValue, ActorStatusEvent, ActorStatusKind, ActorTakeItemEvent};

use super::{ActorApplyResult, ActorSnapshot, ActorStore};

/// Ticks the hurt tint and hurt-driven animations stay active; needs independent measurement.
pub const HURT_DURATION_TICKS: u8 = 10;
/// Alpha of the red damage overlay while hurt or dying; needs independent measurement.
pub const HURT_OVERLAY_ALPHA: f32 = 0.4;
/// Ticks a dying actor takes to tip fully over; needs independent measurement.
pub const DEATH_DURATION_TICKS: u8 = 20;

/// Ticks a picked-up item takes to reach its collector; needs independent measurement.
pub const PICKUP_DURATION_TICKS: u8 = 3;

/// Sequences a knockback impulse stays attributable to a hurt event; needs measurement.
const KNOCKBACK_FRESH_SEQUENCES: u64 = 32;

const HURT_DIRECTION_METADATA_KEY: u32 = 12;

/// Most undrained status notices retained; further ones are dropped.
pub const MAX_STATUS_NOTICES: usize = 256;

/// A decoded actor status event with the actor's pose at the time, for particle and sound consumers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorStatusNotice {
    pub runtime_id: u64,
    pub kind: ActorStatusKind,
    pub data: i32,
    /// Actor feet position.
    pub position: [f32; 3],
    /// Bounding-box height, when the actor streams one.
    pub height: Option<f32>,
}

/// A dropped item flying to the actor that collected it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorPickup {
    pub collector_runtime_id: u64,
    /// Ticks elapsed, saturating at [`PICKUP_DURATION_TICKS`].
    pub ticks: u8,
}

/// Client-derived damage and death presentation state, advanced per tick.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ActorStatus {
    pub(super) terrain_interlock: super::terrain_interlock::TerrainInterlock,
    pub(super) movement_interpolation: super::movement_interpolation::MovementInterpolation,
    /// Vanilla velocity per tick, distinct from query-derived movement speed.
    pub(crate) native_velocity: [f32; 3],
    /// Ticks of hurt state remaining.
    pub hurt_time: u8,
    /// Signed native shake countdown, set verbatim by ActorEvent::Shake.
    pub shake_time: i32,
    /// The current hurt came without damage, so it shows no red flash.
    pub skip_red_flash: bool,
    /// Server-streamed hurt direction, when the server provides one.
    pub hurt_direction: Option<f32>,
    /// Ticks elapsed since death, saturating at [`DEATH_DURATION_TICKS`].
    pub death_time: u8,
    pub(crate) dragon_death_time: u16,
    pub(crate) cloud_start_tick: Option<u32>,
    pub(crate) cloud_particles_expired: bool,
    pub dead: bool,
    /// `age_ticks` when the fuse metadata was last received.
    pub fuse_age_ticks: u32,
    /// Ticks since the actor spawned; drives dropped-item spin and bob phase.
    pub age_ticks: u32,
    pub(super) fire: super::fire::FireAnimation,
    /// Runtime ID and spawn revision of the dragon's last selected healing crystal.
    pub(crate) healing_crystal: Option<(u64, u64)>,
    pub pickup: Option<ActorPickup>,
    /// Body water/lava contact; `None` before the first successful world sample.
    pub fluid: Option<(bool, bool)>,
    /// Breathing point below a liquid surface; `None` before the first world sample.
    pub breathing_submerged: Option<bool>,
    /// Bed orientation in degrees under a sleeping actor, sampled from the world.
    pub sleep_rotation: Option<f32>,
}

impl ActorStatus {
    /// Death ticks presented to animations, including the dragon's longer sequence.
    #[must_use]
    pub fn death_ticks(&self) -> u16 {
        if self.dragon_death_time != 0 {
            self.dragon_death_time
        } else {
            u16::from(self.death_time)
        }
    }

    /// Whether the red damage overlay should tint the actor this frame.
    #[must_use]
    pub fn overlay_active(&self) -> bool {
        (self.hurt_time > 0 && !self.skip_red_flash) || self.dead
    }

    /// Death tip-over progress in `0..=1` at `partial_tick`, or `None` while alive.
    #[must_use]
    pub fn death_progress(&self, partial_tick: f32) -> Option<f32> {
        if !self.dead {
            return None;
        }
        let ticks = f32::from(self.death_time) + partial_tick.clamp(0.0, 1.0);
        Some((ticks / f32::from(DEATH_DURATION_TICKS)).clamp(0.0, 1.0))
    }

    pub(super) fn tick(&mut self) {
        self.age_ticks = self.age_ticks.saturating_add(1);
        self.fire.tick(self.age_ticks);
        if let Some(pickup) = &mut self.pickup {
            pickup.ticks = pickup.ticks.saturating_add(1).min(PICKUP_DURATION_TICKS);
        }
        self.hurt_time = self.hurt_time.saturating_sub(1);
        // Vanilla's actor tick decrements only positive
        // shake values. Zero and well-formed negative server values stay unchanged.
        if self.shake_time > 0 {
            self.shake_time -= 1;
        }
        if self.dead && self.death_time < DEATH_DURATION_TICKS {
            self.death_time += 1;
        }
    }

    fn die(&mut self) {
        self.dead = true;
        self.hurt_time = HURT_DURATION_TICKS;
        self.skip_red_flash = false;
    }

    fn revive(&mut self) {
        self.hurt_time = 0;
        self.hurt_direction = None;
        self.death_time = 0;
        self.dragon_death_time = 0;
        self.dead = false;
    }
}

impl ActorSnapshot {
    /// Marks the actor dead when its health attribute reaches zero and alive when it recovers.
    pub(super) fn sync_status_from_health(&mut self) {
        let Some(health) = self.attributes.get("minecraft:health") else {
            return;
        };
        if !health.current.is_finite() {
            return;
        }
        if health.current <= 0.0 {
            if !self.status.dead {
                self.status.die();
            }
        } else if self.status.dead {
            self.status.revive();
        }
    }

    fn streamed_hurt_direction(&self) -> Option<f32> {
        match self.metadata.get(&HURT_DIRECTION_METADATA_KEY)? {
            ActorMetadataValue::Byte(value) => Some(f32::from(*value)),
            ActorMetadataValue::Short(value) => Some(f32::from(*value)),
            ActorMetadataValue::Int(value) => Some(*value as f32),
            _ => None,
        }
    }
}

impl ActorStore {
    pub(super) fn apply_status(&mut self, event: ActorStatusEvent) -> ActorApplyResult {
        let Some(actor) = self.actors.get_mut(&event.runtime_id) else {
            return ActorApplyResult::MissingActor;
        };
        if self.status_notices.len() < MAX_STATUS_NOTICES {
            self.status_notices.push(ActorStatusNotice {
                runtime_id: event.runtime_id,
                kind: event.kind,
                data: event.data,
                position: actor.position,
                height: actor.bounding_box().map(|(min, max)| max[1] - min[1]),
            });
        }
        match event.kind {
            ActorStatusKind::Hurt | ActorStatusKind::HurtWithoutDamage => {
                actor.status.hurt_time = HURT_DURATION_TICKS;
                actor.status.skip_red_flash = event.kind == ActorStatusKind::HurtWithoutDamage;
                actor.status.hurt_direction = actor.streamed_hurt_direction();
                self.animation.hurt_java_limbs(event.runtime_id);
            }
            ActorStatusKind::Death => {
                if matches!(&actor.kind, super::ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:ender_dragon")
                {
                    actor.status.dead = true;
                    actor.status.dragon_death_time = 1;
                } else if !actor.status.dead {
                    actor.status.die();
                }
            }
            ActorStatusKind::SpawnAlive => actor.status.revive(),
            // Entity event 39 (0x27) sets the shake countdown verbatim.
            ActorStatusKind::Shake => actor.status.shake_time = event.data,
            // Particle-only kinds have no retained actor state.
            _ => {}
        }
        ActorApplyResult::Updated
    }

    pub(super) fn apply_take_item(&mut self, event: ActorTakeItemEvent) -> ActorApplyResult {
        let Some(item) = self.actors.get_mut(&event.item_runtime_id) else {
            return ActorApplyResult::MissingActor;
        };
        item.status.pickup.get_or_insert(ActorPickup {
            collector_runtime_id: event.collector_runtime_id,
            ticks: 0,
        });
        ActorApplyResult::Updated
    }

    /// Drains the status events decoded since the last call, in arrival order.
    pub(crate) fn take_status_notices(&mut self) -> Vec<ActorStatusNotice> {
        std::mem::take(&mut self.status_notices)
    }

    /// Remembers the latest horizontal knockback impulse the local player received.
    pub(crate) fn note_local_knockback(&mut self, sequence: u64, motion: [f32; 3]) {
        if motion[0].hypot(motion[2]) > f32::EPSILON {
            self.local_knockback = Some((sequence, [motion[0], motion[2]]));
        }
    }

    /// Direction toward the damage source: opposite the recent knockback, if one is fresh.
    pub(crate) fn hurt_source_direction(&self, sequence: u64) -> Option<[f32; 2]> {
        let (noted, [x, z]) = self.local_knockback?;
        let length = x.hypot(z);
        (sequence.saturating_sub(noted) <= KNOCKBACK_FRESH_SEQUENCES && length > f32::EPSILON)
            .then(|| [-x / length, -z / length])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hurt_counts_down_and_death_saturates() {
        let mut status = ActorStatus {
            hurt_time: HURT_DURATION_TICKS,
            ..ActorStatus::default()
        };
        for _ in 0..HURT_DURATION_TICKS {
            assert!(status.overlay_active());
            status.tick();
        }
        assert!(!status.overlay_active());

        status.die();
        for _ in 0..(DEATH_DURATION_TICKS + 5) {
            status.tick();
        }
        assert_eq!(status.death_time, DEATH_DURATION_TICKS);
        assert_eq!(status.death_progress(0.9), Some(1.0));
        assert!(status.overlay_active());
    }

    fn spawn() -> protocol::ActorEvent {
        protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: 5,
            runtime_id: 7,
            kind: protocol::ActorKind::Entity {
                identifier: "minecraft:cow".into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: std::sync::Arc::from([]),
            attributes: std::sync::Arc::from([]),
            properties: std::sync::Arc::from([]),
            links: std::sync::Arc::from([]),
        })
    }

    fn status(kind: ActorStatusKind) -> protocol::ActorEvent {
        protocol::ActorEvent::Status(ActorStatusEvent {
            runtime_id: 7,
            kind,
            data: 0,
        })
    }

    #[test]
    fn hurt_event_arms_the_countdown_and_ticks_down() {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, spawn());
        assert_eq!(
            store.apply(1, 2, status(ActorStatusKind::Hurt)),
            ActorApplyResult::Updated
        );
        assert_eq!(store.get(7).unwrap().status.hurt_time, HURT_DURATION_TICKS);
        store.advance_interpolation_ticks(3);
        assert_eq!(
            store.get(7).unwrap().status.hurt_time,
            HURT_DURATION_TICKS - 3
        );
    }

    #[test]
    fn shake_event_uses_the_payload_and_only_completed_ticks_decrement_positive_values() {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, spawn());
        let shake = |data| {
            protocol::ActorEvent::Status(ActorStatusEvent {
                runtime_id: 7,
                kind: ActorStatusKind::Shake,
                data,
            })
        };
        assert_eq!(store.apply(1, 2, shake(12)), ActorApplyResult::Updated);
        assert_eq!(store.get(7).unwrap().status.shake_time, 12);
        store.advance_interpolation_ticks(0);
        assert_eq!(store.get(7).unwrap().status.shake_time, 12);
        store.advance_interpolation_ticks(3);
        assert_eq!(store.get(7).unwrap().status.shake_time, 9);
        store.advance_interpolation_ticks(10);
        assert_eq!(store.get(7).unwrap().status.shake_time, 0);

        for (sequence, data) in [(3, -1), (4, i32::MIN), (5, 0)] {
            assert_eq!(
                store.apply(1, sequence, shake(data)),
                ActorApplyResult::Updated
            );
            store.advance_interpolation_ticks(2);
            assert_eq!(store.get(7).unwrap().status.shake_time, data);
        }
        store.apply(1, 6, shake(i32::MAX));
        store.advance_interpolation_ticks(1);
        assert_eq!(store.get(7).unwrap().status.shake_time, i32::MAX - 1);
    }

    /// Event 81 arms the hurt countdown but never the red damage flash; a real hit restores it.
    #[test]
    fn hurt_without_damage_skips_the_red_flash() {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, spawn());
        store.apply(1, 2, status(ActorStatusKind::HurtWithoutDamage));
        let status_now = store.get(7).unwrap().status;
        assert_eq!(status_now.hurt_time, HURT_DURATION_TICKS);
        assert!(!status_now.overlay_active());
        store.apply(1, 3, status(ActorStatusKind::Hurt));
        assert!(store.get(7).unwrap().status.overlay_active());
    }

    #[test]
    fn take_item_starts_pickup_and_saturates() {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, spawn());
        let take = protocol::ActorEvent::TakeItem(ActorTakeItemEvent {
            item_runtime_id: 7,
            collector_runtime_id: 99,
        });
        assert_eq!(store.apply(1, 2, take), ActorApplyResult::Updated);
        store.advance_interpolation_ticks(u32::from(PICKUP_DURATION_TICKS) + 4);
        let status = store.get(7).unwrap().status;
        assert_eq!(
            status.pickup.map(|pickup| pickup.ticks),
            Some(PICKUP_DURATION_TICKS)
        );
        assert_eq!(
            status.pickup.map(|pickup| pickup.collector_runtime_id),
            Some(99)
        );
    }

    #[test]
    fn hurt_source_is_opposite_a_fresh_knockback() {
        let mut store = ActorStore::new(1, 0);
        assert_eq!(store.hurt_source_direction(1), None);
        store.note_local_knockback(5, [2.0, 0.3, 0.0]);
        assert_eq!(store.hurt_source_direction(6), Some([-1.0, 0.0]));
        assert_eq!(
            store.hurt_source_direction(5 + KNOCKBACK_FRESH_SEQUENCES + 1),
            None
        );
    }

    #[test]
    fn death_event_for_unknown_actor_is_missing() {
        let mut store = ActorStore::new(1, 0);
        assert_eq!(
            store.apply(1, 1, status(ActorStatusKind::Death)),
            ActorApplyResult::MissingActor
        );
    }

    #[test]
    fn revive_clears_death() {
        let mut status = ActorStatus::default();
        status.die();
        status.revive();
        assert_eq!(status.death_progress(0.0), None);
        assert!(!status.overlay_active());
    }
}
