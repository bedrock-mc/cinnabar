use super::*;

fn audio_stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    })
}
fn play() -> WorldEvent {
    WorldEvent::Audio(protocol::AudioEvent::Play(protocol::PlayAudioEvent {
        name: Arc::from("ambient.underwater.loop"),
        position: [0; 3],
        volume: 1.0,
        pitch: 1.0,
        loop_count: -1,
        server_sound_handle: None,
    }))
}
fn dimension(value: i32) -> WorldEvent {
    WorldEvent::ChangeDimension(ChangeDimensionEvent {
        dimension: value,
        position: [0.0; 3],
    })
}

#[test]
fn audio_origin_epochs_are_captured_at_commit_not_final_drain() {
    let mut stream = audio_stream();
    for (sequence, event) in [
        (1, play()),
        (2, dimension(1)),
        (3, play()),
        (4, dimension(0)),
        (5, play()),
    ] {
        stream.submit(sequence, event).unwrap();
    }
    let rows = stream.take_committed_audio();
    assert_eq!(
        rows.iter()
            .map(|row| (row.sequence, row.dimension, row.dimension_epoch))
            .collect::<Vec<_>>(),
        vec![(1, 0, 0), (3, 1, 2), (5, 0, 4)]
    );
    assert!(stream.take_committed_audio().is_empty());
}

#[test]
fn camera_lifetime_latch_survives_drains_timing_resets_and_returning_dimensions() {
    let mut stream = audio_stream();
    assert!(stream.audio_default_camera_eligible());
    stream
        .submit(
            1,
            WorldEvent::Camera(protocol::CameraEvent::Instruction(
                protocol::CameraInstructionEvent::default(),
            )),
        )
        .unwrap();
    assert!(!stream.audio_default_camera_eligible());
    stream.take_committed_camera().clear();
    stream.begin_timed_session();
    stream.submit(2, dimension(1)).unwrap();
    stream.submit(3, dimension(0)).unwrap();
    stream
        .submit(
            4,
            WorldEvent::Camera(protocol::CameraEvent::Instruction(
                protocol::CameraInstructionEvent {
                    clear: Some(true),
                    ..Default::default()
                },
            )),
        )
        .unwrap();
    stream.take_committed_camera();
    assert!(!stream.audio_default_camera_eligible());
    let fresh = audio_stream();
    assert!(fresh.audio_default_camera_eligible());
    assert_ne!(fresh.actor_session_id(), stream.actor_session_id());
}

#[test]
fn audio_admission_refusal_preserves_existing_order_and_named_stop() {
    let mut stream = audio_stream();
    for sequence in 1..COMMITTED_AUDIO_CAPACITY as u64 {
        stream.submit(sequence, play()).unwrap();
    }
    let stop = WorldEvent::Audio(protocol::AudioEvent::Stop(protocol::StopAudioEvent {
        name: Arc::from("ambient.underwater.loop"),
        stop_all_sounds: false,
        stop_music_legacy: false,
    }));
    stream
        .submit(COMMITTED_AUDIO_CAPACITY as u64, stop)
        .unwrap();
    assert!(matches!(
        stream.submit(COMMITTED_AUDIO_CAPACITY as u64 + 1, play()),
        Err(WorldStreamError::AdmissionFull { .. })
    ));
    let rows = stream.take_committed_audio();
    assert_eq!(rows.len(), COMMITTED_AUDIO_CAPACITY);
    assert!(matches!(
        &rows.last().unwrap().event,
        protocol::AudioEvent::Stop(_)
    ));
    assert_eq!(
        rows.last().unwrap().sequence,
        COMMITTED_AUDIO_CAPACITY as u64
    );
}

#[test]
fn every_camera_family_including_clear_and_shake_stop_sets_the_lifetime_latch() {
    for event in [
        protocol::CameraEvent::Switch(protocol::CameraSwitchEvent {
            camera_unique_id: 1,
            target_player_unique_id: 1,
        }),
        protocol::CameraEvent::Instruction(protocol::CameraInstructionEvent::default()),
        protocol::CameraEvent::Instruction(protocol::CameraInstructionEvent {
            clear: Some(true),
            ..Default::default()
        }),
        protocol::CameraEvent::Instruction(protocol::CameraInstructionEvent {
            clear: Some(false),
            ..Default::default()
        }),
        protocol::CameraEvent::Shake(protocol::CameraShakeEvent {
            intensity: 0.1,
            duration_seconds: 0.1,
            shake_type: protocol::CameraShakeType::Positional,
            action: protocol::CameraShakeAction::Add,
        }),
        protocol::CameraEvent::Shake(protocol::CameraShakeEvent {
            intensity: 0.0,
            duration_seconds: 0.0,
            shake_type: protocol::CameraShakeType::Rotational,
            action: protocol::CameraShakeAction::Stop,
        }),
    ] {
        let mut stream = audio_stream();
        assert!(stream.audio_default_camera_eligible());
        stream.submit(1, WorldEvent::Camera(event)).unwrap();
        assert!(!stream.audio_default_camera_eligible());
        assert!(stream.stats().audio_nondefault_camera_observed);
        assert_eq!(stream.take_committed_camera().len(), 1);
        stream.begin_timed_session();
        stream.submit(2, dimension(1)).unwrap();
        stream.submit(3, dimension(0)).unwrap();
        assert!(!stream.audio_default_camera_eligible());
        assert!(audio_stream().audio_default_camera_eligible());
    }
}
