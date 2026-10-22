use super::*;
use protocol::{ActorEvent, ActorMoveEvent, ActorPositionOrigin, ActorRemoveEvent};

/// Spawns an original rocket fixture with the requested silent flag.
fn rocket(silent: bool) -> ActorEvent {
    let ActorEvent::Spawn(mut spawn) = crate::actor_store::tests::spawn(7, 17) else {
        unreachable!()
    };
    spawn.kind = protocol::ActorKind::Entity {
        identifier: "minecraft:fireworks_rocket".into(),
    };
    spawn.metadata = std::sync::Arc::from([protocol::ActorMetadata {
        key: protocol_ids::ActorDataKey::Flags.wire_value(),
        value: protocol::ActorMetadataValue::Flags(
            u64::from(silent) << protocol_ids::ActorFlag::Silent.wire_value(),
        ),
    }]);
    ActorEvent::Spawn(spawn)
}

#[test]
fn rocket_launch_audio_waits_for_first_tick_and_emits_once_per_lifetime() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, rocket(false));
    assert!(store.take_synchronized_audio().is_empty());
    store.advance_interpolation_frame(0);
    assert!(store.take_synchronized_audio().is_empty());
    store.advance_interpolation_frame(4);
    let events = store.take_synchronized_audio();
    assert_eq!(
        events.len(),
        1,
        "the first rocket tick must request launch audio"
    );
    let protocol::AudioEvent::Level(level) = &events[0].event else {
        panic!("expected a level sound route")
    };
    assert_eq!(level.sound_event.as_ref(), "launch");
    assert_eq!(level.position, [1.0, 2.0, 3.0]);
    assert_eq!(level.actor_unique_id, 17);
    assert_eq!(events[0].actor_synchronization.unwrap().spawn_revision, 1);
    store.advance_interpolation_frame(4);
    assert!(store.take_synchronized_audio().is_empty());
    store.apply(1, 2, rocket(false));
    store.advance_interpolation_ticks(1);
    let events = store.take_synchronized_audio();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].actor_synchronization.unwrap().spawn_revision, 2);
}

#[test]
fn rocket_launch_audio_honors_silent_at_first_tick_and_discards_ended_lifetimes() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, rocket(false));
    store.apply(1, 2, rocket(true));
    store.advance_interpolation_ticks(1);
    assert!(store.take_synchronized_audio().is_empty());
    store.apply(1, 3, rocket(false));
    store.apply(
        1,
        4,
        ActorEvent::Remove(ActorRemoveEvent {
            dimension: 0,
            unique_id: 17,
        }),
    );
    store.advance_interpolation_ticks(1);
    assert!(store.take_synchronized_audio().is_empty());
    store.apply(1, 5, rocket(false));
    store.reset_dimension(1, 6, 1);
    store.advance_interpolation_ticks(1);
    assert!(store.take_synchronized_audio().is_empty());
}

#[test]
fn rocket_launch_audio_reads_first_tick_metadata_and_keeps_its_own_admission_credit() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, rocket(false));
    assert_eq!(store.synchronized_audio_count(), 1);
    store.apply(
        1,
        2,
        protocol::ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 7,
            metadata: std::sync::Arc::from([protocol::ActorMetadata {
                key: protocol_ids::ActorDataKey::Flags.wire_value(),
                value: protocol::ActorMetadataValue::Flags(
                    1 << protocol_ids::ActorFlag::Silent.wire_value(),
                ),
            }]),
            properties: std::sync::Arc::from([]),
            tick: 0,
        }),
    );
    store.advance_interpolation_ticks(1);
    assert_eq!(store.synchronized_audio_count(), 0);
    assert!(store.take_synchronized_audio().is_empty());
    store.apply(1, 3, rocket(false));
    for sequence in 4..16 {
        store.queue_synchronized_audio(sound(sequence, 17, [1.0, 2.0, 3.0]));
    }
    assert_eq!(store.synchronized_audio_count(), MAX_SOUNDS_PER_ACTOR + 1);
    store.advance_interpolation_ticks(1);
    let events = store.take_synchronized_audio();
    assert_eq!(events.len(), MAX_SOUNDS_PER_ACTOR + 1);
    assert_eq!(
        events.iter().filter(|event| event.client_generated).count(),
        1
    );
}

fn store() -> ActorStore {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, crate::actor_store::tests::spawn(7, 17));
    store
}

fn sound(sequence: u64, actor_unique_id: i64, fire_at_position: [f32; 3]) -> CommittedAudioEvent {
    CommittedAudioEvent {
        sequence,
        dimension: 0,
        dimension_epoch: 0,
        client_generated: false,
        actor_synchronization: None,
        event: protocol::AudioEvent::Level(protocol::LevelAudioEvent {
            sound_event: "death".into(),
            position: [2.0, 3.0, 4.0],
            data: -1,
            actor_identifier: "minecraft:ender_dragon".into(),
            is_baby: false,
            is_global: false,
            actor_unique_id,
            fire_at_position: Some(fire_at_position),
        }),
    }
}

#[test]
fn synchronized_audio_waits_one_fixed_tick_and_emits_once() {
    let mut store = store();
    store.queue_synchronized_audio(sound(2, 17, [1.0, 2.0, 3.0]));
    assert!(store.take_synchronized_audio().is_empty());
    store.advance_interpolation_ticks(0);
    assert!(store.take_synchronized_audio().is_empty());
    store.advance_interpolation_ticks(1);
    let emitted = store.take_synchronized_audio();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].sequence, 2);
    assert_eq!(emitted[0].actor_synchronization.unwrap().runtime_id, 7);
    store.advance_interpolation_ticks(10);
    assert!(store.take_synchronized_audio().is_empty());
}

fn moving_store() -> ActorStore {
    let mut store = store();
    store.apply(
        1,
        2,
        ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 7,
            position: [Some(7.0), None, None],
            position_origin: ActorPositionOrigin::Feet,
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: None,
            teleported: false,
            force_local_move: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
        }),
    );
    store.queue_synchronized_audio(sound(3, 17, [7.0, 2.0, 3.0]));
    store
}

#[test]
fn synchronized_audio_waits_for_interpolation_and_frame_batching_matches_ticks() {
    let mut per_tick = moving_store();
    let mut per_frame = moving_store();
    let ticks = crate::actor_store::ACTOR_INTERPOLATION_TICKS;
    for _ in 0..ticks {
        per_tick.advance_interpolation_ticks(1);
        assert!(per_tick.take_synchronized_audio().is_empty());
    }
    per_frame.advance_interpolation_frame(ticks);
    assert!(per_frame.take_synchronized_audio().is_empty());
    per_tick.advance_interpolation_ticks(1);
    per_frame.advance_interpolation_frame(1);
    let sounds = per_tick.take_synchronized_audio();
    assert_eq!(sounds.len(), 1);
    assert_eq!(sounds, per_frame.take_synchronized_audio());
}

#[test]
fn synchronized_audio_skips_missing_actor_and_clears_removal_replacement_and_reset() {
    let mut store = store();
    store.queue_synchronized_audio(sound(2, -1, [1.0, 2.0, 3.0]));
    assert_eq!(store.synchronized_audio.skipped, 1);
    assert_eq!(store.synchronized_audio_count(), 0);
    store.queue_synchronized_audio(sound(3, 17, [1.0, 2.0, 3.0]));
    store.apply(
        1,
        4,
        ActorEvent::Remove(ActorRemoveEvent {
            dimension: 0,
            unique_id: 17,
        }),
    );
    assert_eq!(store.synchronized_audio_count(), 0);
    store.apply(1, 5, crate::actor_store::tests::spawn(7, 17));
    store.queue_synchronized_audio(sound(6, 17, [1.0, 2.0, 3.0]));
    store.apply(1, 7, crate::actor_store::tests::spawn(7, 18));
    assert_eq!(store.synchronized_audio_count(), 0);
    store.queue_synchronized_audio(sound(8, 18, [1.0, 2.0, 3.0]));
    store.reset_dimension(1, 9, 1);
    assert_eq!(store.synchronized_audio_count(), 0);
    store.advance_interpolation_ticks(1);
    assert!(store.take_synchronized_audio().is_empty());
}

#[test]
fn synchronized_audio_keeps_the_nine_newest_requests_for_each_actor() {
    let mut store = store();
    for sequence in 2..15 {
        store.queue_synchronized_audio(sound(sequence, 17, [1.0, 2.0, 3.0]));
    }
    assert_eq!(store.synchronized_audio_count(), MAX_SOUNDS_PER_ACTOR);
    store.advance_interpolation_ticks(1);
    assert_eq!(
        store
            .take_synchronized_audio()
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        (6..15).collect::<Vec<_>>()
    );
}

#[test]
fn synchronized_audio_retains_newer_interpolation_targets_without_an_age_timeout() {
    let mut store = moving_store();
    for tick in 0..100 {
        let actor = store.actors.get_mut(&7).unwrap();
        actor.interpolation_ticks_remaining = super::super::ACTOR_INTERPOLATION_TICKS;
        actor.received_pose.position = [7.0, 2.0, 3.0];
        store.advance_synchronized_audio();
        assert!(
            store.take_synchronized_audio().is_empty(),
            "still moving at tick {tick}"
        );
    }
    store
        .actors
        .get_mut(&7)
        .unwrap()
        .interpolation_ticks_remaining = 0;
    for _ in 0..=super::super::ACTOR_INTERPOLATION_TICKS {
        store.advance_synchronized_audio();
    }
    let emitted = store.take_synchronized_audio();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].sequence, 3);
    store.actors.get_mut(&7).unwrap().position = [7.0, 2.0, 3.0];
    store.queue_synchronized_audio(sound(4, 17, [7.0, 2.0, 3.0]));
    store.advance_synchronized_audio();
    store.apply(
        1,
        5,
        ActorEvent::Remove(ActorRemoveEvent {
            dimension: 0,
            unique_id: 17,
        }),
    );
    assert!(
        store.take_synchronized_audio().is_empty(),
        "removal also discards sounds already ready for publication"
    );
}
