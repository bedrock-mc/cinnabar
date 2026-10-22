//! Resolves native constructors once per session entry or actor admission.
use super::*;

impl ActorStore {
    /// Copies the native base's compiled facts; unknown bases keep unresolved class behavior.
    pub(super) fn resolve_entity_fact(&self, kind: &ActorKind) -> EntityFact {
        match kind {
            ActorKind::Player { .. } => EntityFacts::pinned().player(),
            ActorKind::Entity { identifier } => self
                .custom_actor_facts
                .get(identifier)
                .copied()
                .unwrap_or_else(|| EntityFacts::pinned().get(identifier)),
        }
    }

    /// Admits each first definition and refreshes actors that preceded its registry entry.
    pub(super) fn apply_entity_bases(&mut self, registry: &protocol::ActorIdentifierRegistry) {
        for entry in registry.entries.iter() {
            if EntityFacts::pinned().get(&entry.identifier).class_key != 0
                || self.custom_actor_facts.contains_key(&entry.identifier)
            {
                continue;
            }
            if self.custom_actor_facts.len() >= protocol::MAX_ACTOR_IDENTIFIERS {
                self.actor_identifier_skips += 1;
                continue;
            }
            let base = if entry.base_identifier.is_empty() {
                &entry.identifier
            } else {
                &entry.base_identifier
            };
            let mut fact = EntityFacts::pinned().get(base);
            // A declared base does not verify a custom actor's interaction resolver.
            fact.actor_use_resolver = Default::default();
            if !entry.base_identifier.is_empty() && fact.class_key == 0 {
                self.actor_identifier_skips += 1;
            }
            if entry.base_identifier.is_empty() && fact.class_key == 0 {
                fact.interactions = assets::interactions::EntityInteractionTraits::GENERIC_MOB;
            }
            self.custom_actor_facts
                .insert(entry.identifier.clone(), fact);
        }
        for actor in self.actors.values_mut() {
            let fact = match &actor.kind {
                ActorKind::Player { .. } => EntityFacts::pinned().player(),
                ActorKind::Entity { identifier } => self
                    .custom_actor_facts
                    .get(identifier)
                    .copied()
                    .unwrap_or_else(|| EntityFacts::pinned().get(identifier)),
            };
            let previous_fact = actor.entity_fact;
            let previous = actor.network_position_offset();
            let previous_spawn = actor.spawn_network_offset();
            actor.entity_fact = fact;
            let offset = actor.network_position_offset();
            actor.shift_poses_y(
                previous - offset,
                previous_spawn - actor.spawn_network_offset(),
            );
            self.effect_members.update(actor);
            if actor.entity_fact != previous_fact {
                actor
                    .status
                    .terrain_interlock
                    .admit_class(fact.native_class);
                self.animation
                    .insert(self.session_id, self.dimension, actor);
            }
        }
    }
}

#[cfg(test)]
mod tests;
