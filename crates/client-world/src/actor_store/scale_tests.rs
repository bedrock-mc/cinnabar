use std::sync::Arc;

use protocol::{
    ActorEvent, ActorKind, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue,
};

use super::{ActorStore, NAMETAG_METADATA_KEY, SCALE_METADATA_KEY, tests::spawn};

#[test]
fn zero_scale_nametag_host_retains_its_authoritative_scale_and_name() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut event) = spawn(8, 80) else {
        unreachable!();
    };
    event.kind = ActorKind::Entity {
        identifier: "minecraft:sheep".into(),
    };
    event.metadata = Arc::from([
        ActorMetadata {
            key: SCALE_METADATA_KEY,
            value: ActorMetadataValue::Float(0.0),
        },
        ActorMetadata {
            key: NAMETAG_METADATA_KEY,
            value: ActorMetadataValue::String("Game mode".into()),
        },
    ]);
    store.apply(1, 1, ActorEvent::Spawn(event));

    let actor = store.get(8).unwrap();
    assert!(
        !actor.is_invisible(),
        "zero scale is separate from invisibility"
    );
    assert_eq!(actor.render_scale(), 0.0);
    assert_eq!(store.actor_name_tag(80), Some(Arc::from("Game mode")));

    for (tick, scale, expected) in [
        (2, 0.5, 0.5),
        (3, 0.0, 0.0),
        (4, 2.0, 2.0),
        (5, -1.0, 1.0),
        (6, f32::INFINITY, 1.0),
        (7, f32::NAN, 1.0),
    ] {
        store.apply(
            1,
            tick,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 8,
                metadata: Arc::from([ActorMetadata {
                    key: SCALE_METADATA_KEY,
                    value: ActorMetadataValue::Float(scale),
                }]),
                properties: Arc::from([]),
                tick,
            }),
        );
        assert_eq!(store.get(8).unwrap().render_scale(), expected);
        assert_eq!(store.actor_name_tag(80), Some(Arc::from("Game mode")));
    }
}
