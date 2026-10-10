use std::sync::Arc;

use protocol::{
    ActorEvent, ActorKind, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue,
    ActorRemoveEvent,
};

use super::super::{ActorStore, tests::spawn};

/// Spawns an entity through the ordinary admission path.
fn entity(runtime: u64, unique: i64, identifier: &str) -> ActorEvent {
    let ActorEvent::Spawn(mut event) = spawn(runtime, unique) else {
        unreachable!()
    };
    event.kind = ActorKind::Entity {
        identifier: identifier.into(),
    };
    ActorEvent::Spawn(event)
}

#[test]
fn membership_follows_replacement_removal_metadata_and_reset() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, entity(1, 10, "minecraft:tnt"));
    store.apply(1, 2, entity(2, 20, "minecraft:ender_crystal"));
    store.apply(1, 3, entity(3, 30, "minecraft:ender_dragon"));
    store.apply(1, 4, entity(4, 40, "minecraft:fishing_hook"));
    assert_eq!(
        store
            .effect_members
            .blocks
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [1]
    );
    assert_eq!(
        store
            .effect_members
            .crystals
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [2]
    );
    assert_eq!(
        store
            .effect_members
            .dragons
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [3]
    );
    assert_eq!(
        store
            .effect_members
            .ropes
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [4]
    );
    // Both runtime-ID and unique-ID replacement end the previous lifetime.
    store.apply(1, 5, entity(1, 50, "minecraft:bee"));
    store.apply(1, 6, entity(5, 20, "minecraft:bee"));
    assert!(store.effect_members.blocks.is_empty());
    assert!(store.effect_members.crystals.is_empty());
    for (sequence, value, expected) in [
        (7, ActorMetadataValue::Long(50), vec![4, 5]),
        (8, ActorMetadataValue::Int(-1), vec![4]),
        (9, ActorMetadataValue::Int(50), vec![4, 5]),
        (10, ActorMetadataValue::String("invalid".into()), vec![4]),
    ] {
        store.apply(
            1,
            sequence,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 5,
                metadata: Arc::from([ActorMetadata {
                    key: super::entities::LEASH_HOLDER_METADATA_KEY,
                    value,
                }]),
                properties: Arc::from([]),
                tick: 0,
            }),
        );
        assert_eq!(
            store
                .effect_members
                .ropes
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            expected
        );
    }
    store.apply(
        1,
        11,
        ActorEvent::Remove(ActorRemoveEvent {
            dimension: 0,
            unique_id: 30,
        }),
    );
    assert!(store.effect_members.dragons.is_empty());
    store.reset_dimension(1, 12, 1);
    assert!(store.effect_members.ropes.is_empty());
    store.begin_session(2, 0);
    assert!(store.effect_members.blocks.is_empty());
}

#[test]
fn unrelated_actors_do_not_enter_effect_views_and_block_order_is_stable() {
    let mut store = ActorStore::new(1, 0);
    for runtime in 1..=512 {
        store.apply(1, runtime, spawn(runtime, runtime as i64));
    }
    assert!(store.effect_members.ropes.is_empty());
    assert!(store.effect_members.crystals.is_empty());
    assert!(store.effect_members.dragons.is_empty());
    assert!(store.ropes(0.5).is_empty());
    assert!(store.crystal_beams(0.5).is_empty());
    for (offset, runtime) in [500, 4, 257].into_iter().enumerate() {
        store.apply(
            1,
            513 + offset as u64,
            entity(runtime, runtime as i64, "minecraft:tnt"),
        );
    }
    let mut expected = store
        .actors
        .values()
        .filter(|actor| {
            matches!(&actor.kind,
        ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:tnt")
        })
        .map(|actor| actor.runtime_id)
        .collect::<Vec<_>>();
    expected.sort_unstable();
    assert_eq!(
        store
            .block_entities(0.5)
            .iter()
            .map(|view| view.runtime_id)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(store.effect_members.blocks.len(), expected.len());
}
