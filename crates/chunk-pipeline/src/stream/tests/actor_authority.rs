use super::*;

#[test]
fn local_actor_motion_commits_as_a_control_event_and_foreign_motion_is_dropped() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let motion = |actor_runtime_id: u64| {
        WorldEvent::ActorMotion(ActorMotionEvent {
            actor_runtime_id,
            motion: [1.5, 0.25, -0.75],
            tick: 7,
        })
    };

    // A foreign actor's impulse has no velocity consumer yet.
    stream.submit(1, motion(2)).unwrap();
    stream.submit(2, motion(1)).unwrap();

    let controls = stream.take_committed_controls();
    let [super::CommittedControlEvent::LocalActorMotion { sequence, event }] = controls.as_slice()
    else {
        panic!("unexpected committed controls {controls:?}");
    };
    assert_eq!(*sequence, 2);
    assert_eq!(event.actor_runtime_id, 1);
    assert_eq!(event.motion, [1.5, 0.25, -0.75]);
    assert_eq!(event.tick, 7);
}

#[test]
fn latency_fences_retain_motion_order_and_restored_controls_keep_admission_bounded() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::ActorMotion(ActorMotionEvent {
                actor_runtime_id: 1,
                motion: [0.0, 0.4, 0.0],
                tick: 0,
            }),
        )
        .unwrap();
    stream
        .submit(2, WorldEvent::NetworkStackLatency(777))
        .unwrap();
    stream
        .submit(3, WorldEvent::NetworkStackLatency(888))
        .unwrap();
    let capacity = stream.remaining_admission_capacity();
    let controls = stream.take_committed_controls();
    assert!(matches!(
        controls.as_slice(),
        [
            CommittedControlEvent::LocalActorMotion { sequence: 1, .. },
            CommittedControlEvent::NetworkStackLatency {
                sequence: 2,
                creation_time: 777
            },
            CommittedControlEvent::NetworkStackLatency {
                sequence: 3,
                creation_time: 888
            },
        ]
    ));
    stream.restore_committed_controls(controls.clone().into_iter());
    assert_eq!(stream.remaining_admission_capacity(), capacity);
    assert_eq!(stream.take_committed_controls(), controls);
}

#[test]
fn player_list_only_updates_commit_a_tab_cache_refresh_marker() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::PlayerList(PlayerListUpdateEvent {
                entries: Arc::from([PlayerListEntry::Remove { uuid: [7; 16] }]),
            })),
        )
        .unwrap();

    assert_eq!(
        stream.take_committed_controls(),
        vec![CommittedControlEvent::PlayerListChanged { sequence: 1 }]
    );
    assert!(stream.take_committed_ui().is_empty());
}

#[test]
fn respawn_commits_as_a_local_position_authority_change() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let respawn = RespawnEvent {
        position: [8.5, 71.620_01, -4.25],
        state: 1,
        runtime_entity_id: 1,
    };

    stream.submit(1, WorldEvent::Respawn(respawn)).unwrap();

    assert_eq!(
        stream.take_committed_ui(),
        vec![CommittedUiEvent::LocalRespawnReady { sequence: 1 }]
    );

    assert_eq!(
        stream.take_committed_controls(),
        vec![super::CommittedControlEvent::Respawn {
            sequence: 1,
            respawn,
            resolved: super::server_position::ResolvedServerPosition {
                position: respawn.position,
                surface_anchor: None,
            },
        }]
    );
}

#[test]
fn respawn_search_and_unknown_phases_keep_live_position_authority() {
    let position = [3.5, 70.0, -4.5];
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: position,
        world_spawn_position: [3, 70, -5],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    for (sequence, state) in [(1, 0), (2, 2), (3, u8::MAX)] {
        let respawn = RespawnEvent {
            position: [300.5, 32_767.0, 400.5],
            state,
            runtime_entity_id: 0,
        };
        stream
            .submit(sequence, WorldEvent::Respawn(respawn))
            .unwrap();
        assert_eq!(stream.resolved_server_position().position, position);
        let controls = stream.take_committed_controls();
        let [
            CommittedControlEvent::Respawn {
                respawn: observed,
                resolved,
                ..
            },
        ] = controls.as_slice()
        else {
            panic!("ordered phase control");
        };
        assert_eq!(*observed, respawn);
        assert_eq!(resolved.position, position);
        assert_eq!(resolved.surface_anchor, None);
        assert!(stream.take_committed_ui().is_empty());
    }
}

#[test]
fn older_movement_correction_tick_cannot_rewind_newer_correction() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let correction = |tick, position| PlayerMovementCorrectionEvent {
        position,
        delta: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        subject: MovementCorrectionSubject::Player,
        on_ground: true,
        tick,
    };
    let newer = correction(100, [100.0, 80.0, 100.0]);
    let older = correction(99, [10.0, 70.0, 10.0]);

    stream
        .submit(1, WorldEvent::PlayerMovementCorrection(newer))
        .unwrap();
    stream
        .submit(2, WorldEvent::PlayerMovementCorrection(older))
        .unwrap();

    assert_eq!(
        stream.take_committed_controls(),
        vec![super::CommittedControlEvent::PlayerMovementCorrection {
            sequence: 1,
            correction: newer,
            resolved: super::server_position::ResolvedServerPosition {
                position: newer.position,
                surface_anchor: None,
            },
        }]
    );
}

#[test]
fn vehicle_correction_subject_commits_nothing_and_cannot_advance_the_tick_guard() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let correction = |subject, tick| PlayerMovementCorrectionEvent {
        position: [50.0, 80.0, -50.0],
        delta: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        subject,
        on_ground: true,
        tick,
    };

    // A vehicle rewind has no local-player consumer yet: it must not resolve a
    // server position, commit a control, or advance the monotonic guard that
    // would silence a later ordinary player correction.
    stream
        .submit(
            1,
            WorldEvent::PlayerMovementCorrection(correction(
                MovementCorrectionSubject::Vehicle,
                200,
            )),
        )
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::PlayerMovementCorrection(correction(
                MovementCorrectionSubject::Player,
                100,
            )),
        )
        .unwrap();

    let controls = stream.take_committed_controls();
    let [
        super::CommittedControlEvent::PlayerMovementCorrection {
            sequence,
            correction,
            ..
        },
    ] = controls.as_slice()
    else {
        panic!("unexpected committed controls {controls:?}");
    };
    assert_eq!(*sequence, 2);
    assert_eq!(correction.tick, 100);
    assert_eq!(correction.subject, MovementCorrectionSubject::Player);
}

fn riding_stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    })
}

const fn actor_link(
    rider_unique_id: i64,
    ridden_unique_id: i64,
    link_type: ActorLinkType,
) -> ActorLinkEvent {
    ActorLinkEvent {
        dimension: 0,
        ridden_unique_id,
        rider_unique_id,
        link_type,
        immediate: false,
        rider_initiated: false,
    }
}

fn linked_spawn(unique_id: i64, runtime_id: u64, links: Arc<[ActorLinkEvent]>) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id,
        runtime_id,
        kind: ActorKind::Entity {
            identifier: "minecraft:boat".into(),
        },
        position: [0.0; 3],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links,
    }))
}

#[test]
fn embedded_links_publish_one_final_local_mount_and_actor_removal_dismounts() {
    let mut stream = riding_stream();
    stream
        .submit(
            1,
            linked_spawn(
                50,
                50,
                Arc::from([
                    actor_link(1, 40, ActorLinkType::Rider),
                    actor_link(1, 50, ActorLinkType::Passenger),
                    actor_link(1, 60, ActorLinkType::Unknown(9)),
                ]),
            ),
        )
        .unwrap();
    assert_eq!(stream.authority.ridden_unique_id(1), Some(50));
    assert_eq!(
        stream.take_committed_ui(),
        vec![CommittedUiEvent::LocalMount {
            sequence: 1,
            ridden_unique_id: Some(50),
        }]
    );

    stream
        .submit(
            2,
            WorldEvent::Actor(ActorEvent::Remove(ActorRemoveEvent {
                dimension: 0,
                unique_id: 50,
            })),
        )
        .unwrap();
    assert_eq!(stream.authority.ridden_unique_id(1), None);
    assert_eq!(
        stream.take_committed_ui(),
        vec![CommittedUiEvent::LocalMount {
            sequence: 2,
            ridden_unique_id: None,
        }]
    );
}

#[test]
fn world_stream_retains_remote_link_before_spawn_without_local_ui() {
    let mut stream = riding_stream();
    stream
        .submit(
            1,
            WorldEvent::ActorLink(actor_link(7, 8, ActorLinkType::Rider)),
        )
        .unwrap();
    assert_eq!(stream.authority.ridden_unique_id(7), Some(8));
    assert!(stream.take_committed_ui().is_empty());

    stream.submit(2, linked_spawn(7, 7, Arc::from([]))).unwrap();
    assert_eq!(stream.authority.ridden_unique_id(7), Some(8));
}

#[test]
fn local_attributes_commit_without_requiring_a_local_actor_spawn() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let health = ActorAttribute {
        name: Arc::from("minecraft:health"),
        min: 0.0,
        max: 20.0,
        current: 17.5,
        default: Some(20.0),
        modifiers: Arc::from([]),
    };

    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Attributes(ActorAttributesUpdateEvent {
                dimension: 0,
                runtime_id: 42,
                attributes: Arc::from([health.clone()]),
                tick: 99,
            })),
        )
        .unwrap();

    assert!(stream.authority().actor(42).is_none());
    assert_eq!(
        stream.take_committed_ui(),
        vec![CommittedUiEvent::LocalAttributes {
            sequence: 1,
            server_tick: 99,
            attributes: Arc::from([health]),
        }]
    );
}

#[test]
fn stale_dimension_local_attributes_do_not_commit_to_the_hud() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });

    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Attributes(ActorAttributesUpdateEvent {
                dimension: 1,
                runtime_id: 42,
                attributes: Arc::from([ActorAttribute {
                    name: Arc::from("minecraft:health"),
                    min: 0.0,
                    max: 20.0,
                    current: 1.0,
                    default: Some(20.0),
                    modifiers: Arc::from([]),
                }]),
                tick: 100,
            })),
        )
        .unwrap();

    assert!(stream.take_committed_ui().is_empty());
}

fn movement_attribute(current: f32) -> ActorAttribute {
    ActorAttribute {
        name: Arc::from("minecraft:movement"),
        min: 0.0,
        max: 1024.0,
        current,
        default: Some(0.1),
        modifiers: Arc::from([]),
    }
}

#[test]
fn movement_authority_retains_effective_current_and_identifies_only_native_sprint() {
    let modifier = protocol::ActorAttributeModifier {
        id: Arc::from(protocol_ids::SPRINT_SPEED_MODIFIER_ID.to_uppercase()),
        name: Arc::from("unrelated label"),
        amount: 0.3,
        operation: 2,
        operand: 2,
        serializable: true,
    };
    let mut stream = riding_stream();
    let mut sequence = 0;
    let mut submit = |modifiers: Arc<[protocol::ActorAttributeModifier]>,
                      current: f32,
                      expected_modifier: Option<f32>| {
        sequence += 1;
        let mut attribute = movement_attribute(current);
        attribute.modifiers = modifiers;
        stream
            .submit(
                sequence,
                WorldEvent::Actor(ActorEvent::Attributes(ActorAttributesUpdateEvent {
                    dimension: 0,
                    runtime_id: 1,
                    attributes: Arc::from([attribute]),
                    tick: sequence,
                })),
            )
            .unwrap();
        assert_eq!(stream.local_movement_speed(), Some(f64::from(current)));
        assert!(matches!(stream.take_committed_controls().as_slice(), [
                CommittedControlEvent::LocalMovementSpeed { movement: Some(value), .. }
            ] if value.current == f64::from(current) && value.sprint_modifier == expected_modifier));
    };
    let factor = Some(1.0 + modifier.amount);
    submit(Arc::from([modifier.clone()]), 0.13, factor);
    submit(Arc::from([modifier.clone()]), 0.156, factor);
    submit(Arc::from([modifier.clone()]), 0.0, factor);
    submit(Arc::from([]), 0.13, None);
    let mut custom = modifier.clone();
    custom.id = Arc::from("custom-speed");
    custom.name = Arc::from("Sprinting speed boost");
    submit(Arc::from([custom]), 0.13, None);
    let mut different_operation = modifier.clone();
    different_operation.operation = 1;
    submit(Arc::from([different_operation]), 0.13, None);
    let mut different_operand = modifier;
    different_operand.operand = 1;
    submit(Arc::from([different_operand]), 0.13, None);
}

#[test]
fn ambiguous_or_invalid_sprint_authority_keeps_previous_speed_and_session() {
    let modifier = protocol::ActorAttributeModifier {
        id: Arc::from(protocol_ids::SPRINT_SPEED_MODIFIER_ID),
        name: Arc::from(""),
        amount: 0.3,
        operation: 2,
        operand: 2,
        serializable: true,
    };
    let mut stream = riding_stream();
    for (index, modifiers) in [
        Arc::from([]),
        Arc::from([modifier.clone(), modifier.clone()]),
        Arc::from([protocol::ActorAttributeModifier {
            amount: f32::NAN,
            ..modifier.clone()
        }]),
        Arc::from([protocol::ActorAttributeModifier {
            amount: -1.0,
            ..modifier
        }]),
    ]
    .into_iter()
    .enumerate()
    {
        let mut attribute = movement_attribute(if index == 0 { 0.25 } else { 0.13 });
        attribute.modifiers = modifiers;
        let sequence = index as u64 + 1;
        stream
            .submit(
                sequence,
                WorldEvent::Actor(ActorEvent::Attributes(ActorAttributesUpdateEvent {
                    dimension: 0,
                    runtime_id: 1,
                    attributes: Arc::from([attribute]),
                    tick: sequence,
                })),
            )
            .unwrap();
        assert_eq!(stream.local_movement_speed(), Some(0.25));
    }
}

#[test]
fn local_movement_authority_commits_in_fifo_order_and_accepts_zero_updates() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            2,
            WorldEvent::Actor(ActorEvent::Attributes(ActorAttributesUpdateEvent {
                dimension: 0,
                runtime_id: 42,
                attributes: Arc::from([movement_attribute(0.0)]),
                tick: 2,
            })),
        )
        .unwrap();
    assert_eq!(stream.local_movement_speed(), None);

    stream
        .submit(1, WorldEvent::SetTime(SetTimeEvent { time: 0 }))
        .unwrap();
    assert_eq!(stream.local_movement_speed(), Some(0.0));
    assert!(matches!(
        stream.take_committed_controls().as_slice(),
        [
            CommittedControlEvent::SetTime { sequence: 1, .. },
            CommittedControlEvent::LocalMovementSpeed {
                sequence: 2,
                dimension: 0,
                movement: Some(value),
                underwater: None,
                lava: None,
                air_drag_modifier: None,
                tick: 2,
            }
        ] if value.current == 0.0 && value.sprint_modifier.is_none()
    ));
}

#[test]
fn movement_authority_skips_wrong_actor_dimension_and_invalid_values_then_clears_on_transfer() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    for (sequence, dimension, runtime_id, current) in [
        (1, 0, 42, 0.25),
        (2, 0, 7, 0.5),
        (3, 1, 42, 0.75),
        (4, 0, 42, -1.0),
        (5, 0, 42, f32::NAN),
        (6, 0, 42, f32::INFINITY),
    ] {
        stream
            .submit(
                sequence,
                WorldEvent::Actor(ActorEvent::Attributes(ActorAttributesUpdateEvent {
                    dimension,
                    runtime_id,
                    attributes: Arc::from([movement_attribute(current)]),
                    tick: sequence,
                })),
            )
            .unwrap();
    }
    assert_eq!(stream.local_movement_speed(), Some(0.25));

    stream
        .submit(
            7,
            WorldEvent::Actor(ActorEvent::Attributes(ActorAttributesUpdateEvent {
                dimension: 0,
                runtime_id: 42,
                attributes: Arc::from([movement_attribute(0.3), movement_attribute(-1.0)]),
                tick: 7,
            })),
        )
        .unwrap();
    assert_eq!(stream.local_movement_speed(), Some(f64::from(0.3_f32)));

    stream
        .submit(
            8,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0; 3],
                ..Default::default()
            }),
        )
        .unwrap();
    assert_eq!(stream.local_movement_speed(), None);
}

#[test]
fn local_effect_commits_to_movement_control_and_ui_together() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let effect = protocol::ActorEffectEvent {
        dimension: 0,
        actor_runtime_id: 42,
        action: protocol::ActorEffectAction::Add,
        effect_id: 27,
        amplifier: 0,
        particles: true,
        ambient: false,
        duration_ticks: 40,
        tick: 99,
    };

    stream.submit(1, WorldEvent::ActorEffect(effect)).unwrap();

    assert_eq!(
        stream.take_committed_controls(),
        vec![CommittedControlEvent::LocalMovementEffect {
            sequence: 1,
            event: effect,
        }]
    );
    assert_eq!(
        stream.take_committed_ui(),
        vec![CommittedUiEvent::LocalEffect {
            sequence: 1,
            event: effect,
        }]
    );
}

#[test]
fn remote_or_wrong_dimension_effect_does_not_reach_local_movement() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    for (sequence, runtime_id, dimension) in [(1, 7, 0), (2, 42, 1)] {
        stream
            .submit(
                sequence,
                WorldEvent::ActorEffect(protocol::ActorEffectEvent {
                    dimension,
                    actor_runtime_id: runtime_id,
                    action: protocol::ActorEffectAction::Add,
                    effect_id: 8,
                    amplifier: 0,
                    particles: true,
                    ambient: false,
                    duration_ticks: 40,
                    tick: 99,
                }),
            )
            .unwrap();
    }

    assert!(stream.take_committed_controls().is_empty());
    assert!(stream.take_committed_ui().is_empty());
}

#[test]
fn player_spawn_move_player_and_absolute_move_share_feet_space() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 7,
                runtime_id: 8,
                kind: ActorKind::Player {
                    uuid: [8; 16],
                    username: "Alex".into(),
                },
                position: [1.0, 64.0, 2.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: 8,
                position: [1.0, 64.0 + PLAYER_NETWORK_OFFSET, 2.0],
                on_ground: true,
                ..Default::default()
            }),
        )
        .unwrap();
    stream.advance_actor_interpolation_ticks(3);
    assert_eq!(
        stream.authority().actor(8).unwrap().position,
        [1.0, 64.0, 2.0]
    );

    stream
        .submit(
            3,
            WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
                dimension: 0,
                runtime_id: 8,
                position: [Some(1.0), Some(64.0 + PLAYER_NETWORK_OFFSET), Some(2.0)],
                position_origin: ActorPositionOrigin::NetworkOffset,
                pitch: None,
                yaw: None,
                head_yaw: None,
                on_ground: Some(true),
                teleported: true,
                force_local_move: false,
                player_mode: None,
                source_tick: None,
                interpolation: Default::default(),
            })),
        )
        .unwrap();
    let actor = stream.authority().actor(8).unwrap();
    assert_eq!(actor.previous_pose.position, [1.0, 64.0, 2.0]);
    assert_eq!(actor.position, [1.0, 64.0, 2.0]);
    assert_eq!(actor.received_pose.position, [1.0, 64.0, 2.0]);
}

#[test]
fn remote_projectile_motion_replaces_the_retained_velocity() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let spawn = ActorSpawnEvent {
        dimension: 0,
        unique_id: 77,
        runtime_id: 77,
        kind: ActorKind::Entity {
            identifier: "minecraft:ender_pearl".into(),
        },
        position: [0.0; 3],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: protocol::NetworkItemStack::empty(),
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    };
    stream
        .submit(1, WorldEvent::Actor(ActorEvent::Spawn(spawn)))
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::ActorMotion(ActorMotionEvent {
                actor_runtime_id: 77,
                motion: [0.5, 0.2, -0.75],
                tick: 7,
            }),
        )
        .unwrap();
    assert_eq!(
        stream.authority().actor(77).unwrap().velocity,
        [0.5, 0.2, -0.75]
    );
    assert!(stream.take_committed_controls().is_empty());
}
