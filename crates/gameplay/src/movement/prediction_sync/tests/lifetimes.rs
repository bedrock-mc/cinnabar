use super::*;
use crate::item_use::ItemUseRuntime;
use client_world::CommittedControlEvent;
use std::sync::Arc;

/// Applies the committed item-use flags consumed by the app before physics.
fn apply_use_controls(world: &mut client_world::WorldAuthority, runtime: &mut ItemUseRuntime) {
    for control in world.take_committed_controls() {
        if let CommittedControlEvent::LocalMovementFlags { flags, .. } = control {
            runtime.adopt_server_using_item(flags.using_item);
        }
    }
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
        apply_use_controls(&mut world, &mut runtime);
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
        apply_use_controls(&mut world, &mut runtime);
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
        apply_use_controls(&mut world, &mut runtime);
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
    let synthetic = world.actor(7).unwrap().explicit_dimensions;
    let anchor = physics
        .advance_with_context(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            crate::movement::PhysicsSampleContext {
                mode_intent: crate::movement::ModeIntent {
                    explicit_dimensions: synthetic,
                    ..Default::default()
                },
                ..Default::default()
            },
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
        crate::movement::PhysicsSampleContext {
            mode_intent: crate::movement::ModeIntent {
                explicit_dimensions: world.actor(7).unwrap().explicit_dimensions,
                ..Default::default()
            },
            ..Default::default()
        },
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
        crate::movement::PhysicsSampleContext {
            mode_intent: crate::movement::ModeIntent {
                explicit_dimensions: synthetic,
                ..Default::default()
            },
            ..Default::default()
        },
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
    apply_use_controls(&mut world, &mut runtime);
    assert_use(&world, &mut physics, &runtime, true);
}
