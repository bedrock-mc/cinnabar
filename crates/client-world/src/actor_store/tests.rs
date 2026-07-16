use std::sync::Arc;

use protocol::{
    ActorAttribute, ActorAttributesUpdateEvent, ActorEvent, ActorKind, ActorMetadata,
    ActorMetadataUpdateEvent, ActorMetadataValue, ActorMoveEvent, ActorProperty, ActorRemoveEvent,
    ActorSpawnEvent, PlayerListEntry, PlayerListUpdateEvent, PlayerSkin, PlayerSkinUnavailable,
    StandardSkin,
};

use super::{ActorApplyResult, ActorStore};

fn spawn(runtime_id: u64, unique_id: i64) -> ActorEvent {
    ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id,
        runtime_id,
        kind: ActorKind::Entity {
            identifier: "minecraft:bee".into(),
        },
        position: [1.0, 2.0, 3.0],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
    })
}

#[test]
fn actor_lifecycle_applies_fifo_patches_and_removes_by_unique_id() {
    let mut store = ActorStore::new(11, 0);
    assert_eq!(
        store.apply(11, 1, spawn(42, -7)),
        ActorApplyResult::Inserted
    );
    assert_eq!(
        store.apply(
            11,
            2,
            ActorEvent::Move(ActorMoveEvent {
                dimension: 0,
                runtime_id: 42,
                position: [Some(9.0), None, Some(8.0)],
                pitch: Some(10.0),
                yaw: None,
                head_yaw: None,
                on_ground: Some(true),
                teleported: false,
            }),
        ),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply(
            11,
            3,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 42,
                metadata: Arc::from([ActorMetadata {
                    key: 4,
                    value: ActorMetadataValue::String("Beeatrice".into()),
                }]),
                properties: Arc::from([ActorProperty::Int { index: 2, value: 5 }]),
                tick: 10,
            }),
        ),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply(
            11,
            4,
            ActorEvent::Attributes(ActorAttributesUpdateEvent {
                dimension: 0,
                runtime_id: 42,
                attributes: Arc::from([ActorAttribute {
                    name: "minecraft:health".into(),
                    min: 0.0,
                    max: 20.0,
                    current: 17.0,
                    default: Some(20.0),
                    modifiers: Arc::from([]),
                }]),
                tick: 11,
            }),
        ),
        ActorApplyResult::Updated
    );

    let actor = store.get(42).expect("stored actor");
    assert_eq!(actor.movement_revision, 2);
    assert_eq!(actor.position, [9.0, 2.0, 8.0]);
    assert_eq!(actor.pitch, 10.0);
    assert_eq!(actor.on_ground, Some(true));
    assert_eq!(
        actor.metadata[&4],
        ActorMetadataValue::String("Beeatrice".into())
    );
    assert_eq!(actor.attributes["minecraft:health"].current, 17.0);

    assert_eq!(
        store.apply(
            11,
            5,
            ActorEvent::Remove(ActorRemoveEvent {
                dimension: 0,
                unique_id: -7,
            }),
        ),
        ActorApplyResult::Removed
    );
    assert!(store.get(42).is_none());
}

#[test]
fn consecutive_teleport_packets_retain_distinct_movement_revisions() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(42, -7));
    let teleport = |x| {
        ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 42,
            position: [Some(x), None, None],
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: None,
            teleported: true,
        })
    };

    assert_eq!(
        store.apply(1, 2, teleport(100.0)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.get(42).unwrap().movement_revision, 2);
    assert_eq!(
        store.apply(1, 3, teleport(200.0)),
        ActorApplyResult::Updated
    );
    let actor = store.get(42).unwrap();
    assert_eq!(actor.movement_revision, 3);
    assert_eq!(actor.position[0], 200.0);
    assert!(actor.teleported);
}

#[test]
fn duplicate_runtime_or_unique_ids_replace_atomically() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(10, 20));
    assert_eq!(store.get(10).unwrap().spawn_revision, 1);
    assert_eq!(store.apply(1, 2, spawn(10, 21)), ActorApplyResult::Replaced);
    assert_eq!(store.len(), 1);
    assert_eq!(store.get(10).unwrap().spawn_revision, 2);
    assert_eq!(store.get(10).unwrap().unique_id, 21);

    assert_eq!(store.apply(1, 3, spawn(11, 21)), ActorApplyResult::Replaced);
    assert_eq!(store.len(), 1);
    assert!(store.get(10).is_none());
    assert_eq!(store.get(11).unwrap().unique_id, 21);
}

#[test]
fn remove_and_readd_of_the_same_actor_gets_a_new_spawn_revision() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(10, 20));
    store.apply(
        1,
        2,
        ActorEvent::Remove(ActorRemoveEvent {
            dimension: 0,
            unique_id: 20,
        }),
    );
    store.apply(1, 3, spawn(10, 20));

    let actor = store.get(10).unwrap();
    assert_eq!(actor.unique_id, 20);
    assert_eq!(actor.spawn_revision, 3);
}

#[test]
fn capacity_is_bounded_but_existing_actor_replacement_is_allowed() {
    let mut store = ActorStore::with_capacity(1, 0, 2, 2);
    assert_eq!(store.apply(1, 1, spawn(1, 1)), ActorApplyResult::Inserted);
    assert_eq!(store.apply(1, 2, spawn(2, 2)), ActorApplyResult::Inserted);
    assert_eq!(
        store.apply(1, 3, spawn(3, 3)),
        ActorApplyResult::CapacityRejected
    );
    assert_eq!(store.apply(1, 4, spawn(2, 22)), ActorApplyResult::Replaced);
    assert_eq!(store.len(), 2);
}

#[test]
fn cumulative_actor_patches_cannot_exceed_retained_collection_bounds() {
    use protocol::{MAX_ACTOR_ATTRIBUTES, MAX_ACTOR_METADATA_ENTRIES, MAX_ACTOR_PROPERTIES};

    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(1, 1));

    let metadata = (0..MAX_ACTOR_METADATA_ENTRIES)
        .map(|key| ActorMetadata {
            key: i32::try_from(key).unwrap(),
            value: ActorMetadataValue::Int(i32::try_from(key).unwrap()),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        store.apply(
            1,
            2,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                metadata: metadata.into(),
                properties: Arc::from([]),
                tick: 1,
            }),
        ),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply(
            1,
            3,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                metadata: Arc::from([ActorMetadata {
                    key: i32::try_from(MAX_ACTOR_METADATA_ENTRIES).unwrap(),
                    value: ActorMetadataValue::Int(1),
                }]),
                properties: Arc::from([]),
                tick: 2,
            }),
        ),
        ActorApplyResult::CapacityRejected
    );
    assert_eq!(
        store.get(1).unwrap().metadata.len(),
        MAX_ACTOR_METADATA_ENTRIES
    );

    let attributes = (0..MAX_ACTOR_ATTRIBUTES)
        .map(|index| ActorAttribute {
            name: format!("attribute.{index}").into(),
            min: 0.0,
            max: 1.0,
            current: 1.0,
            default: None,
            modifiers: Arc::from([]),
        })
        .collect::<Vec<_>>();
    store.apply(
        1,
        4,
        ActorEvent::Attributes(ActorAttributesUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            attributes: attributes.into(),
            tick: 3,
        }),
    );
    assert_eq!(
        store.apply(
            1,
            5,
            ActorEvent::Attributes(ActorAttributesUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                attributes: Arc::from([ActorAttribute {
                    name: "attribute.overflow".into(),
                    min: 0.0,
                    max: 1.0,
                    current: 1.0,
                    default: None,
                    modifiers: Arc::from([]),
                }]),
                tick: 4,
            }),
        ),
        ActorApplyResult::CapacityRejected
    );
    assert_eq!(store.get(1).unwrap().attributes.len(), MAX_ACTOR_ATTRIBUTES);

    let properties = (0..MAX_ACTOR_PROPERTIES)
        .map(|index| ActorProperty::Int {
            index: i32::try_from(index).unwrap(),
            value: 1,
        })
        .collect::<Vec<_>>();
    store.apply(
        1,
        6,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            metadata: Arc::from([]),
            properties: properties.into(),
            tick: 5,
        }),
    );
    assert_eq!(
        store.apply(
            1,
            7,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                metadata: Arc::from([]),
                properties: Arc::from([ActorProperty::Float {
                    index: i32::try_from(MAX_ACTOR_PROPERTIES).unwrap(),
                    value: 1.0,
                }]),
                tick: 6,
            }),
        ),
        ActorApplyResult::CapacityRejected
    );
    let actor = store.get(1).unwrap();
    assert_eq!(
        actor.int_properties.len() + actor.float_properties.len(),
        MAX_ACTOR_PROPERTIES
    );
}

#[test]
fn stale_session_sequence_and_dimension_are_rejected() {
    let mut store = ActorStore::new(5, 0);
    assert_eq!(
        store.apply(4, 1, spawn(1, 1)),
        ActorApplyResult::StaleSession
    );
    assert_eq!(store.apply(5, 2, spawn(1, 1)), ActorApplyResult::Inserted);
    assert_eq!(
        store.apply(5, 2, spawn(2, 2)),
        ActorApplyResult::StaleSequence
    );
    let mut wrong_dimension = spawn(3, 3);
    let ActorEvent::Spawn(spawn) = &mut wrong_dimension else {
        unreachable!()
    };
    spawn.dimension = 1;
    assert_eq!(
        store.apply(5, 3, wrong_dimension),
        ActorApplyResult::StaleDimension
    );
    assert_eq!(store.len(), 1);
}

#[test]
fn dimension_reset_clears_actors_and_session_reset_also_clears_roster() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn(1, 1));
    store.apply(
        1,
        2,
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Add {
                uuid: [7; 16],
                unique_id: 1,
                username: "Alex".into(),
                verified: true,
                skin: protocol::PlayerSkin::Unavailable(
                    protocol::PlayerSkinUnavailable::InvalidDimensions,
                ),
            }]),
        }),
    );
    assert_eq!(store.player_count(), 1);

    assert_eq!(store.reset_dimension(1, 3, 2), ActorApplyResult::Reset);
    assert!(store.is_empty());
    assert_eq!(store.player_count(), 1);

    store.begin_session(2, 0);
    assert!(store.is_empty());
    assert_eq!(store.player_count(), 0);
    assert_eq!(
        store.apply(1, 4, spawn(2, 2)),
        ActorApplyResult::StaleSession
    );
}

#[test]
fn render_players_join_roster_skins_and_sort_by_runtime_id() {
    let skin = protocol::PlayerSkin::Standard(protocol::StandardSkin {
        width: 64,
        height: 64,
        rgba8: vec![9; 64 * 64 * 4].into(),
    });
    let mut store = ActorStore::new(1, 0);
    for (sequence, runtime_id, unique_id, uuid) in [(1, 20, 2, [2; 16]), (2, 10, 1, [1; 16])] {
        let mut event = spawn(runtime_id, unique_id);
        let ActorEvent::Spawn(spawn) = &mut event else {
            unreachable!()
        };
        spawn.kind = ActorKind::Player {
            uuid,
            username: format!("player-{runtime_id}").into(),
        };
        store.apply(1, sequence, event);
    }
    store.apply(
        1,
        3,
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Add {
                uuid: [1; 16],
                unique_id: 1,
                username: "player-10".into(),
                verified: true,
                skin: skin.clone(),
            }]),
        }),
    );

    let players = store.render_players(None);
    assert_eq!(
        players
            .iter()
            .map(|(actor, _)| actor.runtime_id)
            .collect::<Vec<_>>(),
        [10, 20]
    );
    assert_eq!(players[0].1.map(|profile| &profile.skin), Some(&skin));
    assert!(players[1].1.is_none());

    let remote_players = store.render_players(Some(10));
    assert_eq!(remote_players.len(), 1);
    assert_eq!(remote_players[0].0.runtime_id, 20);
}

#[test]
fn incremental_player_lists_cannot_exceed_the_store_skin_byte_budget() {
    let skin_bytes = 64 * 64 * 4;
    let skin = |value| {
        PlayerSkin::Standard(StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![value; skin_bytes].into(),
        })
    };
    let add = |uuid, unique_id, skin| {
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Add {
                uuid,
                unique_id,
                username: format!("player-{unique_id}").into(),
                verified: true,
                skin,
            }]),
        })
    };
    let mut store = ActorStore::with_limits(1, 0, 2, 2, skin_bytes);

    assert_eq!(
        store.apply(1, 1, add([1; 16], 1, skin(1))),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply(1, 2, add([2; 16], 2, skin(2))),
        ActorApplyResult::Updated
    );
    assert_eq!(store.retained_player_skin_bytes, skin_bytes);
    assert_eq!(
        store.players[&[2; 16]].skin,
        PlayerSkin::Unavailable(PlayerSkinUnavailable::RetainedBudgetExceeded)
    );

    let oversized_replacement = PlayerSkin::Standard(StandardSkin {
        width: 128,
        height: 128,
        rgba8: vec![9; 128 * 128 * 4].into(),
    });
    store.apply(1, 3, add([1; 16], 10, oversized_replacement));
    assert_eq!(store.retained_player_skin_bytes, skin_bytes);
    assert_eq!(store.players[&[1; 16]].unique_id, 10);
    assert_eq!(store.players[&[1; 16]].skin, skin(1));

    store.apply(
        1,
        4,
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Remove { uuid: [1; 16] }]),
        }),
    );
    assert_eq!(store.retained_player_skin_bytes, 0);
    store.apply(1, 5, add([2; 16], 2, skin(3)));
    assert_eq!(store.retained_player_skin_bytes, skin_bytes);
    assert!(matches!(
        store.players[&[2; 16]].skin,
        PlayerSkin::Standard(_)
    ));
}
