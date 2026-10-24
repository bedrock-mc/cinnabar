use super::state_updates::{apply_controls, reported_box, write_dimensions};
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
    apply_controls(world, physics);
}

/// Continuing an already accepted sneak bit does not replace an explicit component size.
#[test]
fn same_server_sneak_retains_explicit_dimensions() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    apply_controls(&mut world, &mut physics);
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &OpenWorld,
    );
    write_dimensions(&mut world, &mut physics, 2, &[(ActorDataKey::Height, 0.7)]);
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
        crate::movement::PhysicsSampleContext::default(),
        &OpenWorld,
    );
    assert_eq!(frame.completed_ticks, 1);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.7]);
}

/// Completes one walking input after the committed actor updates.
fn tick_sneak(physics: &mut LocalPhysicsController, sneaking: bool) {
    let frame = physics.advance_with_context(
        simulation_clock::TICK_DURATION,
        sim::MovementInput {
            sneaking,
            ..Default::default()
        },
        crate::movement::PhysicsSampleContext::default(),
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
            apply_controls(&mut world, &mut physics);
            tick_sneak(&mut physics, !server);
            write_dimensions(&mut world, &mut physics, 2, &[(ActorDataKey::Height, 0.7)]);
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
                sim::MovementMode::Walking.hitbox_height(local, sim::SneakProfile::Current) as f32
            };
            for _ in 0..2 {
                tick_sneak(&mut physics, local);
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
    apply_controls(&mut world, &mut physics);
    tick_sneak(&mut physics, false);
    write_dimensions(
        &mut world,
        &mut physics,
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
    tick_sneak(&mut physics, true);
    write_dimensions(&mut world, &mut physics, 4, &[(ActorDataKey::Width, 0.8)]);
    assert_eq!(reported_box(&mut physics, &world), [1.0, 0.8, 0.7]);
    tick_sneak(&mut physics, false);
    assert_eq!(
        reported_box(&mut physics, &world),
        [1.0, WIDTH, STANDING_HEIGHT]
    );
    write_dimensions(
        &mut world,
        &mut physics,
        5,
        &[(ActorDataKey::Height, 0.9), (ActorDataKey::Scale, 2.0)],
    );
    assert_eq!(reported_box(&mut physics, &world), [2.0, WIDTH, 0.9]);
    write_flags(&mut world, &mut physics, 6, 0);
    tick_sneak(&mut physics, false);
    assert_eq!(reported_box(&mut physics, &world), [2.0, WIDTH, 0.9]);
}

/// Observing Sneaking does not pre-consume a separate horizontal-pose transition.
#[test]
fn matching_sneak_keeps_horizontal_transitions_independent() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    apply_controls(&mut world, &mut physics);
    tick_sneak(&mut physics, false);
    write_dimensions(&mut world, &mut physics, 2, &[(ActorDataKey::Height, 0.7)]);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.7]);
    let sneak = 1 << ActorFlag::Sneaking.wire_value();
    write_flags(
        &mut world,
        &mut physics,
        3,
        sneak | (1 << ActorFlag::DamageNearbyMobs.wire_value()),
    );
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.7]);
    tick_sneak(&mut physics, true);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, WIDTH]);
    write_dimensions(&mut world, &mut physics, 4, &[(ActorDataKey::Height, 0.9)]);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.9]);
    write_flags(&mut world, &mut physics, 5, sneak);
    tick_sneak(&mut physics, true);
    assert_eq!(
        reported_box(&mut physics, &world),
        [
            1.0,
            WIDTH,
            sim::MovementMode::Walking.hitbox_height(true, sim::SneakProfile::Current) as f32
        ]
    );
}

/// A pending newer server size and sneak write retain their ordering through correction replay.
#[test]
fn pending_sneak_and_explicit_size_survive_replay() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    apply_controls(&mut world, &mut physics);
    let anchor = physics
        .advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &OpenWorld,
        )
        .samples
        .remove(0);
    write_dimensions(&mut world, &mut physics, 2, &[(ActorDataKey::Height, 0.7)]);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.7]);
    write_flags(
        &mut world,
        &mut physics,
        3,
        1 << ActorFlag::Sneaking.wire_value(),
    );
    tick_sneak(&mut physics, true);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.7]);
    write_dimensions(&mut world, &mut physics, 4, &[(ActorDataKey::Height, 0.9)]);
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
    tick_sneak(&mut physics, false);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.9]);
}

/// Replaces the local actor with a sparse width request through world authority.
fn replace_with_width(
    world: &mut client_world::WorldAuthority,
    physics: &mut LocalPhysicsController,
    sequence: u64,
) {
    let mut replacement = local_actor_spawn(Vec::new());
    replacement.metadata = Arc::from([protocol::ActorMetadata {
        key: ActorDataKey::Width.wire_value(),
        value: ActorMetadataValue::Float(0.8),
    }]);
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(replacement)),
            Some(sequence),
        )
        .unwrap();
    apply_controls(world, physics);
}

/// A later accepted sneak write preserves the replacement actor's explicit dimensions.
#[test]
fn delayed_replacement_dimensions_keep_the_accepted_sneak() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    apply_controls(&mut world, &mut physics);
    tick_sneak(&mut physics, true);
    replace_with_width(&mut world, &mut physics, 2);
    write_flags(
        &mut world,
        &mut physics,
        3,
        1 << ActorFlag::Sneaking.wire_value(),
    );
    tick_sneak(&mut physics, true);
    assert_eq!(
        reported_box(&mut physics, &world),
        [1.0, 0.8, STANDING_HEIGHT]
    );
    tick_sneak(&mut physics, false);
    assert_eq!(
        reported_box(&mut physics, &world),
        [1.0, WIDTH, STANDING_HEIGHT]
    );
}

/// Dimension observations leave accepted pose ownership independent of sender ordering.
#[test]
fn replacement_size_uses_its_later_accepted_sneak() {
    for old_explicit in [false, true] {
        for server_sneak in [false, true] {
            for (observe_before_flags, observe_after_flags) in
                [(false, false), (true, false), (false, true)]
            {
                let mut world = local_actor_authority();
                let mut physics = due_physics();
                apply_controls(&mut world, &mut physics);
                tick_sneak(&mut physics, !server_sneak);
                if old_explicit {
                    write_dimensions(
                        &mut world,
                        &mut physics,
                        2,
                        &[(ActorDataKey::Width, 0.9), (ActorDataKey::Height, 0.7)],
                    );
                    assert_eq!(reported_box(&mut physics, &world), [1.0, 0.9, 0.7]);
                }
                replace_with_width(&mut world, &mut physics, 3);
                let replacement = [1.0, 0.8, STANDING_HEIGHT];
                if observe_before_flags {
                    assert_eq!(reported_box(&mut physics, &world), replacement);
                }
                write_flags(
                    &mut world,
                    &mut physics,
                    4,
                    if server_sneak {
                        1 << ActorFlag::Sneaking.wire_value()
                    } else {
                        0
                    },
                );
                if observe_after_flags {
                    assert_eq!(reported_box(&mut physics, &world), replacement);
                }
                tick_sneak(&mut physics, server_sneak);
                assert_eq!(reported_box(&mut physics, &world), replacement);
                write_dimensions(&mut world, &mut physics, 5, &[(ActorDataKey::Height, 0.9)]);
                tick_sneak(&mut physics, server_sneak);
                assert_eq!(reported_box(&mut physics, &world), [1.0, 0.8, 0.9]);
                tick_sneak(&mut physics, !server_sneak);
                assert_eq!(
                    reported_box(&mut physics, &world),
                    [
                        1.0,
                        WIDTH,
                        sim::MovementMode::Walking
                            .hitbox_height(!server_sneak, sim::SneakProfile::Current)
                            as f32
                    ]
                );
            }
        }
    }
}

/// Replacement resets the old horizontal pose before a new spin write resizes it.
#[test]
fn replacement_resets_horizontal_pose_before_new_server_writes() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    apply_controls(&mut world, &mut physics);
    tick_sneak(&mut physics, true);
    let sneak = 1 << ActorFlag::Sneaking.wire_value();
    let spin = 1 << ActorFlag::DamageNearbyMobs.wire_value();
    write_flags(&mut world, &mut physics, 2, sneak | spin);
    tick_sneak(&mut physics, true);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, WIDTH]);
    replace_with_width(&mut world, &mut physics, 3);
    assert_eq!(
        reported_box(&mut physics, &world),
        [1.0, 0.8, STANDING_HEIGHT]
    );
    tick_sneak(&mut physics, false);
    assert_eq!(
        reported_box(&mut physics, &world),
        [1.0, 0.8, STANDING_HEIGHT]
    );
    write_flags(&mut world, &mut physics, 4, sneak | spin);
    tick_sneak(&mut physics, true);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, WIDTH]);
    write_flags(&mut world, &mut physics, 5, sneak);
    tick_sneak(&mut physics, true);
    assert_eq!(
        reported_box(&mut physics, &world),
        [
            1.0,
            WIDTH,
            sim::MovementMode::Walking.hitbox_height(true, sim::SneakProfile::Current) as f32
        ]
    );
}

/// Pending and replayed replacements keep accepted pose comparisons and reject older carriers.
#[test]
fn replacement_sneak_and_dimensions_survive_replay() {
    for (observe_dimensions, complete_replacement) in [(false, false), (true, false), (false, true)]
    {
        let mut world = local_actor_authority();
        let mut physics = due_physics();
        apply_controls(&mut world, &mut physics);
        let anchor = physics
            .advance(
                simulation_clock::TICK_DURATION,
                sim::MovementInput {
                    sneaking: true,
                    ..Default::default()
                },
                &OpenWorld,
            )
            .samples
            .remove(0);
        tick_sneak(&mut physics, true);
        let old_actor = LocalActor(world.actor(7).unwrap().clone());
        replace_with_width(&mut world, &mut physics, 2);
        write_flags(
            &mut world,
            &mut physics,
            3,
            1 << ActorFlag::Sneaking.wire_value(),
        );
        if observe_dimensions {
            assert_eq!(
                reported_box(&mut physics, &world),
                [1.0, 0.8, STANDING_HEIGHT]
            );
        }
        if complete_replacement {
            tick_sneak(&mut physics, true);
        }
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
            crate::movement::PhysicsCorrectionOutcome::Replayed { replayed_ticks, .. }
                if replayed_ticks == 1 + usize::from(complete_replacement)
        ));
        tick_sneak(&mut physics, true);
        let mut old_box = None;
        send_movement_prediction_sync(&mut physics, &old_actor, None, None, None, |packet| {
            old_box = protocol::test_support::packets::prediction_sync_bounding_box(&packet);
            false
        });
        assert_eq!(old_box, Some([1.0, 0.8, STANDING_HEIGHT]));
        assert_eq!(
            reported_box(&mut physics, &world),
            [1.0, 0.8, STANDING_HEIGHT]
        );
        tick_sneak(&mut physics, false);
        assert_eq!(
            reported_box(&mut physics, &world),
            [1.0, WIDTH, STANDING_HEIGHT]
        );
    }
}

/// Replacement and matching input retain explicit width under the level's legacy profile.
#[test]
fn legacy_replacement_preserves_explicit_sneak_and_the_level_profile() {
    let mut world = local_actor_authority();
    let mut physics = due_physics();
    physics.set_base_game_version(protocol::BaseGameVersion::parse("1.19.80"));
    world.take_committed_controls();
    tick_sneak(&mut physics, true);
    write_dimensions(&mut world, &mut physics, 2, &[(ActorDataKey::Height, 0.7)]);
    write_flags(
        &mut world,
        &mut physics,
        3,
        1 << ActorFlag::Sneaking.wire_value(),
    );
    tick_sneak(&mut physics, true);
    assert_eq!(reported_box(&mut physics, &world), [1.0, WIDTH, 0.7]);
    replace_with_width(&mut world, &mut physics, 4);
    write_flags(
        &mut world,
        &mut physics,
        5,
        1 << ActorFlag::Sneaking.wire_value(),
    );
    tick_sneak(&mut physics, true);
    assert_eq!(physics.sneak_profile(), sim::SneakProfile::Legacy);
    assert_eq!(
        reported_box(&mut physics, &world),
        [1.0, 0.8, STANDING_HEIGHT]
    );
    tick_sneak(&mut physics, false);
    assert_eq!(
        reported_box(&mut physics, &world),
        [1.0, WIDTH, STANDING_HEIGHT]
    );
}
