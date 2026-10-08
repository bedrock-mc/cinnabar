//! Fixed-tick particle trail left by a dragon fireball.

use protocol::SpawnParticleEffectEvent;

use super::{ActorKind, ActorStore};

const TRAIL_EFFECT: &str = "minecraft:dragon_breath_trail";
const TRAIL_HEIGHT: f32 = 0.5;

impl ActorStore {
    pub(super) fn advance_dragon_fireball_particles(&mut self) {
        let Ok(dimension) = u8::try_from(self.dimension) else {
            return;
        };
        for actor in self.actors.values() {
            if !matches!(&actor.kind, ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:dragon_fireball")
            {
                continue;
            }
            let mut position = actor.position;
            position[1] += TRAIL_HEIGHT;
            if !position.iter().all(|component| component.is_finite()) {
                continue;
            }
            super::dragon_particles::push_effect(
                &mut self.particle_effects,
                self.latest_sequence,
                self.dimension,
                SpawnParticleEffectEvent {
                    dimension,
                    actor_unique_id: None,
                    position,
                    effect: TRAIL_EFFECT.into(),
                    molang_variables: None,
                },
            );
        }
    }
}

#[cfg(test)]
#[path = "dragon_fireball_particles_tests.rs"]
mod tests;
