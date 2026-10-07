use super::*;
use crate::actor_store::ACTOR_FLAG_IMMOBILE;

fn metadata(runtime_id: u64, dimension: i32, flags: u64) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
        runtime_id,
        dimension,
        metadata: Arc::from([ActorMetadata {
            key: 0,
            value: ActorMetadataValue::Flags(flags),
        }]),
        properties: Arc::from([]),
        tick: 91,
    }))
}

#[test]
fn immobile_control_is_local_and_dimension_scoped_before_the_actor_exists() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 5,
            local_player_runtime_id: 41,
            dimension: 2,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    assert!(authority.actor(41).is_none());
    for (sequence, runtime_id, dimension) in [(1, 72, 2), (2, 41, 0)] {
        authority
            .apply_ordered_event(
                metadata(runtime_id, dimension, 1 << ACTOR_FLAG_IMMOBILE),
                Some(sequence),
            )
            .unwrap();
    }
    assert!(authority.take_committed_controls().is_empty());
    authority
        .apply_ordered_event(metadata(41, 2, 1 << ACTOR_FLAG_IMMOBILE), Some(3))
        .unwrap();
    assert!(matches!(
        authority.take_committed_controls().as_slice(),
        [CommittedControlEvent::LocalMovementFlags {
            sequence: 3,
            tick: 91,
            flags: crate::MovementFlagUpdate {
                immobile: Some(true),
                ..
            },
        }],
    ));
    assert!(authority.actor(41).is_none());
    authority
        .apply_ordered_event(metadata(41, 2, 0), Some(4))
        .unwrap();
    assert!(matches!(
        authority.take_committed_controls().as_slice(),
        [CommittedControlEvent::LocalMovementFlags {
            sequence: 4,
            flags: crate::MovementFlagUpdate {
                immobile: Some(false),
                ..
            },
            ..
        }],
    ));
}
