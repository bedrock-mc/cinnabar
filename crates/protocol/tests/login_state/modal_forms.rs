use super::*;
use disconnect_reason::{EPILOGUE_SENTINEL_TIME, PlayEpilogue};

const DRAIN_LIMIT: usize = 32;

fn scripted_form_transport(epilogue: PlayEpilogue) -> ScriptTransport {
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
async fn semantically_invalid_form_json_is_skipped_counted_and_survivable() {
    let transport = scripted_form_transport(PlayEpilogue::OddModalForm);
    let (mut session, _) = LoginSequence::connect_transport(transport, "RustClient")
        .await
        .expect("scripted login");

    for _ in 0..DRAIN_LIMIT {
        let event = session
            .recv_world_event(0)
            .await
            .expect("odd-but-well-formed form content must not end the session");
        if matches!(
            &event,
            WorldEvent::SetTime(protocol::SetTimeEvent {
                time: EPILOGUE_SENTINEL_TIME,
            })
        ) {
            // One skip is the script's own malformed LevelChunk; the malformed
            // form JSON adds exactly one more.
            assert_eq!(session.world_skip_count(), 2);
            assert_eq!(session.decode_error_count(), 0);
            return;
        }
    }
    panic!("the post-form sentinel never arrived");
}

#[tokio::test]
async fn truncated_modal_form_request_wire_stays_fatal_in_play_ingress() {
    let transport = scripted_form_transport(PlayEpilogue::TruncatedModalFormRequest);
    let (mut session, _) = LoginSequence::connect_transport(transport, "RustClient")
        .await
        .expect("scripted login");

    for _ in 0..DRAIN_LIMIT {
        match session.recv_world_event(0).await {
            Ok(WorldEvent::SetTime(protocol::SetTimeEvent { time: 34_567 })) => continue,
            Ok(_) => {}
            Err(error) => {
                // Truncated wire must terminate ingress as a hard error, never
                // degrade into a counted semantic skip. The UI pre-validator
                // surfaces the truncation as a valentine decode failure.
                assert!(matches!(error, ProtocolError::Decode(_)));
                assert_eq!(session.world_skip_count(), 1);
                return;
            }
        }
    }
    panic!("truncated modal form request wire never failed the session");
}
