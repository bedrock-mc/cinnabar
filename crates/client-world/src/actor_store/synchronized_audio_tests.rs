use super::*;
use protocol::{ActorEvent, ActorMoveEvent, ActorPositionOrigin, ActorRemoveEvent};

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
