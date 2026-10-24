//! Vanilla entity-shadow casters: who casts one, its radius and where it hangs.
//! Class dispositions come from the compiled entity facts carrier.
use protocol::{ActorKind, ActorMetadataValue};
use protocol_ids::{ActorDataKey, ActorFlag, Attribute};

use super::{ActorSnapshot, ActorStore, DEFAULT_ACTOR_COLLISION_WIDTH};

/// One caster: interpolated feet, lowered by any authored offset, and its radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorShadowCaster {
    pub runtime_id: u64,
    pub feet: [f32; 3],
    pub radius: f32,
}

impl ActorSnapshot {
    /// The radius vanilla gives this actor's shadow; zero casts none.
    #[must_use]
    pub fn shadow_radius(&self, riding: bool) -> f32 {
        self.entity_fact.shadow.radius(
            self.collision_width(),
            self.flag(ActorFlag::Baby.wire_value()),
            self.variant(),
            riding,
        )
    }

    /// Whether this actor's own state hides its shadow.
    #[must_use]
    pub fn shadow_hidden(&self) -> bool {
        let projectile = self.entity_fact.projectile;
        let dead = self.status.dead
            || self
                .attributes
                .get(Attribute::Health.wire_name())
                .is_some_and(|health| health.current <= 0.0);
        projectile
            || dead
            || self.is_on_fire()
            || self.flag(ActorFlag::Invisible.wire_value())
            || self.status.breathing_submerged == Some(true)
    }

    /// Height the shadow hangs below the feet; ghasts carry authored relative offsets.
    fn shadow_drop(&self) -> f32 {
        let relative = self.entity_fact.shadow_drop;
        let height = match self.metadata.get(&ActorDataKey::Height.wire_value()) {
            Some(ActorMetadataValue::Float(height)) if height.is_finite() => *height,
            _ => 0.0,
        };
        relative * height * self.render_scale()
    }

    /// Uses scaled collision width; only players supply a default for omitted bounds.
    fn collision_width(&self) -> f32 {
        if !matches!(self.kind, ActorKind::Player { .. })
            && !matches!(
                self.metadata.get(&ActorDataKey::Width.wire_value()),
                Some(ActorMetadataValue::Float(width)) if width.is_finite()
            )
        {
            return 0.0;
        }
        self.scaled_collision_dimension(ActorDataKey::Width, DEFAULT_ACTOR_COLLISION_WIDTH)
            .unwrap_or(0.0)
    }

    fn variant(&self) -> i32 {
        match self.metadata.get(&ActorDataKey::Variant.wire_value()) {
            Some(ActorMetadataValue::Int(variant)) => (*variant).max(0),
            _ => 0,
        }
    }
}

impl ActorStore {
    /// The actor's caster at `alpha` of the frame, if it casts one; an actor riding a visible
    /// vehicle leaves the shadow to it.
    pub(crate) fn shadow_caster(
        &self,
        actor: &ActorSnapshot,
        alpha: f32,
    ) -> Option<ActorShadowCaster> {
        let vehicle = self
            .ridden_unique_id(actor.unique_id)
            .and_then(|vehicle| self.snapshot_by_unique(vehicle));
        if actor.shadow_hidden()
            || vehicle.is_some_and(|vehicle| !vehicle.flag(ActorFlag::Invisible.wire_value()))
        {
            return None;
        }
        let radius = actor.shadow_radius(vehicle.is_some());
        if !(radius > 0.0 && radius.is_finite()) {
            return None;
        }
        let mut feet = actor.interpolated_position(alpha.clamp(0.0, 1.0))?;
        feet[1] += actor.shadow_drop();
        Some(ActorShadowCaster {
            runtime_id: actor.runtime_id,
            feet,
            radius,
        })
    }

    pub(crate) fn shadow_casters(
        &self,
        alpha: f32,
    ) -> impl Iterator<Item = ActorShadowCaster> + '_ {
        self.actors
            .values()
            .filter_map(move |actor| self.shadow_caster(actor, alpha))
    }

    /// Records `(runtime_id, submerged)` breathing-point samples that hide shadows.
    pub(crate) fn set_breathing_liquids(&mut self, samples: &[(u64, bool)]) {
        for &(runtime_id, submerged) in samples {
            if let Some(actor) = self.actors.get_mut(&runtime_id) {
                actor.status.breathing_submerged = Some(submerged);
            }
        }
    }
}

#[cfg(test)]
mod tests;
