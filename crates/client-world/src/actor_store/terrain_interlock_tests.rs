use super::terrain_interlock::{FALLING_VISIBILITY_FALLBACK, TerrainInterlock};
use super::*;

fn fixture() -> (ActorStore, std::time::Instant) {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut spawn) = super::tests::spawn(7, -2) else {
        unreachable!()
    };
    spawn.kind = ActorKind::Entity {
        identifier: "minecraft:falling_block".into(),
    };
    spawn.metadata = std::sync::Arc::from([protocol::ActorMetadata {
        key: 2,
        value: ActorMetadataValue::Int(55),
    }]);
    assert_eq!(
        store.apply(1, 1, ActorEvent::Spawn(spawn)),
        ActorApplyResult::Inserted
    );
    let TerrainInterlock::Pending { attached_at } =
        store.actors.get(&7).unwrap().status.terrain_interlock
    else {
        panic!("falling block must initially wait for its terrain")
    };
    (store, attached_at)
}

#[test]
fn falling_block_waits_for_terrain_before_its_first_render() {
    let (store, attached_at) = fixture();
    assert!(
        store.block_entities_at(1.0, attached_at).is_empty(),
        "source terrain still owns the initial visual"
    );
    assert_eq!(
        store.actors.get(&7).unwrap().unique_id,
        -2,
        "render gating retains the actor"
    );
}

#[test]
fn falling_block_pending_fallback_is_strict_and_does_not_release_hidden_actors() {
    let (mut store, attached_at) = fixture();
    let deadline = attached_at + FALLING_VISIBILITY_FALLBACK;
    assert!(store.block_entities_at(1.0, deadline).is_empty());
    assert_eq!(
        store
            .block_entities_at(1.0, deadline + std::time::Duration::from_nanos(1))
            .len(),
        1
    );
    assert!(store.apply_terrain_sync(protocol::ActorBlockSyncMessage {
        actor_unique_id: -2,
        message: 2,
    }));
    assert!(
        store
            .block_entities_at(1.0, deadline + FALLING_VISIBILITY_FALLBACK)
            .is_empty()
    );
    assert!(store.actors.contains_key(&7));
}

#[test]
fn falling_block_sync_uses_signed_identity_and_skips_unknown_messages() {
    let (mut store, attached_at) = fixture();
    for (actor_unique_id, message) in [(-1, 1), (-99, 1), (-2, 0), (-2, 99)] {
        assert!(!store.apply_terrain_sync(protocol::ActorBlockSyncMessage {
            actor_unique_id,
            message,
        }));
        assert!(store.block_entities_at(1.0, attached_at).is_empty());
    }
    assert!(store.apply_terrain_sync(protocol::ActorBlockSyncMessage {
        actor_unique_id: -2,
        message: 1,
    }));
    assert_eq!(store.block_entities_at(1.0, attached_at).len(), 1);
}

#[test]
fn falling_block_candidates_retain_hidden_geometry_for_the_current_render_handoff() {
    let (mut store, attached_at) = fixture();
    for message in [0, 1, 2] {
        if message != 0 {
            assert!(store.apply_terrain_sync(protocol::ActorBlockSyncMessage {
                actor_unique_id: -2,
                message,
            }));
        }
        let candidates = store.block_entity_candidates_at(0.5, attached_at);
        assert_eq!(
            candidates.len(),
            1,
            "the render frame needs pending geometry"
        );
        assert_eq!(candidates[0].unique_id, -2);
        assert_eq!(candidates[0].view.runtime_id, 7);
        assert_eq!(candidates[0].visible, message == 1);
        assert_eq!(
            store.block_entities_at(0.5, attached_at).len(),
            usize::from(message == 1)
        );
    }
}
