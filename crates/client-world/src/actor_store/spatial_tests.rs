use super::*;
use protocol::{ActorEvent, ActorMetadata, ActorMetadataUpdateEvent};
use protocol_ids::ActorDataKey;

#[test]
fn interaction_and_movement_boxes_share_definition_defaults() {
    for record in &EntityFacts::pinned().records {
        let Some([width, height]) = record.fact.collision else {
            continue;
        };
        let ActorEvent::Spawn(mut spawn) = super::super::tests::spawn(1, 1) else {
            unreachable!();
        };
        spawn.kind = ActorKind::Entity {
            identifier: record.identifier.as_ref().into(),
        };
        spawn.position = [0.0; 3];
        spawn.metadata = [].into();
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, ActorEvent::Spawn(spawn));
        let actor = store.get(1).unwrap();
        let [x, y, z] = actor.position;
        let half = width * 0.5;
        let expected = ([x - half, y, z - half], [x + half, y + height, z + half]);
        assert_eq!(
            actor.bounding_box(),
            Some(expected),
            "{}",
            record.identifier
        );
        assert_eq!(actor.hit_boxes().collect::<Vec<_>>(), [expected]);
        assert_eq!(actor.movement_collision_bounds(), Some(expected));
    }
}

#[test]
fn zero_dimensions_and_scale_keep_the_minimum_collision_box() {
    for player in [false, true] {
        for (width, height, scale) in [
            (0.0, 0.0, 1.0),
            (0.6, 1.8, 0.0),
            (0.6, 1.8, -1.0),
            (-1.0, -2.0, 1.0),
        ] {
            let ActorEvent::Spawn(mut spawn) = super::super::tests::spawn(1, 1) else {
                unreachable!();
            };
            spawn.position = [0.0; 3];
            if player {
                spawn.kind = ActorKind::Player {
                    uuid: [1; 16],
                    username: "Remote".into(),
                };
            }
            let mut store = ActorStore::new(1, 0);
            store.apply(1, 1, ActorEvent::Spawn(spawn));
            store.apply(
                1,
                2,
                ActorEvent::Metadata(ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 1,
                    metadata: [
                        ActorMetadata {
                            key: ActorDataKey::Width.wire_value(),
                            value: ActorMetadataValue::Float(width),
                        },
                        ActorMetadata {
                            key: ActorDataKey::Height.wire_value(),
                            value: ActorMetadataValue::Float(height),
                        },
                        ActorMetadata {
                            key: ActorDataKey::Scale.wire_value(),
                            value: ActorMetadataValue::Float(scale),
                        },
                    ]
                    .into(),
                    properties: [].into(),
                    tick: 0,
                }),
            );
            let actor = store.get(1).unwrap();
            let half = MIN_COLLISION_DIMENSION * 0.5;
            let expected = ([-half, 0.0, -half], [half, MIN_COLLISION_DIMENSION, half]);
            assert_eq!(actor.bounding_box(), Some(expected));
            assert_eq!(actor.hit_boxes().collect::<Vec<_>>(), [expected]);
        }
    }
}
