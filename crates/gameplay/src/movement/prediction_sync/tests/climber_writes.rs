use super::*;

/// Applies a real second-word metadata update through the movement authority boundary.
fn accept(
    physics: &mut LocalPhysicsController,
    actor: &mut LocalActor,
    tick: u64,
    word: u64,
) -> Option<u64> {
    let metadata = protocol::ActorMetadata {
        key: ActorDataKey::FlagsTwo.wire_value(),
        value: ActorMetadataValue::FlagsExtended(word),
    };
    actor
        .0
        .metadata
        .insert(metadata.key, metadata.value.clone());
    let flags = client_world::MovementFlagUpdate::from_metadata(&[metadata]).unwrap();
    physics.apply_server_movement_flags(tick, flags)
}

/// Reads the packet projection without consuming the correction countdown.
fn reported(physics: &LocalPhysicsController, actor: &LocalActor) -> [u64; 3] {
    predicted_flag_words(&actor.0.metadata, physics.prediction_sync_flags(), None)
}

#[test]
fn climber_server_writes_survive_until_the_next_completed_action_and_scan() {
    let mut words = [0; 3];
    for flag in [
        ActorFlag::InScaffolding,
        ActorFlag::OverScaffolding,
        ActorFlag::DescendThroughBlock,
        ActorFlag::InAscendingBlock,
        ActorFlag::OverDescendingBlock,
    ] {
        set_flag(&mut words, flag, true);
    }
    for stamp in [0, 101, 102] {
        let mut physics = due_physics();
        let mut actor = local_actor();
        let tick = simulation_clock::TICK_DURATION;
        physics.advance(tick, sim::MovementInput::default(), &OpenWorld);
        assert_eq!(accept(&mut physics, &mut actor, stamp, words[1]), None);
        assert_eq!(reported(&physics, &actor)[1], words[1], "stamp={stamp}");
        assert_eq!(
            physics
                .advance(
                    std::time::Duration::ZERO,
                    sim::MovementInput::default(),
                    &OpenWorld
                )
                .completed_ticks,
            0
        );
        assert_eq!(reported(&physics, &actor)[1], words[1]);
        let frame = physics.advance(
            tick,
            sim::MovementInput {
                sneaking: true,
                ..Default::default()
            },
            &OpenWorld,
        );
        assert_eq!(frame.completed_ticks, 1);
        assert_eq!(frame.samples[0].movement[1], -0.15_f32);
        let mut expected = [0; 3];
        set_flag(&mut expected, ActorFlag::DescendThroughBlock, true);
        assert_eq!(reported(&physics, &actor)[1], expected[1]);
        physics.advance(tick, sim::MovementInput::default(), &OpenWorld);
        accept(&mut physics, &mut actor, 0, 0);
        assert_eq!(reported(&physics, &actor)[1], 0);
    }
}

#[test]
fn climber_server_writes_replay_before_the_following_frames_input() {
    let mut physics = due_physics();
    let mut actor = local_actor();
    let input = sim::MovementInput {
        sneaking: true,
        ..Default::default()
    };
    let frame = physics.advance(std::time::Duration::from_millis(150), input, &OpenWorld);
    let anchor = frame.samples[0].clone();
    let mut words = [0; 3];
    set_flag(&mut words, ActorFlag::OverDescendingBlock, true);
    assert_eq!(
        accept(&mut physics, &mut actor, anchor.tick, words[1]),
        Some(anchor.tick)
    );
    assert!(flag_set(
        &reported(&physics, &actor),
        ActorFlag::OverDescendingBlock
    ));
    let plan = physics
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
    assert_eq!(plan.replayed_samples[0].movement[1], -0.15_f32);
    assert!(!flag_set(
        &reported(&physics, &actor),
        ActorFlag::OverDescendingBlock
    ));
}

#[test]
fn climber_server_writes_preserve_bits_unchanged_from_the_selected_capture() {
    let mut physics = due_physics();
    let mut actor = local_actor();
    let mut contact = [0; 3];
    set_flag(&mut contact, ActorFlag::OverDescendingBlock, true);
    accept(&mut physics, &mut actor, 0, contact[1]);
    let first = physics
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
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &OpenWorld,
    );
    let mut captured = [0; 3];
    set_flag(&mut captured, ActorFlag::DescendThroughBlock, true);
    assert_eq!(
        accept(&mut physics, &mut actor, first.tick, captured[1]),
        None
    );
    assert_eq!(reported(&physics, &actor)[1], 0);
    set_flag(&mut captured, ActorFlag::InAscendingBlock, true);
    assert_eq!(
        accept(&mut physics, &mut actor, first.tick, captured[1]),
        Some(first.tick)
    );
    assert!(flag_set(
        &reported(&physics, &actor),
        ActorFlag::InAscendingBlock
    ));
    assert!(!flag_set(
        &reported(&physics, &actor),
        ActorFlag::DescendThroughBlock
    ));
}

#[test]
fn climber_server_writes_with_a_future_stamp_attach_to_the_current_capture() {
    let mut physics = due_physics();
    let mut actor = local_actor();
    let frame = physics.advance(
        std::time::Duration::from_millis(150),
        sim::MovementInput {
            sneaking: true,
            ..Default::default()
        },
        &OpenWorld,
    );
    let anchor = frame.samples[0].clone();
    let mut words = [0; 3];
    set_flag(&mut words, ActorFlag::OverDescendingBlock, true);
    assert_eq!(
        accept(&mut physics, &mut actor, anchor.tick + 100, words[1]),
        None
    );
    let plan = physics
        .replay_retained_from(anchor.tick, &OpenWorld)
        .unwrap();
    assert_ne!(plan.replayed_samples[0].movement[1], -0.15_f32);
    assert_eq!(plan.replayed_samples[1].movement[1], -0.15_f32);
    assert_eq!(
        reported(&physics, &actor)[1],
        1 << (ActorFlag::DescendThroughBlock.wire_value() % 64)
    );
}

#[test]
fn climber_server_writes_survive_a_spatial_correction_without_replayed_ticks() {
    let mut physics = due_physics();
    let mut actor = local_actor();
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &OpenWorld,
    );
    let current = physics.state().unwrap().tick;
    let mut words = [0; 3];
    set_flag(&mut words, ActorFlag::OverDescendingBlock, true);
    assert_eq!(accept(&mut physics, &mut actor, 0, words[1]), None);
    let plan = physics.replay_retained_from(current, &OpenWorld).unwrap();
    assert!(plan.replayed_samples.is_empty());
    assert!(flag_set(
        &reported(&physics, &actor),
        ActorFlag::OverDescendingBlock
    ));
    let next = physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput {
            sneaking: true,
            ..Default::default()
        },
        &OpenWorld,
    );
    assert_eq!(next.samples[0].movement[1], -0.15_f32);
}

/// An echo compares with the following entry capture, including an intervening live write.
#[test]
fn climber_echo_compares_with_following_entry_after_an_unstamped_write() {
    let mut physics = due_physics();
    let mut actor = local_actor();
    let tick = simulation_clock::TICK_DURATION;
    physics.advance(tick, sim::MovementInput::default(), &OpenWorld);
    let stamp = physics.state().unwrap().tick;
    let mut word = [0; 3];
    set_flag(&mut word, ActorFlag::OverDescendingBlock, true);
    assert_eq!(accept(&mut physics, &mut actor, 0, word[1]), None);
    physics.advance(tick, sim::MovementInput::default(), &OpenWorld);
    assert!(!flag_set(
        &reported(&physics, &actor),
        ActorFlag::OverDescendingBlock
    ));
    assert_eq!(accept(&mut physics, &mut actor, stamp, word[1]), None);
    assert!(!flag_set(
        &reported(&physics, &actor),
        ActorFlag::OverDescendingBlock
    ));
}

#[test]
fn climber_replay_keeps_the_first_entry_and_refreshes_later_entries() {
    let mut physics = due_physics();
    let mut actor = local_actor();
    let tick = simulation_clock::TICK_DURATION;
    physics.advance(tick, sim::MovementInput::default(), &OpenWorld);
    let stamp = physics.state().unwrap().tick;
    let mut word = [0; 3];
    set_flag(&mut word, ActorFlag::OverDescendingBlock, true);
    accept(&mut physics, &mut actor, 0, word[1]);
    let input = sim::MovementInput {
        sneaking: true,
        ..Default::default()
    };
    physics.advance(tick, input, &OpenWorld);
    physics.advance(tick, input, &OpenWorld);
    let plan = physics.replay_retained_from(stamp, &OpenWorld).unwrap();
    assert_eq!(plan.replayed_samples[0].movement[1], -0.15_f32);
    assert_ne!(plan.replayed_samples[1].movement[1], -0.15_f32);
}

/// Replaying a correction keeps the original capture for later same-stamp comparisons.
#[test]
fn climber_replayed_correction_preserves_its_comparison_capture() {
    let mut physics = due_physics();
    let mut actor = local_actor();
    let frame = physics.advance(
        std::time::Duration::from_millis(150),
        sim::MovementInput {
            sneaking: true,
            ..Default::default()
        },
        &OpenWorld,
    );
    let stamp = frame.samples[0].tick;
    let mut word = [0; 3];
    set_flag(&mut word, ActorFlag::OverDescendingBlock, true);
    assert_eq!(
        accept(&mut physics, &mut actor, stamp, word[1]),
        Some(stamp)
    );
    let plan = physics.replay_retained_from(stamp, &OpenWorld).unwrap();
    assert_eq!(plan.replayed_samples[0].movement[1], -0.15_f32);
    assert_eq!(accept(&mut physics, &mut actor, stamp, 0), None);
    let plan = physics.replay_retained_from(stamp, &OpenWorld).unwrap();
    assert_eq!(plan.replayed_samples[0].movement[1], -0.15_f32);
}
