use super::*;
use crate::item_use::ItemUseRuntime;
use client_world::CommittedControlEvent;
use std::sync::Arc;

/// Reports the item-use flag accepted by the committed physics authority.
fn apply_use_controls(
    world: &mut client_world::WorldAuthority,
    physics: &mut LocalPhysicsController,
    runtime: &mut ItemUseRuntime,
) {
    super::state_updates::apply_controls(world, physics);
    runtime.adopt_server_using_item(physics.take_server_using_item());
}

/// Reads a due sync from the current accepted actor without admitting it.
fn reported_packet(
    world: &client_world::WorldAuthority,
    physics: &mut LocalPhysicsController,
    runtime: &ItemUseRuntime,
) -> protocol::Packet {
    let mut sent = None;
    send_movement_prediction_sync(
        physics,
        &LocalActor(world.actor(7).unwrap().clone()),
        None,
        None,
        runtime.reported_using_item(),
        |packet| {
            sent = Some(packet);
            false
        },
    );
    sent.expect("due correction sync")
}

/// Supplies one live primary flag word through world authority.
fn write_use(world: &mut client_world::WorldAuthority, sequence: u64, using: bool) {
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                protocol::ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 7,
                    metadata: Arc::from([protocol::ActorMetadata {
                        key: ActorDataKey::Flags.wire_value(),
                        value: ActorMetadataValue::Flags(if using {
                            1 << ActorFlag::UsingItem.wire_value()
                        } else {
                            0
                        }),
                    }]),
                    properties: Arc::from([]),
                    tick: 0,
                },
            )),
            Some(sequence),
        )
        .unwrap();
}

/// Checks the item-use bit in a real emitted sync.
fn assert_use(
    world: &client_world::WorldAuthority,
    physics: &mut LocalPhysicsController,
    runtime: &ItemUseRuntime,
    expected: bool,
) {
    let packet = reported_packet(world, physics, runtime);
    let flags = protocol::test_support::packets::prediction_sync_flags(&packet).unwrap();
    assert_eq!(flag_set(&flags, ActorFlag::UsingItem), expected);
}

/// A replacement's cleared flag supersedes a use accepted for the previous actor.
#[test]
fn sync_lifetime_replacement_reseeds_item_use() {
    for replacement_use in [None, Some(false), Some(true)] {
        let mut world = local_actor_authority();
        let mut runtime = ItemUseRuntime::default();
        let mut physics = due_physics();
        world.take_committed_controls();
        write_use(&mut world, 2, !replacement_use.unwrap_or(false));
        apply_use_controls(&mut world, &mut physics, &mut runtime);
        let mut replacement = local_actor_spawn(Vec::new());
        replacement.metadata = replacement_use
            .into_iter()
            .map(|using| protocol::ActorMetadata {
                key: ActorDataKey::Flags.wire_value(),
                value: ActorMetadataValue::Flags(if using {
                    1 << ActorFlag::UsingItem.wire_value()
                } else {
                    0
                }),
            })
            .collect();
        world
            .apply_ordered_event(
                protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(replacement)),
                Some(3),
            )
            .unwrap();
        assert_eq!(
            world.actor(7).unwrap().is_using_item(),
            replacement_use.unwrap_or(false)
        );
        apply_use_controls(&mut world, &mut physics, &mut runtime);
        assert_use(
            &world,
            &mut physics,
            &runtime,
            replacement_use.unwrap_or(false),
        );

        let (_, mut frame) = super::progress::food_use();
        runtime.observe_press(true);
        assert!(runtime.step(&frame).started);
        assert_use(&world, &mut physics, &runtime, true);
        frame.held = false;
        assert!(!runtime.step(&frame).packets.is_empty());
        assert_use(&world, &mut physics, &runtime, false);
        write_use(&mut world, 4, true);
        apply_use_controls(&mut world, &mut physics, &mut runtime);
        runtime.cancel_for_respawn();
        assert_use(&world, &mut physics, &runtime, true);
    }
}

/// A network replacement remains newer after many presentation feeds and synthetic recreation.
#[test]
fn sync_lifetime_network_dimensions_replace_a_synthetic_actor() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    let feed = super::super::presentation_tests::feed();
    for _ in 0..10 {
        world.sync_local_player_pose(&feed);
    }
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Remove(protocol::ActorRemoveEvent {
                dimension: 0,
                unique_id: 70,
            })),
            Some(2),
        )
        .unwrap();
    assert!(world.actor(7).is_none());
    world.sync_local_player_pose(&feed);
    super::state_updates::apply_controls(&mut world, &mut physics);
    let anchor = physics
        .advance_with_context(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            crate::movement::PhysicsSampleContext::default(),
            &OpenWorld,
        )
        .samples
        .remove(0);
    let mut replacement = local_actor_spawn(Vec::new());
    replacement.metadata = Arc::from([protocol::ActorMetadata {
        key: ActorDataKey::Height.wire_value(),
        value: ActorMetadataValue::Float(0.7),
    }]);
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(replacement)),
            Some(3),
        )
        .unwrap();
    super::state_updates::apply_controls(&mut world, &mut physics);
    assert_eq!(world.actor(7).unwrap().spawn_revision, 3);
    assert_eq!(
        world
            .actor(7)
            .unwrap()
            .explicit_dimensions
            .height
            .unwrap()
            .value,
        0.7
    );
    let packet = reported_packet(&world, &mut physics, &ItemUseRuntime::default());
    assert_eq!(
        protocol::test_support::packets::prediction_sync_bounding_box(&packet),
        Some([1.0, sim::PLAYER_WIDTH as f32, 0.7])
    );
    physics.advance_with_context(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        crate::movement::PhysicsSampleContext::default(),
        &OpenWorld,
    );
    physics
        .apply_correction(
            crate::movement::PhysicsAnchor {
                network_position: anchor.position,
                tick: anchor.tick,
                on_ground: false,
                velocity: Some(anchor.velocity),
            },
            crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
            None,
            &OpenWorld,
        )
        .unwrap();
    let packet = reported_packet(&world, &mut physics, &ItemUseRuntime::default());
    assert_eq!(
        protocol::test_support::packets::prediction_sync_bounding_box(&packet),
        Some([1.0, sim::PLAYER_WIDTH as f32, 0.7]),
        "replay must retain the accepted network lifetime"
    );
    physics.advance_with_context(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        crate::movement::PhysicsSampleContext::default(),
        &OpenWorld,
    );
    let packet = reported_packet(&world, &mut physics, &ItemUseRuntime::default());
    assert_eq!(
        protocol::test_support::packets::prediction_sync_bounding_box(&packet),
        Some([1.0, sim::PLAYER_WIDTH as f32, 0.7]),
        "a delayed frame cannot restore the synthetic lifetime"
    );
}

/// A later server write wins even when an older replacement is drained in the same batch.
#[test]
fn sync_lifetime_later_server_use_wins_after_replacement() {
    let mut world = local_actor_authority();
    let mut runtime = ItemUseRuntime::default();
    let mut physics = due_physics();
    world.take_committed_controls();
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(local_actor_spawn(Vec::new()))),
            Some(2),
        )
        .unwrap();
    write_use(&mut world, 3, true);
    apply_use_controls(&mut world, &mut physics, &mut runtime);
    assert_use(&world, &mut physics, &runtime, true);
}

/// Synthetic recreation retires explicit dimensions even without a local pose change.
#[test]
fn sync_lifetime_synthetic_recreation_clears_old_bounds() {
    for dimension_reset in [false, true] {
        let mut world = local_actor_authority();
        let mut physics = due_physics();
        super::state_updates::write_dimensions(
            &mut world,
            &mut physics,
            2,
            &[(ActorDataKey::Width, 0.9), (ActorDataKey::Height, 0.7)],
        );
        assert_eq!(
            super::state_updates::reported_box(&mut physics, &world),
            [1.0, 0.9, 0.7]
        );
        if dimension_reset {
            world.reset_dimension(3, 1);
            physics.reanchor_network_position([0.5, 72.0, 0.5], 101, false);
        } else {
            world
                .apply_ordered_event(
                    protocol::WorldEvent::Actor(protocol::ActorEvent::Remove(
                        protocol::ActorRemoveEvent {
                            dimension: 0,
                            unique_id: 70,
                        },
                    )),
                    Some(3),
                )
                .unwrap();
        }
        world.sync_local_player_pose(&super::super::presentation_tests::feed());
        super::state_updates::apply_controls(&mut world, &mut physics);
        for _ in 0..2 {
            assert_eq!(
                super::state_updates::reported_box(&mut physics, &world),
                [1.0, sim::PLAYER_WIDTH as f32, sim::PLAYER_HEIGHT as f32],
                "dimension reset: {dimension_reset}"
            );
            physics.advance(
                simulation_clock::TICK_DURATION,
                sim::MovementInput::default(),
                &OpenWorld,
            );
        }
    }
}

/// Controlled passengers report the replacement's Moving bit through correction replay.
#[test]
fn sync_lifetime_replacement_reseeds_controlled_mount_moving() {
    for ride in [
        crate::movement::RideKind::Boat,
        crate::movement::RideKind::Horse,
    ] {
        for replacement_moving in [None, Some(false), Some(true)] {
            let moving = replacement_moving.unwrap_or(false);
            let mut world = local_actor_authority();
            let mut physics = due_physics();
            world.take_committed_controls();
            physics.apply_server_movement_flags(
                0,
                client_world::MovementFlagUpdate {
                    moving: Some(!moving),
                    ..Default::default()
                },
            );
            let context = crate::movement::PhysicsSampleContext {
                mode_intent: crate::movement::ModeIntent {
                    ride: Some(ride),
                    vehicle_controlled_locally: true,
                    ..Default::default()
                },
                ..Default::default()
            };
            let anchor = physics
                .advance_with_context(
                    simulation_clock::TICK_DURATION,
                    sim::MovementInput::default(),
                    context,
                    &OpenWorld,
                )
                .samples
                .remove(0);
            let mut replacement = local_actor_spawn(Vec::new());
            replacement.metadata = replacement_moving
                .into_iter()
                .map(|moving| protocol::ActorMetadata {
                    key: ActorDataKey::Flags.wire_value(),
                    value: ActorMetadataValue::Flags(if moving {
                        1 << ActorFlag::Moving.wire_value()
                    } else {
                        0
                    }),
                })
                .collect();
            replacement.links = Arc::from([protocol::ActorLinkEvent {
                dimension: 0,
                rider_unique_id: 70,
                ridden_unique_id: 90,
                link_type: protocol::ActorLinkType::Rider,
                immediate: true,
                rider_initiated: false,
            }]);
            world
                .apply_ordered_event(
                    protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(replacement)),
                    Some(2),
                )
                .unwrap();
            assert_eq!(world.ridden_unique_id(70), Some(90));
            super::state_updates::apply_controls(&mut world, &mut physics);
            physics.advance_with_context(
                simulation_clock::TICK_DURATION,
                sim::MovementInput::default(),
                context,
                &OpenWorld,
            );
            physics
                .apply_correction(
                    crate::movement::PhysicsAnchor {
                        network_position: anchor.position,
                        tick: anchor.tick,
                        on_ground: false,
                        velocity: Some(anchor.velocity),
                    },
                    crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
                    None,
                    &OpenWorld,
                )
                .unwrap();
            let packet = reported_packet(&world, &mut physics, &ItemUseRuntime::default());
            let flags = protocol::test_support::packets::prediction_sync_flags(&packet).unwrap();
            assert_eq!(
                flag_set(&flags, ActorFlag::Moving),
                moving,
                "ride: {ride:?}"
            );
        }
    }
}

/// Accepted packets while the actor is absent become the synthetic actor's authority.
#[test]
fn sync_lifetime_server_updates_survive_synthetic_recreation() {
    for reset_dimension in [false, true] {
        let mut world = local_actor_authority();
        let mut physics = due_physics();
        if reset_dimension {
            world.reset_dimension(2, 1);
        } else {
            world
                .apply_ordered_event(
                    protocol::WorldEvent::Actor(protocol::ActorEvent::Remove(
                        protocol::ActorRemoveEvent {
                            dimension: 0,
                            unique_id: 70,
                        },
                    )),
                    Some(2),
                )
                .unwrap();
        }
        let dimension = world.current_dimension();
        world
            .apply_ordered_event(
                protocol::WorldEvent::Actor(protocol::ActorEvent::Attributes(
                    protocol::ActorAttributesUpdateEvent {
                        dimension,
                        runtime_id: 7,
                        tick: 0,
                        attributes: [
                            (Attribute::Movement, 0.2),
                            (Attribute::UnderwaterMovement, 0.3),
                            (Attribute::LavaMovement, 0.4),
                            (Attribute::AirDragModifier, 1.7),
                        ]
                        .map(|(name, current)| protocol::ActorAttribute {
                            name: name.wire_name().into(),
                            min: 0.0,
                            max: 10.0,
                            current,
                            default: Some(current),
                            modifiers: Arc::from([]),
                        })
                        .into(),
                    },
                )),
                Some(3),
            )
            .unwrap();
        world
            .apply_ordered_event(
                protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                    protocol::ActorMetadataUpdateEvent {
                        dimension,
                        runtime_id: 7,
                        tick: 0,
                        metadata: Arc::from([
                            protocol::ActorMetadata {
                                key: ActorDataKey::Flags.wire_value(),
                                value: ActorMetadataValue::Flags(
                                    (1 << ActorFlag::NoAI.wire_value())
                                        | (1 << ActorFlag::UsingItem.wire_value()),
                                ),
                            },
                            protocol::ActorMetadata {
                                key: ActorDataKey::Width.wire_value(),
                                value: ActorMetadataValue::Float(0.8),
                            },
                        ]),
                        properties: Arc::from([]),
                    },
                )),
                Some(4),
            )
            .unwrap();
        super::state_updates::apply_controls(&mut world, &mut physics);
        world.sync_local_player_pose(&super::super::presentation_tests::feed());
        super::state_updates::apply_controls(&mut world, &mut physics);
        for (name, current) in [
            (Attribute::Movement, 0.2),
            (Attribute::UnderwaterMovement, 0.3),
            (Attribute::LavaMovement, 0.4),
            (Attribute::AirDragModifier, 1.7),
        ] {
            assert_eq!(
                world
                    .actor(7)
                    .unwrap()
                    .attributes
                    .get(name.wire_name())
                    .map(|a| a.current),
                Some(current)
            );
        }
        assert_eq!(physics.prediction_sync_flags().immobile, Some(true));
        assert_eq!(physics.prediction_sync_flags().using_item, Some(true));
        assert_eq!(
            super::state_updates::reported_box(&mut physics, &world),
            [1.0, 0.8, sim::PLAYER_HEIGHT as f32]
        );
    }
}

/// Completed and still-pending replacements end the old travel mode through correction replay.
#[test]
fn sync_lifetime_replacement_mode_clear_survives_replay() {
    for complete_replacement in [true, false] {
        let mut world = local_actor_authority();
        let mut physics = due_physics();
        super::state_updates::apply_controls(&mut world, &mut physics);
        let context = crate::movement::PhysicsSampleContext {
            mode_intent: crate::movement::ModeIntent {
                elytra_ready: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let anchor = physics
            .advance_with_context(
                simulation_clock::TICK_DURATION,
                sim::MovementInput {
                    jumping: true,
                    ..Default::default()
                },
                context,
                &OpenWorld,
            )
            .samples
            .remove(0);
        assert_eq!(physics.mode(), sim::MovementMode::Gliding);
        physics.advance_with_context(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            context,
            &OpenWorld,
        );
        world
            .apply_ordered_event(
                protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(local_actor_spawn(
                    Vec::new(),
                ))),
                Some(2),
            )
            .unwrap();
        super::state_updates::apply_controls(&mut world, &mut physics);
        assert_eq!(physics.mode(), sim::MovementMode::Walking);
        if complete_replacement {
            physics.advance_with_context(
                simulation_clock::TICK_DURATION,
                sim::MovementInput::default(),
                context,
                &OpenWorld,
            );
        }
        physics
            .apply_correction(
                crate::movement::PhysicsAnchor {
                    network_position: anchor.position,
                    tick: anchor.tick,
                    on_ground: false,
                    velocity: Some(anchor.velocity),
                },
                crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
                None,
                &OpenWorld,
            )
            .unwrap();
        assert_eq!(
            physics.mode(),
            sim::MovementMode::Walking,
            "completed: {complete_replacement}"
        );
        let packet = reported_packet(&world, &mut physics, &ItemUseRuntime::default());
        let flags = protocol::test_support::packets::prediction_sync_flags(&packet).unwrap();
        assert!(!flag_set(&flags, ActorFlag::Gliding));
        assert_eq!(
            super::state_updates::reported_box(&mut physics, &world),
            [1.0, sim::PLAYER_WIDTH as f32, sim::PLAYER_HEIGHT as f32]
        );
    }
}

/// Applies committed movement owners and the app's retained actor facts together.
fn apply_cached_authority(
    world: &mut client_world::WorldAuthority,
    physics: &mut LocalPhysicsController,
    speed: &mut crate::movement::LocalMovementSpeedAuthority,
    facts: &mut client_world::LocalPlayerFacts,
) {
    let mut movement = crate::movement::MovementTicker::default();
    movement.reset(1, 100, [0.5, 72.0, 0.5]);
    movement.set_source(crate::movement::MovementSource::Physics);
    let mut effects = crate::movement::LocalMovementEffectTimeline::default();
    effects.begin_session(1);
    for control in world.take_committed_controls() {
        if let CommittedControlEvent::LocalMovementFlags {
            flags, replacement, ..
        } = control
        {
            if replacement {
                facts.clear_actor_movement_authority(1);
            }
            facts.apply_local_movement_flags(1, flags);
        }
        if let CommittedControlEvent::LocalMovementSpeed {
            air_drag_modifier: Some(value),
            ..
        } = control
        {
            facts.apply_air_drag_modifier(1, value);
        }
        crate::committed_control::CommittedGameplayState {
            movement: &mut movement,
            physics,
            effects: &mut effects,
            speed,
            session_generation: 1,
            dimension: world.current_dimension(),
            dimension_transfer_active: false,
        }
        .apply(control, &OpenWorld, |_| {});
    }
}

/// Creating a body preserves accepted effects and values retained after skipped updates.
#[test]
fn sync_lifetime_synthetic_body_preserves_effective_authority() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    let mut speed = crate::movement::LocalMovementSpeedAuthority::default();
    speed.begin_session(1, 0);
    let mut facts = client_world::LocalPlayerFacts::new(1);
    apply_cached_authority(&mut world, &mut physics, &mut speed, &mut facts);
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Remove(protocol::ActorRemoveEvent {
                dimension: 0,
                unique_id: 70,
            })),
            Some(2),
        )
        .unwrap();
    for (sequence, name, current) in [
        (3, Attribute::Movement, 0.2),
        (5, Attribute::AirDragModifier, 1.7),
        (6, Attribute::AirDragModifier, f32::NAN),
    ] {
        if sequence == 5 {
            world
                .apply_ordered_event(
                    protocol::WorldEvent::ActorEffect(protocol::ActorEffectEvent {
                        dimension: 0,
                        actor_runtime_id: 7,
                        action: protocol::ActorEffectAction::Add,
                        effect_id: protocol_ids::Effect::Speed.wire_value(),
                        amplifier: 0,
                        duration_ticks: 200,
                        particles: false,
                        ambient: false,
                        tick: 0,
                    }),
                    Some(4),
                )
                .unwrap();
        }
        world
            .apply_ordered_event(
                protocol::WorldEvent::Actor(protocol::ActorEvent::Attributes(
                    protocol::ActorAttributesUpdateEvent {
                        dimension: 0,
                        runtime_id: 7,
                        tick: 0,
                        attributes: [protocol::ActorAttribute {
                            name: name.wire_name().into(),
                            min: 0.0,
                            max: 10.0,
                            current,
                            default: Some(current),
                            modifiers: [].into(),
                        }]
                        .into(),
                    },
                )),
                Some(sequence),
            )
            .unwrap();
    }
    apply_cached_authority(&mut world, &mut physics, &mut speed, &mut facts);
    let accepted_speed = speed.reported_current().unwrap();
    assert!(accepted_speed as f32 > 0.2_f32);
    assert_eq!(facts.air_drag_modifier(), Some(1.7));
    world.sync_local_player_pose(&super::super::presentation_tests::feed());
    apply_cached_authority(&mut world, &mut physics, &mut speed, &mut facts);
    assert_eq!(speed.reported_current(), Some(accepted_speed));
    assert_eq!(facts.air_drag_modifier(), Some(1.7));
    send_movement_prediction_sync(
        &mut physics,
        &LocalActor(world.actor(7).unwrap().clone()),
        facts.air_drag_modifier(),
        speed.reported_current(),
        None,
        |packet| {
            assert_eq!(
                protocol::test_support::packets::prediction_sync_attributes(&packet).unwrap()[0],
                accepted_speed as f32
            );
            assert_eq!(
                protocol::test_support::packets::prediction_sync_attributes(&packet).unwrap()[8],
                1.7
            );
            false
        },
    );
}

/// Two replacements between completed ticks retain the earlier mode-ending action.
#[test]
fn sync_lifetime_multiple_replacements_keep_mode_end() {
    for complete_replacement in [true, false] {
        let mut world = local_actor_authority();
        let mut physics = due_physics();
        super::state_updates::apply_controls(&mut world, &mut physics);
        let context = crate::movement::PhysicsSampleContext {
            mode_intent: crate::movement::ModeIntent {
                elytra_ready: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let anchor = physics
            .advance_with_context(
                simulation_clock::TICK_DURATION,
                sim::MovementInput {
                    jumping: true,
                    ..Default::default()
                },
                context,
                &OpenWorld,
            )
            .samples
            .remove(0);
        physics.advance_with_context(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            context,
            &OpenWorld,
        );
        assert_eq!(physics.mode(), sim::MovementMode::Gliding);
        for (sequence, gliding) in [(2, false), (3, true)] {
            let mut spawn = local_actor_spawn(Vec::new());
            spawn.metadata = [protocol::ActorMetadata {
                key: ActorDataKey::Flags.wire_value(),
                value: ActorMetadataValue::Flags(if gliding {
                    1 << ActorFlag::Gliding.wire_value()
                } else {
                    0
                }),
            }]
            .into();
            world
                .apply_ordered_event(
                    protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(spawn)),
                    Some(sequence),
                )
                .unwrap();
            super::state_updates::apply_controls(&mut world, &mut physics);
        }
        assert_eq!(physics.mode(), sim::MovementMode::Gliding);
        if complete_replacement {
            physics.advance_with_context(
                simulation_clock::TICK_DURATION,
                sim::MovementInput::default(),
                context,
                &OpenWorld,
            );
        }
        physics
            .apply_correction(
                crate::movement::PhysicsAnchor {
                    network_position: anchor.position,
                    tick: anchor.tick,
                    on_ground: false,
                    velocity: Some(anchor.velocity),
                },
                crate::movement::PhysicsCorrectionMode::ReplayIfRetained,
                None,
                &OpenWorld,
            )
            .unwrap();
        assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    }
}

/// Unique-ID conflict cleanup ends the local lifetime even when the incoming runtime is remote.
#[test]
fn sync_lifetime_unique_id_conflict_retires_local_bounds() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    super::state_updates::write_dimensions(
        &mut world,
        &mut physics,
        2,
        &[(ActorDataKey::Width, 0.9), (ActorDataKey::Height, 0.7)],
    );
    assert_eq!(
        super::state_updates::reported_box(&mut physics, &world),
        [1.0, 0.9, 0.7]
    );
    let mut replacement = local_actor_spawn(Vec::new());
    replacement.runtime_id = 8;
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(replacement)),
            Some(3),
        )
        .unwrap();
    assert!(world.actor(7).is_none());
    world.sync_local_player_pose(&super::super::presentation_tests::feed());
    super::state_updates::apply_controls(&mut world, &mut physics);
    for _ in 0..2 {
        assert_eq!(
            super::state_updates::reported_box(&mut physics, &world),
            [1.0, sim::PLAYER_WIDTH as f32, sim::PLAYER_HEIGHT as f32]
        );
        physics.advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &OpenWorld,
        );
    }
}

/// A replay of the old actor cannot undo writes accepted after its replacement.
#[test]
fn pending_replacement_preserves_newer_pose_and_spin_through_replay() {
    let mut physics = due_physics();
    let anchor = physics
        .advance(
            simulation_clock::TICK_DURATION * 3,
            sim::MovementInput::default(),
            &OpenWorld,
        )
        .samples
        .remove(0);
    physics.replace_actor_authority(
        client_world::MovementFlagUpdate {
            gliding: Some(true),
            spinning: Some(true),
            using_item: Some(true),
            ..Default::default()
        },
        client_world::ExplicitDimensions::for_spawn(1),
    );
    physics.apply_server_movement_flags(
        0,
        client_world::MovementFlagUpdate {
            gliding: Some(false),
            swimming: Some(false),
            crawling: Some(false),
            spinning: Some(false),
            using_item: Some(true),
            ..Default::default()
        },
    );
    assert!(physics.live_using_item());
    let flags = physics.prediction_sync_flags();
    let size = physics.prediction_sync_size();
    assert_eq!(flags.gliding, Some(false));
    assert_eq!(flags.spinning, Some(false));
    physics
        .replay_retained_from(anchor.tick, &OpenWorld)
        .unwrap();
    assert_eq!(physics.prediction_sync_flags(), flags);
    assert_eq!(physics.prediction_sync_size(), size);
    assert!(physics.live_using_item());
}

/// A replacement flag follows the player game-mode stop that preceded the replacement.
#[test]
fn actor_replacement_preserves_an_unconsumed_item_use_stop() {
    let mut physics = due_physics();
    physics.stop_item_use_for_game_mode();
    physics.replace_actor_authority(
        client_world::MovementFlagUpdate {
            using_item: Some(true),
            ..Default::default()
        },
        client_world::ExplicitDimensions::for_spawn(1),
    );
    assert!(physics.take_game_mode_stop());
    assert_eq!(physics.take_server_using_item(), Some(true));
}
