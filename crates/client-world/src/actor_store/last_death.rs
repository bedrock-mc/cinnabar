//! The player's last death location, which recovery compasses point at.

use protocol::ActorMetadataValue;
use protocol_ids::ActorDataKey;

use super::{ActorSnapshot, ActorStore};

/// Whether actor data `key` belongs to the player's last death.
fn is_last_death_key(key: u32) -> bool {
    matches!(
        ActorDataKey::from_wire(key),
        Some(
            ActorDataKey::PlayerLastDeathPosition
                | ActorDataKey::PlayerLastDeathDimension
                | ActorDataKey::PlayerHasDied
        )
    )
}

impl ActorStore {
    /// Keeps the local player's last-death data while its actor is recreated, as on a
    /// dimension change: servers need not resend it, and recovery compasses read it.
    pub(super) fn retain_local_death(&mut self) {
        let Some(actor) = self
            .remote_state_excluded_runtime_id
            .and_then(|runtime_id| self.actors.get(&runtime_id))
        else {
            return;
        };
        let held: Vec<protocol::ActorMetadata> = actor
            .metadata
            .iter()
            .filter(|(key, _)| is_last_death_key(**key))
            .map(|(key, value)| protocol::ActorMetadata {
                key: *key,
                value: value.clone(),
            })
            .collect();
        if !held.is_empty() {
            self.pending_local_death = held;
        }
    }

    /// Holds last-death data the server sends while the local actor is absent.
    pub(super) fn merge_pending_local_death(&mut self, metadata: &[protocol::ActorMetadata]) {
        for entry in metadata.iter().filter(|entry| is_last_death_key(entry.key)) {
            self.pending_local_death
                .retain(|held| held.key != entry.key);
            self.pending_local_death.push(entry.clone());
        }
    }

    /// Restores held last-death data once the local actor exists again, without replacing
    /// newer values it already received.
    pub(super) fn apply_pending_local_death(&mut self, runtime_id: u64) {
        if self.pending_local_death.is_empty() {
            return;
        }
        let Some(actor) = self.actors.get_mut(&runtime_id) else {
            return;
        };
        for entry in std::mem::take(&mut self.pending_local_death) {
            actor.metadata.entry(entry.key).or_insert(entry.value);
        }
    }
}

impl ActorSnapshot {
    /// Where and in which dimension the player last died, as the server reports it. Servers
    /// send the position as a block position or a vector; `None` until the player has died.
    #[must_use]
    pub fn last_death(&self) -> Option<([f64; 3], i32)> {
        if let Some(ActorMetadataValue::Byte(0)) =
            self.metadata.get(&ActorDataKey::PlayerHasDied.wire_value())
        {
            return None;
        }
        let position = match self
            .metadata
            .get(&ActorDataKey::PlayerLastDeathPosition.wire_value())?
        {
            ActorMetadataValue::BlockPosition(block) => block.map(|axis| f64::from(axis) + 0.5),
            ActorMetadataValue::Vector(vector) => vector.map(f64::from),
            _ => return None,
        };
        let dimension = match self
            .metadata
            .get(&ActorDataKey::PlayerLastDeathDimension.wire_value())?
        {
            ActorMetadataValue::Int(dimension) => *dimension,
            ActorMetadataValue::Byte(dimension) => i32::from(*dimension),
            _ => return None,
        };
        position
            .iter()
            .all(|axis| axis.is_finite())
            .then_some((position, dimension))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::{ActorEvent, ActorMetadata, ActorMetadataUpdateEvent};

    use super::*;
    use crate::actor_store::ActorStore;

    /// A store whose actor 42 has received `metadata`.
    fn store_with(metadata: Vec<ActorMetadata>) -> ActorStore {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, crate::actor_store::tests::spawn(42, -7));
        store.apply(
            1,
            2,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 42,
                metadata: metadata.into(),
                properties: Arc::from([]),
                tick: 2,
            }),
        );
        store
    }

    /// One actor data entry.
    fn entry(key: u32, value: ActorMetadataValue) -> ActorMetadata {
        ActorMetadata { key, value }
    }

    #[test]
    fn last_death_reads_block_positions_and_vectors() {
        let block = store_with(vec![
            entry(127, ActorMetadataValue::BlockPosition([10, 64, -3])),
            entry(128, ActorMetadataValue::Int(1)),
            entry(129, ActorMetadataValue::Byte(1)),
        ]);
        assert_eq!(
            block.get(42).unwrap().last_death(),
            Some(([10.5, 64.5, -2.5], 1))
        );
        let vector = store_with(vec![
            entry(127, ActorMetadataValue::Vector([1.25, 70.0, 8.5])),
            entry(128, ActorMetadataValue::Int(0)),
        ]);
        assert_eq!(
            vector.get(42).unwrap().last_death(),
            Some(([1.25, 70.0, 8.5], 0))
        );
    }

    #[test]
    fn players_who_never_died_have_no_last_death() {
        let alive = store_with(vec![
            entry(127, ActorMetadataValue::BlockPosition([0, 0, 0])),
            entry(128, ActorMetadataValue::Int(0)),
            entry(129, ActorMetadataValue::Byte(0)),
        ]);
        assert_eq!(alive.get(42).unwrap().last_death(), None);
        assert_eq!(store_with(Vec::new()).get(42).unwrap().last_death(), None);
    }
}

#[cfg(test)]
mod local_tests {
    use std::sync::Arc;

    use protocol::{ActorEvent, ActorMetadata, ActorMetadataUpdateEvent};

    use super::*;
    use crate::actor_store::ActorStore;

    /// A death at block (10, 64, -3) in the Overworld.
    fn death_metadata() -> Arc<[ActorMetadata]> {
        Arc::from([
            ActorMetadata {
                key: 127,
                value: ActorMetadataValue::BlockPosition([10, 64, -3]),
            },
            ActorMetadata {
                key: 128,
                value: ActorMetadataValue::Int(0),
            },
            ActorMetadata {
                key: 129,
                value: ActorMetadataValue::Byte(1),
            },
        ])
    }

    /// Actor data `values` for actor 42 in `dimension`.
    fn metadata(sequence: u64, values: Arc<[ActorMetadata]>, dimension: i32) -> ActorEvent {
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension,
            runtime_id: 42,
            metadata: values,
            properties: Arc::from([]),
            tick: sequence,
        })
    }

    /// Actor 42 spawning in `dimension`.
    fn spawn_in(dimension: i32) -> ActorEvent {
        let ActorEvent::Spawn(mut spawn) = crate::actor_store::tests::spawn(42, -7) else {
            unreachable!()
        };
        spawn.dimension = dimension;
        ActorEvent::Spawn(spawn)
    }

    /// A store whose local player is actor 42.
    fn local_store() -> ActorStore {
        let mut store = ActorStore::new(1, 0);
        store.exclude_remote_state_for(42);
        store.apply(1, 1, crate::actor_store::tests::spawn(42, -7));
        store
    }

    #[test]
    fn the_local_last_death_survives_a_dimension_change() {
        let mut store = local_store();
        store.apply(1, 2, metadata(2, death_metadata(), 0));
        assert!(store.get(42).unwrap().last_death().is_some());
        store.reset_dimension(1, 3, 1);
        store.apply(1, 4, spawn_in(1));
        assert_eq!(
            store.get(42).and_then(ActorSnapshot::last_death),
            Some(([10.5, 64.5, -2.5], 0)),
            "the server does not resend it after the switch"
        );
    }

    #[test]
    fn last_death_sent_while_the_local_actor_is_absent_is_kept() {
        let mut store = local_store();
        store.reset_dimension(1, 2, 1);
        store.apply(1, 3, metadata(3, death_metadata(), 1));
        store.apply(1, 4, spawn_in(1));
        assert!(store.get(42).and_then(ActorSnapshot::last_death).is_some());
    }
}
