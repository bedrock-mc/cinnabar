use super::state_updates::{reported_box, write_dimensions};
use super::*;
use std::sync::Arc;

const WIDTH: f32 = sim::PLAYER_WIDTH as f32;
const STANDING_HEIGHT: f32 = sim::PLAYER_HEIGHT as f32;

/// Commits a live server flag word and applies the resulting movement authority.
fn write_flags(
    world: &mut client_world::WorldAuthority,
    physics: &mut LocalPhysicsController,
    sequence: u64,
    bits: u64,
) {
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Metadata(
                protocol::ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 7,
                    metadata: Arc::from([protocol::ActorMetadata {
                        key: ActorDataKey::Flags.wire_value(),
                        value: ActorMetadataValue::Flags(bits),
                    }]),
                    properties: Arc::from([]),
                    tick: 0,
                },
            )),
            Some(sequence),
        )
        .unwrap();
    let mut applied = false;
    for control in world.take_committed_controls() {
        if let client_world::CommittedControlEvent::LocalMovementFlags { tick, flags, .. } = control
        {
            physics.apply_server_movement_flags(tick, flags);
            applied = true;
        }
    }
    assert!(
        applied,
        "accepted local metadata publishes movement authority"
    );
}

/// Continuing an already accepted sneak bit does not replace an explicit component size.
#[test]
fn same_server_sneak_retains_explicit_dimensions() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    world.take_committed_controls();
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &OpenWorld,
    );
    write_dimensions(&mut world, 2, &[(ActorDataKey::Height, 0.7)]);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.7]);
    write_flags(
        &mut world,
        &mut physics,
        3,
        1 << ActorFlag::Sneaking.wire_value(),
    );
    assert_eq!(physics.prediction_sync_flags().sneaking, Some(true));
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.7]);
    let frame = physics.advance_with_context(
        simulation_clock::TICK_DURATION,
        sim::MovementInput {
            sneaking: true,
            ..Default::default()
        },
        crate::movement::PhysicsSampleContext {
            mode_intent: crate::movement::ModeIntent {
                explicit_dimensions: world.actor(7).unwrap().explicit_dimensions,
                ..Default::default()
            },
            ..Default::default()
        },
        &OpenWorld,
    );
    assert_eq!(frame.completed_ticks, 1);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.7]);
}

/// Completes one walking input with this actor's current sparse dimension carrier.
fn tick_sneak(
    world: &client_world::WorldAuthority,
    physics: &mut LocalPhysicsController,
    sneaking: bool,
) {
    let frame = physics.advance_with_context(
        simulation_clock::TICK_DURATION,
        sim::MovementInput {
            sneaking,
            ..Default::default()
        },
        crate::movement::PhysicsSampleContext {
            mode_intent: crate::movement::ModeIntent {
                explicit_dimensions: world.actor(7).unwrap().explicit_dimensions,
                ..Default::default()
            },
            ..Default::default()
        },
        &OpenWorld,
    );
    assert_eq!(frame.completed_ticks, 1);
}

/// Both matching continuation and real opposite requests compare against the accepted bit.
#[test]
fn explicit_sneak_size_follows_the_accepted_bit_in_both_directions() {
    for server in [true, false] {
        for matching in [true, false] {
            let mut world = local_actor_authority();
            let mut physics = due_physics();
            world.take_committed_controls();
            tick_sneak(&world, &mut physics, !server);
            write_dimensions(&mut world, 2, &[(ActorDataKey::Height, 0.7)]);
            assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.7]);
            write_flags(
                &mut world,
                &mut physics,
                3,
                if server {
                    1 << ActorFlag::Sneaking.wire_value()
                } else {
                    0
                },
            );
            let local = if matching { server } else { !server };
            let height = if matching {
                0.7
            } else {
                sim::MovementMode::Walking.hitbox_height(local) as f32
            };
            for _ in 0..2 {
                tick_sneak(&world, &mut physics, local);
                assert_eq!(
                    reported_box(&mut physics, &world),
                    [1.0, WIDTH, height],
                    "server={server}, local={local}"
                );
            }
        }
    }
}

/// Same-value sparse arrivals retain the other accepted axis after matching sneak input.
#[test]
fn matching_sneak_keeps_sparse_size_ownership() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    world.take_committed_controls();
    tick_sneak(&world, &mut physics, false);
    write_dimensions(
        &mut world,
        2,
        &[(ActorDataKey::Width, 0.8), (ActorDataKey::Height, 0.7)],
    );
    assert_eq!(reported_box(&mut physics, &world), [1.0, 0.8, 0.7]);
    write_flags(
        &mut world,
        &mut physics,
        3,
        1 << ActorFlag::Sneaking.wire_value(),
    );
    tick_sneak(&world, &mut physics, true);
    write_dimensions(&mut world, 4, &[(ActorDataKey::Width, 0.8)]);
    assert_eq!(reported_box(&mut physics, &world), [1.0, 0.8, 0.7]);
    tick_sneak(&world, &mut physics, false);
    assert_eq!(
        reported_box(&mut physics, &world),
        [1.0, WIDTH, STANDING_HEIGHT]
    );
    write_dimensions(
        &mut world,
        5,
        &[(ActorDataKey::Height, 0.9), (ActorDataKey::Scale, 2.0)],
    );
    assert_eq!(reported_box(&mut physics, &world), [2.0, WIDTH, 0.9]);
    write_flags(&mut world, &mut physics, 6, 0);
    tick_sneak(&world, &mut physics, false);
    assert_eq!(reported_box(&mut physics, &world), [2.0, WIDTH, 0.9]);
}

/// Observing Sneaking does not pre-consume a separate horizontal-pose transition.
#[test]
fn matching_sneak_keeps_horizontal_transitions_independent() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    world.take_committed_controls();
    tick_sneak(&world, &mut physics, false);
    write_dimensions(&mut world, 2, &[(ActorDataKey::Height, 0.7)]);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.7]);
    let sneak = 1 << ActorFlag::Sneaking.wire_value();
    write_flags(
        &mut world,
        &mut physics,
        3,
        sneak | (1 << ActorFlag::DamageNearbyMobs.wire_value()),
    );
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.7]);
    tick_sneak(&world, &mut physics, true);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, WIDTH]);
    write_dimensions(&mut world, 4, &[(ActorDataKey::Height, 0.9)]);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.9]);
    write_flags(&mut world, &mut physics, 5, sneak);
    tick_sneak(&world, &mut physics, true);
    assert_eq!(
        reported_box(&mut physics, &world),
        [
            1.0,
            WIDTH,
            sim::MovementMode::Walking.hitbox_height(true) as f32
        ]
    );
}

/// A pending newer server size and sneak write retain their ordering through correction replay.
#[test]
fn pending_sneak_and_explicit_size_survive_replay() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    world.take_committed_controls();
    let anchor = physics
        .advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &OpenWorld,
        )
        .samples
        .remove(0);
    write_dimensions(&mut world, 2, &[(ActorDataKey::Height, 0.7)]);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.7]);
    write_flags(
        &mut world,
        &mut physics,
        3,
        1 << ActorFlag::Sneaking.wire_value(),
    );
    tick_sneak(&world, &mut physics, true);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.7]);
    write_dimensions(&mut world, 4, &[(ActorDataKey::Height, 0.9)]);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.9]);
    write_flags(&mut world, &mut physics, 5, 0);
    let correction = physics
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
    assert!(matches!(
        correction.outcome,
        crate::movement::PhysicsCorrectionOutcome::Replayed {
            replayed_ticks: 1,
            ..
        }
    ));
    assert_eq!(physics.prediction_sync_flags().sneaking, Some(false));
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.9]);
    tick_sneak(&world, &mut physics, false);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.9]);
}

/// A later accepted sneak write preserves the replacement actor's explicit dimensions.
#[test]
fn replacement_size_uses_its_later_accepted_sneak() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    world.take_committed_controls();
    tick_sneak(&world, &mut physics, true);
    let mut replacement = local_actor_spawn(Vec::new());
    replacement.metadata = Arc::from([protocol::ActorMetadata {
        key: ActorDataKey::Width.wire_value(),
        value: ActorMetadataValue::Float(0.8),
    }]);
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(replacement)),
            Some(2),
        )
        .unwrap();
    assert_eq!(
        reported_box(&mut physics, &world),
        [1.0, 0.8, STANDING_HEIGHT]
    );
    write_flags(
        &mut world,
        &mut physics,
        3,
        1 << ActorFlag::Sneaking.wire_value(),
    );
    tick_sneak(&world, &mut physics, true);
    assert_eq!(
        reported_box(&mut physics, &world),
        [1.0, 0.8, STANDING_HEIGHT]
    );
    tick_sneak(&world, &mut physics, false);
    assert_eq!(
        reported_box(&mut physics, &world),
        [1.0, WIDTH, STANDING_HEIGHT]
    );
}
