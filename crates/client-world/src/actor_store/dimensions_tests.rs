use super::*;
use std::sync::Arc;

/// Synthetic and received lifetimes keep their arrival order independently of frame count.
#[test]
fn synthetic_and_network_dimensions_share_lifetime_order() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(7);
    let feed = super::local_tests::local_feed(0.0, 0.0);
    store.sync_local_player(7, 70, &feed);
    let first = store.get(7).unwrap().explicit_dimensions;
    for _ in 0..10 {
        store.sync_local_player(7, 70, &feed);
    }
    assert_eq!(store.get(7).unwrap().explicit_dimensions, first);
    assert_eq!(
        store.apply(1, 1, super::tests::spawn(7, 70)),
        ActorApplyResult::Replaced
    );
    let network = store.get(7).unwrap().explicit_dimensions;
    assert!(network.spawn_revision > first.spawn_revision);
    store.apply(
        1,
        2,
        ActorEvent::Remove(protocol::ActorRemoveEvent {
            dimension: 0,
            unique_id: 70,
        }),
    );
    store.sync_local_player(7, 70, &feed);
    let synthetic = store.get(7).unwrap().explicit_dimensions;
    assert!(synthetic.spawn_revision > network.spawn_revision);
    assert_eq!(
        store.apply(1, 3, super::tests::spawn(7, 70)),
        ActorApplyResult::Replaced
    );
    let current = store.get(7).unwrap().explicit_dimensions;
    assert!(current.spawn_revision > synthetic.spawn_revision);
    assert_eq!(
        store.apply(2, 99, super::tests::spawn(7, 70)),
        ActorApplyResult::StaleSession
    );
    assert_eq!(
        store.apply(1, 3, super::tests::spawn(7, 70)),
        ActorApplyResult::StaleSequence
    );
    assert_eq!(store.get(7).unwrap().explicit_dimensions, current);
    assert_eq!(store.reset_dimension(1, 4, 1), ActorApplyResult::Reset);
    store.sync_local_player(7, 70, &feed);
    assert!(store.get(7).unwrap().explicit_dimensions.spawn_revision > current.spawn_revision);
    let previous_session = store.lifetime(7).unwrap();
    store.begin_session(2, 0);
    store.sync_local_player(7, 70, &feed);
    assert_ne!(
        store.lifetime(7).unwrap().session_id,
        previous_session.session_id
    );
}

/// Explicit dimension ownership follows accepted writes even when their values are unchanged.
#[test]
fn explicit_dimensions_keep_independent_arrivals() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut spawn) = super::tests::spawn(7, 70) else {
        unreachable!()
    };
    spawn.metadata = Arc::from([protocol::ActorMetadata {
        key: ActorDataKey::Height.wire_value(),
        value: ActorMetadataValue::Float(1.5),
    }]);
    store.apply(1, 10, ActorEvent::Spawn(spawn));
    assert_eq!(
        store.get(7).unwrap().explicit_dimensions.height,
        Some(DimensionWrite {
            sequence: 10,
            value: 1.5
        })
    );
    for (sequence, value, expected) in [(11, 1.5, 11), (12, f32::NAN, 11), (13, 0.6, 13)] {
        store.apply(
            1,
            sequence,
            ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 7,
                metadata: Arc::from([protocol::ActorMetadata {
                    key: ActorDataKey::Height.wire_value(),
                    value: ActorMetadataValue::Float(value),
                }]),
                properties: Arc::from([]),
                tick: 0,
            }),
        );
        assert_eq!(
            store
                .get(7)
                .unwrap()
                .explicit_dimensions
                .height
                .unwrap()
                .sequence,
            expected
        );
    }

    store.apply(
        1,
        14,
        ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 7,
            metadata: Arc::from([protocol::ActorMetadata {
                key: ActorDataKey::Width.wire_value(),
                value: ActorMetadataValue::Float(0.6),
            }]),
            properties: Arc::from([]),
            tick: 0,
        }),
    );
    let dimensions = store.get(7).unwrap().explicit_dimensions;
    assert_eq!(
        dimensions.width,
        Some(DimensionWrite {
            sequence: 14,
            value: 0.6
        })
    );
    assert_eq!(
        dimensions.height,
        Some(DimensionWrite {
            sequence: 13,
            value: 0.6
        })
    );
}
