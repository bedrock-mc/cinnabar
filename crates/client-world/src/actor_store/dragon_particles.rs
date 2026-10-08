//! Fixed-tick dragon death effects, independent of animation evaluation cadence.

use protocol::{ParticleEvent, SpawnParticleEffectEvent};

use super::{ActorKind, ActorStore};

pub(super) const DRAGON_DEATH_TICKS: u16 = 200;
const FINAL_EXPLOSION_START: u16 = 180;
const DYING_EXPLOSION: &str = "minecraft:dragon_dying_explosion";
const DEATH_EXPLOSION: &str = "minecraft:dragon_death_explosion_emitter";
const DEATH_LIFT_PER_TICK: f32 = 0.1;

impl ActorStore {
    pub(super) fn advance_dragon_particles(&mut self) {
        self.advance_cloud_particles();
        self.advance_dragon_fireball_particles();
        let dimension = u8::try_from(self.dimension).ok();
        for actor in self.actors.values_mut() {
            if !matches!(&actor.kind, ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:ender_dragon")
                || !actor.status.dead
                || actor.status.dragon_death_time >= DRAGON_DEATH_TICKS
            {
                continue;
            }
            actor.status.dragon_death_time += 1;
            let mut seed = actor.runtime_id ^ u64::from(actor.status.age_ticks).rotate_left(32);
            if actor.status.dragon_death_time >= FINAL_EXPLOSION_START
                && let Some(dimension) = dimension
            {
                let offset = death_offset(&mut seed);
                let position = std::array::from_fn(|axis| actor.position[axis] + offset[axis]);
                push_effect(
                    &mut self.particle_effects,
                    self.latest_sequence,
                    self.dimension,
                    SpawnParticleEffectEvent {
                        dimension,
                        actor_unique_id: None,
                        position,
                        effect: DEATH_EXPLOSION.into(),
                        molang_variables: None,
                    },
                );
            }
            actor.status.native_velocity = [0.0; 3];
            actor.position[1] += DEATH_LIFT_PER_TICK;
            let Some(dimension) = dimension else {
                continue;
            };
            let offset = death_offset(&mut seed);
            let position = std::array::from_fn(|axis| actor.position[axis] + offset[axis]);
            push_effect(
                &mut self.particle_effects,
                self.latest_sequence,
                self.dimension,
                SpawnParticleEffectEvent {
                    dimension,
                    actor_unique_id: None,
                    position,
                    effect: DYING_EXPLOSION.into(),
                    molang_variables: None,
                },
            );
        }
    }

    pub(crate) fn take_particle_effects(&mut self) -> Vec<crate::CommittedParticleEvent> {
        self.particle_effects.drain(..).collect()
    }
}

pub(super) fn push_effect(
    queue: &mut std::collections::VecDeque<crate::CommittedParticleEvent>,
    sequence: u64,
    dimension: i32,
    effect: SpawnParticleEffectEvent,
) {
    if queue.len() == crate::COMMITTED_PARTICLE_CAPACITY {
        queue.pop_front();
    }
    queue.push_back(crate::CommittedParticleEvent {
        sequence,
        dimension,
        event: ParticleEvent::Spawn(effect),
    });
}

fn death_offset(seed: &mut u64) -> [f32; 3] {
    let mut next = || {
        *seed = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = *seed;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^= value >> 31;
        (value >> 40) as f32 / (1u32 << 24) as f32 - 0.5
    };
    [next() * 8.0, next() * 4.0 + 2.0, next() * 8.0]
}

#[cfg(test)]
#[path = "dragon_particles_tests.rs"]
mod tests;
