//! Local health packets mutate actor authority before its UI projection is delivered.

use super::{ActorAttribute, ActorStore};

/// Initial health range of a player without a streamed health attribute.
pub const DEFAULT_PLAYER_HEALTH: f32 = 20.0;

impl ActorStore {
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
        health.current = f32::from(value).clamp(health.min, health.max);
        if let Some(actor) = self.actors.get_mut(&runtime_id) {
            self.pending_local_health = None;
            if actor.apply_attributes(&[health]) {
                self.skip_local_health();
                return;
            }
            actor.sync_status_from_health();
        } else {
            self.pending_local_health = Some(health);
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
        }
    }
}
