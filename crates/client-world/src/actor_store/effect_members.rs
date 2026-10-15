//! Ordered candidate membership for actor effects, retained at ingress.

use protocol_ids::ActorDataKey;
use std::collections::{BTreeMap, BTreeSet};

use assets::facts::entity::NativeEntityClass;

use super::{ActorSnapshot, entities};

#[derive(Debug, Default)]
pub(super) struct EffectMembers {
    pub blocks: BTreeSet<u64>,
    pub ropes: BTreeSet<u64>,
    pub crystals: BTreeSet<u64>,
    pub dragons: BTreeSet<u64>,
    pub paintings: BTreeSet<u64>,
    pub collidable: super::colliders::ActorColliders,
    /// Streamed bed positions for actors currently sleeping in a known bed.
    pub sleepers: BTreeMap<u64, [i32; 3]>,
}

impl EffectMembers {
    /// Refreshes candidate membership after the actor's kind or metadata changes.
    pub fn update(&mut self, actor: &mut ActorSnapshot) {
        let class = actor.entity_fact.native_class;
        let runtime = actor.runtime_id;
        let bed = actor.is_sleeping().then(|| actor.bed_position()).flatten();
        let previous = match bed {
            Some(bed) => self.sleepers.insert(runtime, bed),
            None => self.sleepers.remove(&runtime),
        };
        if previous != bed {
            actor.status.sleep_rotation = None;
        }
        self.collidable.update(actor);
        set(
            &mut self.paintings,
            runtime,
            class == NativeEntityClass::Painting,
        );
        set(
            &mut self.blocks,
            runtime,
            matches!(
                class,
                NativeEntityClass::FallingBlock | NativeEntityClass::Tnt
            ),
        );
        set(
            &mut self.crystals,
            runtime,
            class == NativeEntityClass::EnderCrystal,
        );
        set(
            &mut self.dragons,
            runtime,
            class == NativeEntityClass::EnderDragon,
        );
        set(
            &mut self.ropes,
            runtime,
            class == NativeEntityClass::FishingHook
                || entities::metadata_i64(actor, ActorDataKey::LeashHolder.wire_value())
                    .is_some_and(|holder| holder != entities::INVALID_LEASH_HOLDER_ID),
        );
    }

    /// Removes every membership for an actor lifetime that has ended.
    pub fn remove(&mut self, runtime: u64) {
        self.blocks.remove(&runtime);
        self.ropes.remove(&runtime);
        self.crystals.remove(&runtime);
        self.dragons.remove(&runtime);
        self.paintings.remove(&runtime);
        self.collidable.remove(runtime);
        self.sleepers.remove(&runtime);
    }
}

/// Keeps a runtime ID in an ordered set exactly while its predicate holds.
fn set(members: &mut BTreeSet<u64>, runtime: u64, present: bool) {
    if present {
        members.insert(runtime);
    } else {
        members.remove(&runtime);
    }
}

#[cfg(test)]
mod tests;
