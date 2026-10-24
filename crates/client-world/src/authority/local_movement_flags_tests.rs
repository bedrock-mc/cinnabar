use super::*;
use protocol_ids::{ActorDataKey, ActorFlag, Attribute};

/// Creates authority whose local actor has not arrived yet.
fn spawn_authority() -> WorldAuthority {
    WorldAuthority::new(
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
    )
}

/// Supplies a local spawn with optional item-use metadata.
fn spawn_with_use(using: Option<bool>) -> ActorSpawnEvent {
    ActorSpawnEvent {
        dimension: 2,
        unique_id: 5,
        runtime_id: 41,
        kind: ActorKind::Player {
            uuid: [0; 16],
            username: "local".into(),
        },
        position: [0.0; 3],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: using
            .into_iter()
            .map(|using| ActorMetadata {
                key: ActorDataKey::Flags.wire_value(),
                value: ActorMetadataValue::Flags(if using {
                    1 << ActorFlag::UsingItem.wire_value()
                } else {
                    0
                }),
            })
            .collect(),
        attributes: [].into(),
        properties: [].into(),
        links: [].into(),
    }
}

/// Only an accepted local spawn replaces item-use authority, including an absent flag word.
#[test]
fn local_spawn_use_follows_admission_and_replacement() {
    let mut authority = spawn_authority();
    for (sequence, using) in [
        (1, Some(true)),
        (2, None),
        (3, Some(false)),
        (4, Some(true)),
    ] {
        authority
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Spawn(spawn_with_use(using))),
                Some(sequence),
            )
            .unwrap();
        assert!(
            authority
                .take_committed_controls()
                .iter()
                .any(|control| matches!(control,
                    CommittedControlEvent::LocalMovementFlags { sequence: received, tick: 0, flags, .. }
                    if *received == sequence && flags.using_item == Some(using.unwrap_or(false))
                ))
        );
    }
    let mut remote = spawn_with_use(Some(false));
    remote.runtime_id += 1;
    remote.unique_id += 1;
    authority
        .apply_ordered_event(WorldEvent::Actor(ActorEvent::Spawn(remote)), Some(5))
        .unwrap();
    let mut wrong_dimension = spawn_with_use(Some(false));
    wrong_dimension.dimension += 1;
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(wrong_dimension)),
            Some(6),
        )
        .unwrap();
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(spawn_with_use(Some(false)))),
            Some(4),
        )
        .unwrap();
    let mut rejected = spawn_with_use(Some(false));
    rejected.links = [protocol::ActorLinkEvent {
        dimension: 3,
        rider_unique_id: 5,
        ridden_unique_id: 9,
        link_type: protocol::ActorLinkType::Rider,
        immediate: false,
        rider_initiated: false,
    }]
    .into();
    authority
        .apply_ordered_event(WorldEvent::Actor(ActorEvent::Spawn(rejected)), Some(7))
        .unwrap();
    assert!(authority.take_committed_controls().is_empty());
    assert!(authority.actor(41).unwrap().is_using_item());
}

/// Spawn flags, attributes and a death can be retained together within admission capacity.
#[test]
fn local_spawn_retains_use_speed_and_death_controls() {
    let mut authority = spawn_authority();
    let mut spawn = spawn_with_use(Some(true));
    spawn.attributes = [ActorAttribute {
        name: Attribute::Health.wire_name().into(),
        min: 0.0,
        max: 20.0,
        current: 0.0,
        default: Some(20.0),
        modifiers: [].into(),
    }]
    .into();
    authority
        .apply_ordered_event(WorldEvent::Actor(ActorEvent::Spawn(spawn)), Some(1))
        .unwrap();
    let controls = authority.take_committed_controls();
    assert!(controls.iter().any(|control| matches!(control, CommittedControlEvent::LocalMovementFlags { flags, .. } if flags.using_item == Some(true))));
    assert!(controls.iter().any(|control| matches!(
        control,
        CommittedControlEvent::LocalMovementSpeed {
            movement_received: true,
            ..
        }
    )));
    assert!(controls.contains(&CommittedControlEvent::LocalDeath { sequence: 1 }));
}

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

/// Sends one sparse size component through ordinary actor metadata admission.
fn dimension_metadata(runtime_id: u64, dimension: i32, value: f32) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
        runtime_id,
        dimension,
        metadata: Arc::from([ActorMetadata {
            key: ActorDataKey::Width.wire_value(),
            value: ActorMetadataValue::Float(value),
        }]),
        properties: Arc::from([]),
        tick: 0,
    }))
}

#[test]
fn sparse_dimensions_share_the_ordered_movement_authority_queue() {
    let mut authority = spawn_authority();
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(spawn_with_use(None))),
            Some(1),
        )
        .unwrap();
    authority.take_committed_controls();
    for (sequence, event) in [
        (2, dimension_metadata(41, 2, 0.8)),
        (3, metadata(41, 2, 1 << ActorFlag::Sneaking.wire_value())),
        (4, dimension_metadata(41, 2, 0.8)),
        (5, dimension_metadata(41, 2, f32::NAN)),
        (6, dimension_metadata(41, 3, 0.9)),
        (7, dimension_metadata(72, 2, 0.9)),
    ] {
        authority
            .apply_ordered_event(event, Some(sequence))
            .unwrap();
    }
    let controls = authority.take_committed_controls();
    let writes: Vec<_> = controls
        .into_iter()
        .map(|control| {
            let CommittedControlEvent::LocalMovementFlags {
                sequence,
                flags,
                dimensions,
                ..
            } = control
            else {
                panic!("expected movement metadata");
            };
            (
                sequence,
                flags.sneaking,
                dimensions
                    .and_then(|size| size.width)
                    .map(|write| (write.sequence, write.value)),
            )
        })
        .collect();
    assert_eq!(
        writes,
        [
            (2, None, Some((2, 0.8))),
            (3, Some(true), None),
            (4, None, Some((4, 0.8)))
        ]
    );
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
                metadata(runtime_id, dimension, 1 << ActorFlag::NoAI.wire_value()),
                Some(sequence),
            )
            .unwrap();
    }
    assert!(authority.take_committed_controls().is_empty());
    authority
        .apply_ordered_event(metadata(41, 2, 1 << ActorFlag::NoAI.wire_value()), Some(3))
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
            ..
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

fn air_drag(current: f32, tick: u64) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Attributes(ActorAttributesUpdateEvent {
        dimension: 2,
        runtime_id: 41,
        attributes: Arc::from([ActorAttribute {
            name: Arc::from(Attribute::AirDragModifier.wire_name()),
            min: 0.0,
            max: f32::MAX,
            current,
            default: None,
            modifiers: Arc::from([]),
        }]),
        tick,
    }))
}

#[test]
fn local_air_drag_modifier_commits_finite_values_with_their_tick() {
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
    authority
        .apply_ordered_event(air_drag(f32::NAN, 7), Some(1))
        .unwrap();
    assert!(
        !authority
            .take_committed_controls()
            .iter()
            .any(|control| matches!(
                control,
                CommittedControlEvent::LocalMovementSpeed {
                    air_drag_modifier: Some(_),
                    ..
                }
            ))
    );
    authority
        .apply_ordered_event(air_drag(2.5, 8), Some(2))
        .unwrap();
    assert!(
        authority
            .take_committed_controls()
            .iter()
            .any(|control| matches!(
                    control,
                    CommittedControlEvent::LocalMovementSpeed {
                sequence: 2,
                air_drag_modifier: Some(current),
                tick: 8,
                ..
            } if *current == 2.5
                ))
    );
}

/// A full actor budget rejects a new flag word while retaining updates to an existing one.
#[test]
fn local_metadata_controls_follow_partial_admission() {
    let mut authority = spawn_authority();
    let mut spawn = spawn_with_use(None);
    spawn.metadata = (1..=protocol::MAX_ACTOR_METADATA_ENTRIES as u32)
        .map(|key| ActorMetadata {
            key,
            value: ActorMetadataValue::Int(0),
        })
        .collect();
    authority
        .apply_ordered_event(WorldEvent::Actor(ActorEvent::Spawn(spawn)), Some(1))
        .unwrap();
    authority.take_committed_controls();
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 2,
                runtime_id: 41,
                tick: 0,
                metadata: [
                    ActorMetadata {
                        key: ActorDataKey::Flags.wire_value(),
                        value: ActorMetadataValue::Flags(1 << ActorFlag::UsingItem.wire_value()),
                    },
                    ActorMetadata {
                        key: ActorDataKey::FlagsTwo.wire_value(),
                        value: ActorMetadataValue::FlagsExtended(u64::MAX),
                    },
                ]
                .into(),
                properties: [].into(),
            })),
            Some(2),
        )
        .unwrap();
    assert!(!authority.actor(41).unwrap().is_using_item());
    let controls = authority.take_committed_controls();
    assert!(
        matches!(controls.as_slice(), [CommittedControlEvent::LocalMovementFlags { flags, .. }]
        if flags.using_item.is_none() && flags.crawling == Some(true))
    );
}

/// Stale actor packets cannot replace already accepted flag or attribute controls.
#[test]
fn local_movement_controls_skip_stale_packets() {
    let mut authority = spawn_authority();
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(spawn_with_use(Some(true)))),
            Some(3),
        )
        .unwrap();
    authority.take_committed_controls();
    authority
        .apply_ordered_event(metadata(41, 2, 0), Some(2))
        .unwrap();
    assert!(authority.take_committed_controls().is_empty());
    assert!(authority.actor(41).unwrap().is_using_item());
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Attributes(ActorAttributesUpdateEvent {
                dimension: 2,
                runtime_id: 41,
                tick: 0,
                attributes: [ActorAttribute {
                    name: Attribute::Movement.wire_name().into(),
                    min: 0.0,
                    max: 1.0,
                    current: 0.2,
                    default: Some(0.2),
                    modifiers: [].into(),
                }]
                .into(),
            })),
            Some(2),
        )
        .unwrap();
    assert_eq!(authority.local_movement_speed(), None);
    assert!(authority.take_committed_controls().is_empty());
}

/// Rejected attributes cannot create movement authority absent from the admitted actor.
#[test]
fn local_attribute_controls_follow_partial_admission() {
    let mut authority = spawn_authority();
    let mut spawn = spawn_with_use(None);
    spawn.attributes = (0..protocol::MAX_ACTOR_ATTRIBUTES)
        .map(|index| ActorAttribute {
            name: if index == 0 {
                Attribute::AirDragModifier.wire_name().into()
            } else {
                format!("fixture:{index}").into()
            },
            min: 0.0,
            max: 1.0,
            current: 0.1,
            default: Some(0.1),
            modifiers: [].into(),
        })
        .collect();
    authority
        .apply_ordered_event(WorldEvent::Actor(ActorEvent::Spawn(spawn)), Some(1))
        .unwrap();
    authority.take_committed_controls();
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Attributes(ActorAttributesUpdateEvent {
                dimension: 2,
                runtime_id: 41,
                tick: 0,
                attributes: [
                    ActorAttribute {
                        name: Attribute::Movement.wire_name().into(),
                        min: 0.0,
                        max: 1.0,
                        current: 0.2,
                        default: Some(0.2),
                        modifiers: [].into(),
                    },
                    ActorAttribute {
                        name: Attribute::AirDragModifier.wire_name().into(),
                        min: 0.0,
                        max: 10.0,
                        current: 2.0,
                        default: Some(1.0),
                        modifiers: [].into(),
                    },
                ]
                .into(),
            })),
            Some(2),
        )
        .unwrap();
    assert!(
        !authority
            .actor(41)
            .unwrap()
            .attributes
            .contains_key(Attribute::Movement.wire_name())
    );
    assert_eq!(authority.local_movement_speed(), None);
    assert!(matches!(
        authority.take_committed_controls().as_slice(),
        [CommittedControlEvent::LocalMovementSpeed {
            movement_received: false,
            air_drag_modifier: Some(2.0),
            ..
        }]
    ));
}
