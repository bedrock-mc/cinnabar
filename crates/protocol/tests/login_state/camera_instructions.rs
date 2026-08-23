use super::*;
use disconnect_reason::{CAMERA_INTERLEAVED_SENTINEL_TIME, EPILOGUE_SENTINEL_TIME, PlayEpilogue};
use protocol::CameraEvent;

const DRAIN_LIMIT: usize = 32;

fn scripted_camera_transport(epilogue: PlayEpilogue) -> ScriptTransport {
    let mut script = ServerScript::new(
        CompressionMode::Deflate,
        SpawnOrder::RadiusThenSpawn,
        false,
        false,
        false,
        CachePlayScript::ResolveValid,
    );
    script.epilogue = epilogue;
    ScriptTransport {
        script: Arc::new(Mutex::new(script)),
    }
}

#[tokio::test]
async fn play_ingress_preserves_wire_order_across_interleaved_camera_families() {
    let transport = scripted_camera_transport(PlayEpilogue::CameraInstructions);
    let (mut session, _) = LoginSequence::connect_transport(transport, "RustClient")
        .await
        .expect("scripted login");

    let mut events = Vec::new();
    for _ in 0..DRAIN_LIMIT {
        let event = session
            .recv_world_event(0)
            .await
            .expect("camera traffic stays decodable");
        let reached_end = matches!(
            &event,
            WorldEvent::SetTime(protocol::SetTimeEvent {
                time: CAMERA_INTERLEAVED_SENTINEL_TIME,
            })
        );
        events.push(event);
        if reached_end {
            break;
        }
    }
    assert!(
        matches!(
            events.last(),
            Some(WorldEvent::SetTime(protocol::SetTimeEvent {
                time: CAMERA_INTERLEAVED_SENTINEL_TIME,
            }))
        ),
        "the interleaved sentinel must arrive"
    );

    let instruction_index = events
        .iter()
        .position(|event| matches!(event, WorldEvent::Camera(CameraEvent::Instruction(_))))
        .expect("clear instruction arrives");
    let shake_index = events
        .iter()
        .position(|event| matches!(event, WorldEvent::Camera(CameraEvent::Shake(_))))
        .expect("shake arrives");
    let switch_index = events
        .iter()
        .position(|event| matches!(event, WorldEvent::Camera(CameraEvent::Switch(_))))
        .expect("legacy switch arrives");
    assert_eq!(instruction_index + 1, shake_index - 1);
    assert_eq!(shake_index + 1, switch_index);

    let clear_instruction = match &events[instruction_index] {
        WorldEvent::Camera(CameraEvent::Instruction(event)) => event.clone(),
        other => panic!("expected instruction event, got {other:?}"),
    };
    assert_eq!(
        clear_instruction,
        protocol::CameraInstructionEvent {
            clear: Some(true),
            ..Default::default()
        }
    );
    assert_eq!(
        events[switch_index],
        WorldEvent::Camera(protocol::CameraEvent::Switch(protocol::CameraSwitchEvent {
            camera_unique_id: -11,
            target_player_unique_id: -22,
        }))
    );
    assert_eq!(
        events[instruction_index + 1],
        WorldEvent::SetTime(protocol::SetTimeEvent {
            time: EPILOGUE_SENTINEL_TIME
        })
    );
    // One skip is the script's own malformed LevelChunk; camera traffic adds none.
    assert_eq!(session.world_skip_count(), 1);
    assert_eq!(session.decode_error_count(), 0);
}

#[tokio::test]
async fn unsupported_camera_instruction_is_counted_and_session_survives() {
    let transport = scripted_camera_transport(PlayEpilogue::OddCameraInstruction);
    let (mut session, _) = LoginSequence::connect_transport(transport, "RustClient")
        .await
        .expect("scripted login");

    for _ in 0..DRAIN_LIMIT {
        let event = session
            .recv_world_event(0)
            .await
            .expect("odd-but-well-formed camera data must not end the session");
        if matches!(
            &event,
            WorldEvent::SetTime(protocol::SetTimeEvent {
                time: EPILOGUE_SENTINEL_TIME
            })
        ) {
            break;
        }
    }
    // The sentinel follows the skipped spline instruction in wire order, so
    // reaching it proves ingress survived the semantic skip. One skip is the
    // script's own malformed LevelChunk; the spline adds exactly one more.
    assert_eq!(session.world_skip_count(), 2);
    assert_eq!(session.decode_error_count(), 0);
}

#[tokio::test]
async fn truncated_camera_shake_wire_is_fatal_in_play_ingress() {
    let transport = scripted_camera_transport(PlayEpilogue::TruncatedCameraShake);
    let (mut session, _) = LoginSequence::connect_transport(transport, "RustClient")
        .await
        .expect("scripted login");

    for _ in 0..DRAIN_LIMIT {
        match session.recv_world_event(0).await {
            Ok(WorldEvent::SetTime(protocol::SetTimeEvent { time }))
                if time == EPILOGUE_SENTINEL_TIME =>
            {
                continue;
            }
            Ok(_) => {}
            Err(error) => {
                assert!(matches!(error, ProtocolError::Session(_)));
                assert_eq!(session.decode_error_count(), 1);
                return;
            }
        }
    }
    panic!("truncated camera wire never failed the session");
}
