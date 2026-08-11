use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use client_world::{
    COMMITTED_AUDIO_CAPACITY, MAX_ADMITTED_WORLD_EVENTS, WorldStream, WorldStreamError,
};
use protocol::{
    AudioEvent, BlockUpdateEvent, PlayAudioEvent, WeatherChannel, WeatherUpdateEvent,
    WorldBootstrap, WorldEvent,
};

fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0, 64.0, 0.0],
        world_spawn_position: [0, 64, 0],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    })
}

fn audio(name: &str) -> WorldEvent {
    WorldEvent::Audio(AudioEvent::Play(PlayAudioEvent {
        name: Arc::from(name),
        position: [1, -2, 3],
        volume: -0.5,
        pitch: 4.0,
        loop_count: -9,
        server_sound_handle: None,
    }))
}

#[test]
fn audio_fifo_survives_interleaved_weather_and_block_updates() {
    let deadline = Instant::now() + Duration::from_secs(10);
    for attempt in 0..32 {
        let mut stream = stream();
        stream.submit(1, audio("first")).unwrap();
        stream
            .submit(
                2,
                WorldEvent::Weather(WeatherUpdateEvent {
                    channel: WeatherChannel::Rain,
                    level: 1.0,
                }),
            )
            .unwrap();
        stream
            .submit(
                3,
                WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                    dimension: 0,
                    position: [0, 64, 0],
                    layer: 0,
                    network_id: 1,
                }]),
            )
            .unwrap();
        stream.submit(4, audio("second")).unwrap();

        while stream.committed_sequence() != 4 {
            stream.poll([0.0, 64.0, 0.0], 0);
            assert!(
                Instant::now() < deadline,
                "attempt {attempt} timed out after committing sequence {}; stats: {:?}",
                stream.committed_sequence(),
                stream.stats()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let committed = stream.take_committed_audio();
        assert_eq!(committed.len(), 2);
        assert_eq!((committed[0].sequence, committed[1].sequence), (1, 4));
        let AudioEvent::Play(first) = &committed[0].event else {
            panic!("expected play event")
        };
        let AudioEvent::Play(second) = &committed[1].event else {
            panic!("expected play event")
        };
        assert_eq!((&*first.name, &*second.name), ("first", "second"));
        assert_eq!(stream.take_committed_controls().len(), 1);
    }
}

#[test]
fn bounded_audio_commit_queue_applies_backpressure_and_recovers_after_drain() {
    assert_eq!(COMMITTED_AUDIO_CAPACITY, MAX_ADMITTED_WORLD_EVENTS);
    let mut stream = stream();
    for sequence in 1..=MAX_ADMITTED_WORLD_EVENTS as u64 {
        stream.submit(sequence, audio("queued")).unwrap();
    }
    assert_eq!(
        stream.stats().committed_audio_events,
        COMMITTED_AUDIO_CAPACITY
    );
    assert!(matches!(
        stream.submit(MAX_ADMITTED_WORLD_EVENTS as u64 + 1, audio("blocked")),
        Err(WorldStreamError::AdmissionFull { .. })
    ));

    assert_eq!(
        stream.take_committed_audio().len(),
        COMMITTED_AUDIO_CAPACITY
    );
    assert_eq!(stream.stats().committed_audio_events, 0);
    stream
        .submit(MAX_ADMITTED_WORLD_EVENTS as u64 + 1, audio("recovered"))
        .unwrap();
    let recovered = stream.take_committed_audio();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].sequence, MAX_ADMITTED_WORLD_EVENTS as u64 + 1);
}

#[test]
fn audio_queue_is_session_scoped_and_draining_is_nonaccumulating() {
    let mut old = stream();
    old.submit(1, audio("old session")).unwrap();
    assert_eq!(old.take_committed_audio().len(), 1);
    assert!(old.take_committed_audio().is_empty());

    let mut replacement = stream();
    assert!(replacement.take_committed_audio().is_empty());
    replacement.submit(1, audio("new session")).unwrap();
    assert_eq!(replacement.take_committed_audio()[0].sequence, 1);
    assert!(replacement.take_committed_audio().is_empty());
}
