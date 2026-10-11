//! Ordered candidate membership for actor effects, retained at ingress.

use std::collections::BTreeSet;

use protocol::ActorKind;

use super::{ActorSnapshot, entities};

#[derive(Debug, Default)]
pub(super) struct EffectMembers {
    pub blocks: BTreeSet<u64>,
    pub ropes: BTreeSet<u64>,
    pub crystals: BTreeSet<u64>,
    pub dragons: BTreeSet<u64>,
}

impl EffectMembers {
    /// Refreshes candidate membership after the actor's kind or metadata changes.
    pub fn update(&mut self, actor: &ActorSnapshot) {
        let identifier = match &actor.kind {
            ActorKind::Entity { identifier } => identifier.as_ref(),
            _ => "",
        };
        let runtime = actor.runtime_id;
        set(
            &mut self.blocks,
            runtime,
            matches!(identifier, "minecraft:falling_block" | "minecraft:tnt"),
        );
        set(
            &mut self.crystals,
            runtime,
            identifier == "minecraft:ender_crystal",
        );
        set(
            &mut self.dragons,
            runtime,
            identifier == "minecraft:ender_dragon",
        );
        set(
            &mut self.ropes,
            runtime,
            identifier == "minecraft:fishing_hook"
                || entities::metadata_i64(actor, entities::LEASH_HOLDER_METADATA_KEY)
                    .is_some_and(|holder| holder != entities::INVALID_LEASH_HOLDER_ID),
        );
    }

    /// Removes every membership for an actor lifetime that has ended.
    pub fn remove(&mut self, runtime: u64) {
        self.blocks.remove(&runtime);
        self.ropes.remove(&runtime);
        self.crystals.remove(&runtime);
        self.dragons.remove(&runtime);
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
