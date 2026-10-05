//! Healing beams retain a randomly refreshed nearest crystal on the dragon's fixed ticks.

use std::{hash::BuildHasher, sync::OnceLock};

use protocol::ActorKind;

use super::{ActorSnapshot, ActorStore, CrystalBeamView};

const HEALING_RADIUS_SQUARED: f32 = 1024.0;
const REFRESH_CHANCES: u64 = 10;

fn kind(actor: &ActorSnapshot, identifier: &str) -> bool {
    matches!(&actor.kind, ActorKind::Entity { identifier: actual } if actual.as_ref() == identifier)
}

fn alive(actor: &ActorSnapshot) -> bool {
    !actor.status.dead
        && actor
            .attributes
            .get("minecraft:health")
            .is_none_or(|health| health.current > 0.0)
}

impl ActorStore {
    pub(super) fn advance_dragon_beams(&mut self) {
        static RANDOM: OnceLock<std::collections::hash_map::RandomState> = OnceLock::new();
        let session = self.session_id;
        let dimension = self.dimension;
        self.advance_dragon_beams_with(|actor| {
            let hash = RANDOM.get_or_init(Default::default).hash_one((
                session,
                dimension,
                actor.runtime_id,
                actor.spawn_revision,
                actor.status.age_ticks,
            ));
            hash % REFRESH_CHANCES == 0
        });
    }

    fn advance_dragon_beams_with(&mut self, mut refresh: impl FnMut(&ActorSnapshot) -> bool) {
        let selections = self
            .actors
            .values()
            .filter(|actor| kind(actor, "minecraft:ender_dragon"))
            .map(|dragon| {
                let selected = if !alive(dragon) {
                    None
                } else if refresh(dragon) {
                    self.actors
                        .values()
                        .filter(|crystal| {
                            kind(crystal, "minecraft:ender_crystal") && alive(crystal)
                        })
                        .filter_map(|crystal| {
                            let squared: f32 = (0..3)
                                .map(|axis| {
                                    (crystal.position[axis] - dragon.position[axis]).powi(2)
                                })
                                .sum();
                            (squared.is_finite() && squared < HEALING_RADIUS_SQUARED).then_some((
                                squared,
                                crystal.runtime_id,
                                crystal.spawn_revision,
                            ))
                        })
                        .min_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)))
                        .map(|(_, runtime, revision)| (runtime, revision))
                } else {
                    dragon.status.healing_crystal.filter(|(runtime, revision)| {
                        self.actors.get(runtime).is_some_and(|crystal| {
                            crystal.spawn_revision == *revision
                                && alive(crystal)
                                && kind(crystal, "minecraft:ender_crystal")
                        })
                    })
                };
                (dragon.runtime_id, selected)
            })
            .collect::<Vec<_>>();
        for (runtime, selected) in selections {
            if let Some(dragon) = self.actors.get_mut(&runtime) {
                dragon.status.healing_crystal = selected;
            }
        }
    }

    pub(super) fn dragon_healing_beams(
        &self,
        alpha: f32,
    ) -> impl Iterator<Item = CrystalBeamView> + '_ {
        self.actors.values().filter_map(move |dragon| {
            if !kind(dragon, "minecraft:ender_dragon") || !alive(dragon) {
                return None;
            }
            let (runtime, revision) = dragon.status.healing_crystal?;
            let crystal = self.actors.get(&runtime)?;
            if crystal.spawn_revision != revision
                || !alive(crystal)
                || !kind(crystal, "minecraft:ender_crystal")
            {
                return None;
            }
            let owner_position = dragon.interpolated_position(alpha)?;
            let mut target = owner_position;
            target[1] += 2.0;
            let mut crystal = crystal.interpolated_position(alpha)?;
            crystal[1] += 1.0;
            Some(CrystalBeamView {
                runtime_id: dragon.runtime_id,
                owner_position,
                target,
                crystal,
                age_ticks: dragon.status.age_ticks as f32 + alpha,
            })
        })
    }
}

#[cfg(test)]
mod tests;
