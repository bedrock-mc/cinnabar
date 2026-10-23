use super::*;
use std::sync::Arc;

/// Explicit dimension ownership follows accepted writes even when their values are unchanged.
#[test]
fn explicit_dimensions_keep_their_arrival_revision() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut spawn) = super::tests::spawn(7, 70) else {
        unreachable!()
    };
    spawn.metadata = Arc::from([protocol::ActorMetadata {
        key: ActorDataKey::Height.wire_value(),
        value: ActorMetadataValue::Float(1.5),
    }]);
    store.apply(1, 10, ActorEvent::Spawn(spawn));
    assert_eq!(store.get(7).unwrap().explicit_dimensions_revision, Some(10));
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
            store.get(7).unwrap().explicit_dimensions_revision,
            Some(expected)
        );
    }
}
