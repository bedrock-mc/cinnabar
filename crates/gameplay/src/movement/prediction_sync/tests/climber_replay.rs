use super::*;

/// Applies the supplied extended metadata word through movement authority.
fn edit(physics: &mut LocalPhysicsController, stamp: u64, word: u64) {
    assert_eq!(
        physics.apply_server_movement_flags(
            stamp,
            client_world::MovementFlagUpdate {
                extended_flags: Some(word),
                ..Default::default()
            },
        ),
        None
    );
}

/// Projects the completed controller flags into prediction-sync packet words.
fn reported(physics: &LocalPhysicsController) -> [u64; 3] {
    predicted_flag_words(
        &local_actor().0.metadata,
        physics.prediction_sync_flags(),
        None,
    )
}

#[test]
fn consumed_unstamped_climber_write_survives_replay_from_101_and_102() {
    let mut physics = due_physics();
    let tick = simulation_clock::TICK_DURATION;
    let earlier = physics.advance(tick * 2, sim::MovementInput::default(), &OpenWorld);
    let mut words = [0; 3];
    set_flag(&mut words, ActorFlag::OverDescendingBlock, true);
    edit(&mut physics, 0, words[1]);
    let live = physics.advance(
        tick,
        sim::MovementInput {
            sneaking: true,
            ..Default::default()
        },
        &OpenWorld,
    );
    assert_eq!(live.samples[0].tick, 103);
    assert_eq!(live.samples[0].movement[1], -0.15_f32);
    let flags = reported(&physics);
    assert!(flag_set(&flags, ActorFlag::DescendThroughBlock));
    for anchor in &earlier.samples {
        let mut replay = physics.clone();
        let plan = replay
            .replay_retained_from(anchor.tick, &OpenWorld)
            .unwrap();
        let sample = plan.replayed_samples.last().unwrap();
        assert_eq!(
            sample.movement, live.samples[0].movement,
            "anchor={}",
            anchor.tick
        );
        assert_eq!(sample.position, live.samples[0].position);
        assert_eq!(sample.velocity, live.samples[0].velocity);
        assert_eq!(reported(&replay), flags);
    }
}

#[test]
fn future_stamped_climber_write_replays_at_its_live_consumption_tick() {
    let mut physics = due_physics();
    let tick = simulation_clock::TICK_DURATION;
    physics.advance(tick * 2, sim::MovementInput::default(), &OpenWorld);
    let word = 1 << (ActorFlag::OverDescendingBlock.wire_value() % 64);
    edit(&mut physics, 200, word);
    let live = physics.advance(
        tick,
        sim::MovementInput {
            sneaking: true,
            ..Default::default()
        },
        &OpenWorld,
    );
    assert_eq!(live.samples[0].tick, 103);
    assert_eq!(live.samples[0].movement[1], -0.15_f32);
    let flags = reported(&physics);
    for anchor in [101, 102] {
        let mut replay = physics.clone();
        let plan = replay.replay_retained_from(anchor, &OpenWorld).unwrap();
        assert_eq!(
            plan.replayed_samples.last().unwrap().movement,
            live.samples[0].movement
        );
        assert_eq!(reported(&replay), flags);
    }
}

#[test]
fn clearing_a_consumed_climber_write_uses_the_immutable_live_capture() {
    let mut physics = due_physics();
    let tick = simulation_clock::TICK_DURATION;
    physics.advance(tick * 2, sim::MovementInput::default(), &OpenWorld);
    let word = 1 << (ActorFlag::OverDescendingBlock.wire_value() % 64);
    edit(&mut physics, 0, word);
    let live = physics.advance(
        tick,
        sim::MovementInput {
            sneaking: true,
            ..Default::default()
        },
        &OpenWorld,
    );
    assert_eq!(live.samples[0].movement[1], -0.15_f32);
    physics.replay_retained_from(101, &OpenWorld).unwrap();
    assert_eq!(
        physics.apply_server_movement_flags(
            102,
            client_world::MovementFlagUpdate {
                extended_flags: Some(0),
                ..Default::default()
            },
        ),
        Some(102)
    );
    for anchor in [102, 101, 102] {
        let plan = physics.replay_retained_from(anchor, &OpenWorld).unwrap();
        assert_ne!(plan.replayed_samples.last().unwrap().movement[1], -0.15_f32);
        assert_eq!(reported(&physics)[1], 0);
    }
}

/// Missing terrain rejects a replay before it can consume any input.
pub(super) struct MissingTerrain;

impl sim::CollisionWorld for MissingTerrain {
    /// Refuses movement queries while the retained input's terrain is unavailable.
    fn collision_boxes(
        &self,
        _query: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Err(sim::WorldQueryError::CoordinateOutOfRange)
    }
}

#[test]
fn a_failed_historical_replay_leaves_the_write_for_its_first_successful_live_input() {
    let mut physics = due_physics();
    let tick = simulation_clock::TICK_DURATION;
    physics.advance(tick * 3, sim::MovementInput::default(), &OpenWorld);
    let word = 1 << (ActorFlag::OverDescendingBlock.wire_value() % 64);
    assert_eq!(
        physics.apply_server_movement_flags(
            101,
            client_world::MovementFlagUpdate {
                extended_flags: Some(word),
                ..Default::default()
            },
        ),
        Some(101)
    );
    assert!(physics.replay_retained_from(101, &MissingTerrain).is_err());
    let live = physics.advance(
        tick,
        sim::MovementInput {
            sneaking: true,
            ..Default::default()
        },
        &OpenWorld,
    );
    assert_eq!(live.samples[0].tick, 104);
    assert_eq!(live.samples[0].movement[1], -0.15_f32);
    let flags = reported(&physics);
    for anchor in [101, 102, 103] {
        let mut replay = physics.clone();
        let plan = replay.replay_retained_from(anchor, &OpenWorld).unwrap();
        assert_eq!(
            plan.replayed_samples.last().unwrap().movement,
            live.samples[0].movement
        );
        assert_eq!(reported(&replay), flags);
    }
}

#[test]
fn a_newer_historical_write_supersedes_an_unconsumed_write_before_replay() {
    let mut physics = due_physics();
    let tick = simulation_clock::TICK_DURATION;
    let input = sim::MovementInput {
        sneaking: true,
        ..Default::default()
    };
    physics.advance(tick * 3, input, &OpenWorld);
    let word = 1 << (ActorFlag::OverDescendingBlock.wire_value() % 64);
    for stamp in [102, 101] {
        assert_eq!(
            physics.apply_server_movement_flags(
                stamp,
                client_world::MovementFlagUpdate {
                    extended_flags: Some(word),
                    ..Default::default()
                }
            ),
            Some(stamp)
        );
        assert!(
            physics
                .replay_retained_from(stamp, &MissingTerrain)
                .is_err()
        );
    }
    let mut reference = due_physics();
    reference.advance(tick, input, &OpenWorld);
    edit(&mut reference, 0, word);
    let expected = reference.advance(tick * 2, input, &OpenWorld);
    for _ in 0..2 {
        let replay = physics.replay_retained_from(101, &OpenWorld).unwrap();
        for (actual, expected) in replay.replayed_samples.iter().zip(&expected.samples) {
            assert_eq!(actual.movement, expected.movement, "tick={}", actual.tick);
            assert_eq!(actual.position, expected.position);
        }
        assert_eq!(reported(&physics), reported(&reference));
    }
}

#[test]
fn consumed_climber_edits_match_live_processing_from_every_replay_anchor() {
    let flags = [
        ActorFlag::InScaffolding,
        ActorFlag::OverScaffolding,
        ActorFlag::DescendThroughBlock,
        ActorFlag::InAscendingBlock,
        ActorFlag::OverDescendingBlock,
    ];
    let mut mask = [0; 3];
    for flag in flags {
        set_flag(&mut mask, flag, true);
    }
    assert_eq!(mask[1], ClimberFlags::mask());
    let variants = 1 << flags.len();
    for controls in 0..variants {
        for edit_pattern in 0..variants {
            for stamped in [false, true] {
                let mut physics = due_physics();
                let tick = simulation_clock::TICK_DURATION;
                let mut samples = physics
                    .advance(tick, sim::MovementInput::default(), &OpenWorld)
                    .samples;
                for step in 0..flags.len() {
                    let stamp = if stamped {
                        physics.state().unwrap().tick
                    } else {
                        0
                    };
                    let mut words = [0; 3];
                    for (bit, flag) in flags.iter().enumerate() {
                        set_flag(
                            &mut words,
                            *flag,
                            edit_pattern & (1 << ((bit + step) % flags.len())) != 0,
                        );
                    }
                    // Accept a set and a clear at the same boundary before the final word.
                    edit(&mut physics, stamp, mask[1]);
                    edit(&mut physics, stamp, 0);
                    edit(&mut physics, stamp, words[1]);
                    samples.extend(
                        physics
                            .advance(
                                tick,
                                sim::MovementInput {
                                    sneaking: controls & (1 << step) != 0,
                                    ..Default::default()
                                },
                                &OpenWorld,
                            )
                            .samples,
                    );
                }
                let expected_flags = reported(&physics);
                for anchor in &samples {
                    let mut replay = physics.clone();
                    for _ in 0..2 {
                        let plan = replay
                            .replay_retained_from(anchor.tick, &OpenWorld)
                            .unwrap();
                        let expected: Vec<_> = samples
                            .iter()
                            .filter(|sample| sample.tick > anchor.tick)
                            .collect();
                        assert_eq!(plan.replayed_samples.len(), expected.len());
                        for (actual, expected) in plan.replayed_samples.iter().zip(expected) {
                            assert_eq!(
                                actual.movement, expected.movement,
                                "controls={controls} edits={edit_pattern} stamped={stamped} anchor={} tick={}",
                                anchor.tick, actual.tick
                            );
                            assert_eq!(actual.position, expected.position);
                            assert_eq!(actual.velocity, expected.velocity);
                        }
                        assert_eq!(reported(&replay), expected_flags);
                    }
                }
            }
        }
    }
}
