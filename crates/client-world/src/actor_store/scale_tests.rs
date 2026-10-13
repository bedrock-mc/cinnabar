use std::sync::Arc;

use protocol::{
    ActorEvent, ActorKind, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue,
};

use super::{
    ActorStore, NAMETAG_METADATA_KEY, SCALE_METADATA_KEY,
    tests::{entity_spawn, network_move, spawn},
};

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

#[test]
fn falling_block_network_offsets_follow_their_scaled_box() {
    let metadata = |height: f32, scale: f32| -> Arc<[ActorMetadata]> {
        Arc::from([
            ActorMetadata {
                key: super::BOUNDING_BOX_HEIGHT_METADATA_KEY,
                value: ActorMetadataValue::Float(height),
            },
            ActorMetadata {
                key: SCALE_METADATA_KEY,
                value: ActorMetadataValue::Float(scale),
            },
        ])
    };
    // The vanilla box keeps its half-block offset; text hosts stay where the server put them.
    for (height, scale, sent, feet) in [
        (0.98, 1.0, 64.5, 64.0),
        (0.98, 0.000_000_1, 64.0, 64.0),
        (0.0, 1.0, 64.0, 64.0),
    ] {
        let mut store = ActorStore::new(1, 0);
        let ActorEvent::Spawn(mut event) =
            entity_spawn(40, -40, "minecraft:falling_block", metadata(height, scale))
        else {
            unreachable!()
        };
        event.position[1] = sent;
        store.apply(1, 1, ActorEvent::Spawn(event));
        assert!(
            (store.get(40).unwrap().position[1] - feet).abs() < 1e-5,
            "height {height} at scale {scale}"
        );
        store.apply(1, 2, network_move(40, sent, true));
        assert!((store.get(40).unwrap().position[1] - feet).abs() < 1e-5);
    }
}

#[test]
fn falling_block_scale_change_keeps_its_network_position_and_targets() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut event) =
        entity_spawn(40, -40, "minecraft:falling_block", Arc::from([]))
    else {
        unreachable!()
    };
    event.position[1] = 64.5;
    store.apply(1, 1, ActorEvent::Spawn(event));
    store.apply(1, 2, network_move(40, 66.5, false));
    let before = store.get(40).unwrap().native_position()[1];
    store.apply(
        1,
        3,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 40,
            metadata: Arc::from([ActorMetadata {
                key: SCALE_METADATA_KEY,
                value: ActorMetadataValue::Float(2.0),
            }]),
            properties: Arc::from([]),
            tick: 3,
        }),
    );
    assert!((store.get(40).unwrap().native_position()[1] - before).abs() < 1e-5);
    store.advance_interpolation_ticks(64);
    assert!((store.get(40).unwrap().native_position()[1] - 66.5).abs() < 1e-5);
}
