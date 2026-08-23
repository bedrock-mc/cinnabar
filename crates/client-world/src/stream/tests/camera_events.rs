use super::*;

fn camera_stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    })
}

fn shake(intensity: f32) -> WorldEvent {
    WorldEvent::Camera(protocol::CameraEvent::Shake(protocol::CameraShakeEvent {
        intensity,
        duration_seconds: 2.0,
        shake_type: protocol::CameraShakeType::Positional,
        action: protocol::CameraShakeAction::Add,
    }))
}

#[test]
fn committed_camera_events_preserve_packet_fifo_across_interleaved_families() {
    let mut stream = camera_stream();
    stream.submit(1, shake(0.25)).expect("admit camera shake");
    stream
        .submit(2, WorldEvent::SetTime(SetTimeEvent { time: 7 }))
        .expect("admit set time");
    stream
        .submit(
            3,
            WorldEvent::Camera(protocol::CameraEvent::Switch(protocol::CameraSwitchEvent {
                camera_unique_id: -3,
                target_player_unique_id: -4,
            })),
        )
        .expect("admit legacy switch");

    let audio = stream.take_committed_audio();
    let camera = stream.take_committed_camera();
    assert_eq!(camera.len(), 2);
    assert_eq!(camera[0].sequence, 1);
    assert_eq!(
        camera[0].event,
        protocol::CameraEvent::Shake(protocol::CameraShakeEvent {
            intensity: 0.25,
            duration_seconds: 2.0,
            shake_type: protocol::CameraShakeType::Positional,
            action: protocol::CameraShakeAction::Add,
        })
    );
    assert_eq!(camera[1].sequence, 3);
    assert_eq!(
        camera[1].event,
        protocol::CameraEvent::Switch(protocol::CameraSwitchEvent {
            camera_unique_id: -3,
            target_player_unique_id: -4,
        })
    );
    // The interleaved SetTime committed through its own channel untouched.
    assert_eq!(stream.take_committed_controls().len(), 1);
    drop(audio);
}

#[test]
fn take_committed_camera_drains_so_a_second_call_is_empty() {
    let mut stream = camera_stream();
    stream.submit(1, shake(0.5)).expect("admit camera shake");
    assert_eq!(stream.take_committed_camera().len(), 1);
    assert!(stream.take_committed_camera().is_empty());
}

#[test]
fn committed_camera_events_count_toward_retained_commit_accounting() {
    let mut stream = camera_stream();
    for sequence in 1..=4 {
        stream.submit(sequence, shake(0.5)).expect("admit camera");
    }
    let stats = stream.stats();
    assert_eq!(stats.committed_camera_events, 4);
}
