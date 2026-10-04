use super::test_support::sample;
use super::*;

fn state() -> NamedAudio {
    let mut value = NamedAudio::new(Some(Arc::new(sample())));
    value.bind(Some(AudioOwner {
        session: 0,
        stream: 1,
        dimension: 0,
        epoch: 0,
    }));
    value
}
fn play(sequence: u64) -> SequencedAudioEvent {
    SequencedAudioEvent {
        origin_stream_session_id: 1,
        sequence,
        dimension: 0,
        dimension_epoch: 0,
        event: protocol::AudioEvent::Play(protocol::PlayAudioEvent {
            name: Arc::from("ambient.underwater.loop"),
            position: [0; 3],
            volume: 1.0,
            pitch: 1.0,
            loop_count: -1,
            server_sound_handle: None,
        }),
    }
}
fn stop(sequence: u64, all: bool) -> SequencedAudioEvent {
    SequencedAudioEvent {
        origin_stream_session_id: 1,
        sequence,
        dimension: 0,
        dimension_epoch: 0,
        event: protocol::AudioEvent::Stop(protocol::StopAudioEvent {
            name: Arc::from("ambient.underwater.loop"),
            stop_all_sounds: all,
            stop_music_legacy: false,
        }),
    }
}
#[test]
fn same_batch_stop_dominates_pending_and_stop_then_play_is_fresh() {
    let mut value = state();
    value.event(&play(1), Some([0.0; 3]), true);
    value.event(&stop(2, false), None, true);
    let mut submitted = 0;
    value.flush(|_| {
        submitted += 1;
        true
    });
    assert_eq!(submitted, 0);
    assert_eq!(value.pool.occupied(), 0);
    value.event(&stop(3, true), None, true);
    value.event(&play(4), Some([0.0; 3]), true);
    value.flush(|_| {
        submitted += 1;
        true
    });
    assert_eq!(submitted, 1);
}
#[test]
fn cancelled_submitted_sources_survive_session_reset_and_exhaust_capacity() {
    let mut value = state();
    let mut stalled = Vec::new();
    for sequence in 1..=16 {
        value.event(&play(sequence), Some([0.0; 3]), true);
    }
    value.flush(|source| {
        stalled.push(source);
        true
    });
    value.event(&stop(17, true), None, true);
    assert_eq!(value.pool.occupied(), 16);
    value.bind(None);
    value.bind(Some(AudioOwner {
        session: 1,
        stream: 1,
        dimension: 0,
        epoch: 0,
    }));
    value.event(&play(18), Some([0.0; 3]), true);
    assert_eq!(value.stats.capacity, 1);
    for source in &mut stalled {
        assert!(source.next().is_none());
    }
    assert_eq!(value.pool.occupied(), 16);
    drop(stalled);
    value.collect_retired();
    assert_eq!(value.pool.occupied(), 0);
}
#[test]
fn wrong_origin_epoch_duplicate_dynamics_camera_and_device_fail_closed() {
    let mut value = state();
    let mut event = play(1);
    event.dimension_epoch = 2;
    value.event(&event, Some([0.0; 3]), true);
    event = play(2);
    event.origin_stream_session_id = 2;
    value.event(&event, Some([0.0; 3]), true);
    event = play(3);
    if let protocol::AudioEvent::Play(play) = &mut event.event {
        play.loop_count = 0;
    }
    value.event(&event, Some([0.0; 3]), true);
    value.event(&play(4), None, true);
    value.event(&play(5), Some([0.0; 3]), false);
    value.event(&play(5), Some([0.0; 3]), true);
    assert_eq!(
        (
            value.stats.stale,
            value.stats.unsupported,
            value.stats.missing_camera,
            value.stats.unavailable
        ),
        (3, 1, 1, 1)
    );
    assert_eq!(value.pool.occupied(), 0);
    value.event(&play(6), Some([0.0; 3]), true);
    value.flush(|_| false);
    assert_eq!(value.stats.backend_failed, 1);
    assert_eq!(value.pool.occupied(), 0);
}
#[test]
fn cutoff_uses_three_axes_eighth_coordinates_and_strict_boundary() {
    for axis in 0..3 {
        let mut raw = [0; 3];
        raw[axis] = 128;
        assert!(!inside_radius(raw, [0.0; 3]));
        raw[axis] = 127;
        assert!(inside_radius(raw, [0.0; 3]));
        raw[axis] = -128;
        assert!(!inside_radius(raw, [0.0; 3]));
    }
    assert!(!inside_radius([i32::MAX; 3], [0.0; 3]));
    assert!(!inside_radius([0; 3], [f32::NAN; 3]));
    assert!(inside_radius([8, -16, 24], [1.0, -2.0, 3.0]));
}

#[test]
fn only_unit_finite_unhandled_no_loop_dynamics_are_admitted_and_stop_survives_capacity() {
    let mut value = state();
    for (index, (volume, pitch, loops, handle)) in [
        (0.2, 1.0, -1, None),
        (f32::NAN, 1.0, -1, None),
        (1.0, 0.5, -1, None),
        (1.0, f32::INFINITY, -1, None),
        (1.0, 1.0, -2, None),
        (1.0, 1.0, 1, None),
        (1.0, 1.0, -1, Some(1)),
    ]
    .into_iter()
    .enumerate()
    {
        let mut event = play(index as u64 + 1);
        if let protocol::AudioEvent::Play(play) = &mut event.event {
            play.volume = volume;
            play.pitch = pitch;
            play.loop_count = loops;
            play.server_sound_handle = handle;
        }
        value.event(&event, Some([0.0; 3]), true);
    }
    assert_eq!(value.stats.unsupported, 7);
    assert_eq!(value.pool.occupied(), 0);
    for sequence in 8..24 {
        value.event(&play(sequence), Some([0.0; 3]), true);
    }
    value.event(&play(24), Some([0.0; 3]), true);
    assert_eq!(value.stats.capacity, 1);
    assert_eq!(value.pool.occupied(), 16);
    value.event(&stop(25, false), None, false);
    assert_eq!(value.stats.stopped, 1);
    assert_eq!(
        value.pool.occupied(),
        0,
        "pending sources are cancelled before submission"
    );
    let mut submitted = 0;
    value.flush(|_| {
        submitted += 1;
        true
    });
    assert_eq!(submitted, 0);
}
