//! Local health packets mutate actor authority before its UI projection is delivered.

use super::{ActorAttribute, ActorDamageState, ActorStore};

/// Damage state retained while the logical local player has no synthetic pose.
#[derive(Debug, Clone, Copy)]
pub(super) struct PendingLocalDamage {
    pub damage: ActorDamageState,
    pub hurt_time: u8,
}

impl PendingLocalDamage {
    /// Advances both counters by one completed actor tick.
    pub(super) fn tick(&mut self) {
        self.damage.remaining_ticks = self.damage.remaining_ticks.saturating_sub(1);
        self.hurt_time = self.hurt_time.saturating_sub(1);
    }
}

/// Initial health range of a player without a streamed health attribute.
pub const DEFAULT_PLAYER_HEALTH: f32 = 20.0;

impl ActorStore {
    /// Enables health-drop animations and resets the local death and hurt clocks at PlayerSpawn.
    pub(crate) fn mark_local_player_spawned(&mut self, runtime_id: u64) {
        self.local_player_spawned = true;
        if let Some(pending) = &mut self.pending_local_damage {
            pending.hurt_time = 0;
        }
        if let Some(actor) = self.actors.get_mut(&runtime_id) {
            actor.status.hurt_time = 0;
            actor.status.death_time = 0;
            actor.status.native_death_ticks = 0;
        }
    }

    /// Number of local health updates skipped for an unusable value, range, or attribute capacity.
    #[cfg(test)]
    pub fn local_health_skips(&self) -> u64 {
        self.local_health_skips
    }

    /// Applies SetHealth to the local actor, retaining it until the first pose when necessary.
    pub(crate) fn set_local_health(&mut self, runtime_id: u64, value: i32) {
        let Ok(value) = u16::try_from(value) else {
            self.skip_local_health();
            return;
        };
        let mut health = self
            .actors
            .get(&runtime_id)
            .and_then(|actor| actor.attributes.get("minecraft:health"))
            .cloned()
            .or_else(|| self.pending_local_health.take())
            .unwrap_or_else(|| ActorAttribute {
                name: "minecraft:health".into(),
                min: 0.0,
                max: DEFAULT_PLAYER_HEALTH,
                current: DEFAULT_PLAYER_HEALTH,
                default: None,
                modifiers: Default::default(),
            });
        if !health.min.is_finite() || !health.max.is_finite() || health.min > health.max {
            self.skip_local_health();
            return;
        }
        let previous = health.current.ceil();
        health.current = f32::from(value).clamp(health.min, health.max);
        let dropped = previous > f32::from(value);
        if let Some(actor) = self.actors.get_mut(&runtime_id) {
            self.pending_local_health = None;
            if actor.apply_attributes(&[health]) {
                self.skip_local_health();
                return;
            }
            actor.sync_status_from_health();
            if dropped {
                actor.status.damage.previous_health = previous;
                if self.local_player_spawned {
                    actor.status.hurt_time = super::HURT_DURATION_TICKS;
                    actor.status.damage.remaining_ticks = super::HURT_DURATION_TICKS;
                }
            }
        } else {
            self.pending_local_health = Some(health);
            if dropped {
                let pending = self.pending_local_damage.get_or_insert(PendingLocalDamage {
                    damage: ActorDamageState::default(),
                    hurt_time: 0,
                });
                pending.damage.previous_health = previous;
                if self.local_player_spawned {
                    pending.hurt_time = super::HURT_DURATION_TICKS;
                    pending.damage.remaining_ticks = super::HURT_DURATION_TICKS;
                }
            }
        }
    }

    /// Counts rejected local updates and reports them at a bounded logarithmic rate.
    fn skip_local_health(&mut self) {
        self.local_health_skips = self.local_health_skips.saturating_add(1);
        if self.local_health_skips.is_power_of_two() {
            tracing::warn!(
                skipped = self.local_health_skips,
                "skipping unusable local health update"
            );
        }
    }

    /// Retains a newer local health attribute while the synthetic actor is absent.
    pub(super) fn retain_unspawned_local_health(
        &mut self,
        runtime_id: u64,
        attributes: &[ActorAttribute],
    ) {
        if self.remote_state_excluded_runtime_id == Some(runtime_id)
            && !self.actors.contains_key(&runtime_id)
            && let Some(health) = attributes
                .iter()
                .rev()
                .find(|attribute| attribute.name.as_ref() == "minecraft:health")
        {
            self.pending_local_health = Some(health.clone());
        }
    }

    /// Transfers pending health once, after the synthetic local actor is created.
    pub(super) fn apply_pending_local_health(&mut self, runtime_id: u64) {
        if let Some(actor) = self.actors.get_mut(&runtime_id)
            && let Some(health) = self.pending_local_health.take()
        {
            if actor.apply_attributes(&[health]) {
                self.skip_local_health();
                return;
            }
            actor.sync_status_from_health();
            if let Some(pending) = self.pending_local_damage.take() {
                actor.status.damage = pending.damage;
                actor.status.hurt_time = pending.hurt_time;
            }
        }
    }
}
