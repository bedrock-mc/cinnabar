//! Accepted same-origin TLS/WebSocket play reaches metadata refusal before upstream acquisition.

use super::*;
use protocol::{
    RawClientData,
    session_wire::{self, CoreMessage, SessionDisconnect},
};
use tokio_util::codec::Decoder;

/// A real Play Hello and valid large Connect use the original control capability and selected limits.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_tls_play_refuses_a_newer_protocol_and_preserves_its_original_control() {
    tokio::time::timeout(Duration::from_secs(10), async {
        // Validate the ordinary full skin/metadata before starting any owned fixture task.
        let connect = newer_connect();
        let message_bytes = connect.len().checked_add(size_of::<u32>()).unwrap();
        assert!(
            message_bytes > LIMIT,
            "actual Connect did not exceed the control WebSocket limit"
        );
        assert!(
            message_bytes <= RELAY_PLAY_MESSAGE_BYTES,
            "actual Connect exceeded the play WebSocket limit"
        );
        let expected =
            session_wire::encode_core_message(&CoreMessage::Disconnect(SessionDisconnect {
                reason: 0,
                message: DISCONNECT_KEY_OUTDATED_SERVER.into(),
                filtered_message: String::new(),
                hide_screen: false,
            }))
            .unwrap();
        let mut fixture = Fixture::new().await;
        let (mut control, capability) = fixture.control(String::new()).await;
        let uri = format!("wss://{}{RELAY_PLAY_PATH}", fixture.address)
            .parse()
            .unwrap();
        let builder = ClientBuilder::from_uri(uri)
            .add_header(
                SEC_WEBSOCKET_PROTOCOL,
                HeaderValue::from_static(RELAY_SOCKET_PROTOCOL),
            )
            .unwrap()
            .add_header(
                ORIGIN,
                HeaderValue::from_str(&format!("https://{}", fixture.address)).unwrap(),
            )
            .unwrap();
        let upgrade = builder.connect_on(fixture.secure().await).await;
        let mut upgrade_error = None;
        let mut selected_protocol = false;
        let mut outbound = Vec::new();
        let mut complete_framing = false;
        match upgrade {
            Ok((socket, response)) => {
                selected_protocol = response
                    .headers()
                    .get(SEC_WEBSOCKET_PROTOCOL)
                    .is_some_and(|value| value == RELAY_SOCKET_PROTOCOL);
                let mut browser = Framed::new(
                    Socket::new(socket, RELAY_PLAY_MESSAGE_BYTES),
                    bridge::BridgeCodec::with_max(bridge::MAX_FRAME_LEN),
                );
                browser
                    .send(Bytes::from(
                        serde_json::to_vec(&Hello {
                            version: RELAY_VERSION,
                            channel: RELAY_CHANNEL_PLAY.into(),
                            channels: vec![RELAY_CHANNEL_CONTROL.into(), RELAY_CHANNEL_PLAY.into()],
                            resume: capability.resume.clone(),
                            generation: capability.generation,
                            join: 1,
                        })
                        .unwrap(),
                    ))
                    .await
                    .unwrap();
                // The existing Socket writes this bounded encoded frame as one binary WS message.
                browser.send(connect).await.unwrap();
                loop {
                    match browser.next().await {
                        Some(Ok(frame)) => outbound.push(frame),
                        Some(Err(bridge::BridgeError::Io(_))) | None => {
                            // A completed refusal may physically drop TLS without close_notify.
                            // No partial application frame may be mistaken for a complete terminal.
                            let mut remaining = browser.read_buffer().clone();
                            complete_framing =
                                matches!(browser.codec_mut().decode_eof(&mut remaining), Ok(None))
                                    && remaining.is_empty();
                            break;
                        }
                        Some(Err(_)) => break,
                    }
                }
                drop(browser);
            }
            Err(error) => upgrade_error = Some(error.to_string()),
        }
        test_time::eventually("actual play handler released its accepted socket", || {
            crate::registry::tests::retained_counts(&fixture.registry) == (1, 1)
        });
        // The original control, bearer generation and ephemeral account remain usable after refusal.
        control
            .send(Bytes::from(serde_json::to_vec(&Request::SignOut).unwrap()))
            .await
            .unwrap();
        let mut idle_progress = true;
        let control_usable = loop {
            match event(&mut control).await {
                Event::Account { .. } => break true,
                Event::Connect { connect } => {
                    idle_progress &= connect.stage == bridge::ConnectStage::Idle
                }
                _ => break false,
            }
        };
        let attachment = fixture
            .registry
            .open_play(&capability.resume, capability.generation, Instant::now())
            .unwrap();
        let lease = attachment
            .session
            .begin_play(attachment.generation, 2)
            .await;
        let lease_released = lease.as_ref().is_ok_and(|lease| lease.current().is_ok());
        drop(lease);
        control.close().await.unwrap();
        drop(control);
        attachment.cancel.cancelled().await;
        drop(attachment);
        fixture.finish().await;

        assert!(
            upgrade_error.is_none(),
            "actual /v2/play did not switch to WebSocket: {upgrade_error:?}"
        );
        assert!(
            selected_protocol,
            "actual play upgrade did not select the shared subprotocol"
        );
        assert!(
            complete_framing,
            "actual refusal ended with malformed or partial framing"
        );
        assert_eq!(
            outbound,
            [expected],
            "actual play must emit only its exact version refusal, without Handoff"
        );
        assert!(
            control_usable && idle_progress && lease_released,
            "actual play refusal retained its lease or retired the original control"
        );
    })
    .await
    .expect("actual accepted TLS play refusal fixture stalled");
}

/// Uses the ordinary client metadata and Connect schema, changing only its newer protocol.
fn newer_connect() -> Bytes {
    let mut claims = protocol::login_client_data(
        "Fixture",
        protocol::ClientRandomId::new(1).unwrap(),
        None,
        &protocol::LoginSettings::default(),
    );
    claims["ServerAddress"] = SocketAddr::from((Ipv4Addr::LOCALHOST, DEFAULT_BEDROCK_PORT))
        .to_string()
        .into();
    let raw = serde_json::value::to_raw_value(&claims).unwrap();
    let claims_valid = protocol::NativeDeviceProfile::fresh()
        .validate_relay(
            RawClientData::from_raw(&raw).unwrap(),
            protocol::PROTOCOL_VERSION,
        )
        .is_ok();
    assert!(
        claims_valid,
        "ordinary current-version metadata was not valid before the route witness"
    );
    session_wire::encode_connect(&session_wire::ConnectRequest {
        protocol: protocol::PROTOCOL_VERSION.checked_add(1).unwrap(),
        join_profile_id: None,
        target: None,
        client_cache: true,
        client_data: claims,
    })
    .unwrap()
}
