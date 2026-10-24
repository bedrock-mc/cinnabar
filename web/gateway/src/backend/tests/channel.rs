//! The selected HTTP route and bounded application Hello must agree before account attachment.

use super::*;
use tokio_util::codec::Decoder;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_play_route_rejects_a_control_hello_without_replacing_its_original_controller() {
    mismatch(RELAY_PLAY_PATH, RELAY_CHANNEL_CONTROL).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_control_route_rejects_a_play_hello_without_acquiring_its_original_play_lease() {
    mismatch(RELAY_CONTROL_PATH, RELAY_CHANNEL_PLAY).await;
}

/// Uses the real original bearer/generation so only the HTTP/Hello channel mismatch refuses.
async fn mismatch(path: &str, channel: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut fixture = Fixture::new().await;
        let (mut control, capability) = fixture.control(String::new()).await;
        let uri = format!("wss://{}{path}", fixture.address).parse().unwrap();
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
        let mut frames = Vec::new();
        let mut complete_framing = false;
        match upgrade {
            Ok((socket, _response)) => {
                let mut browser = Framed::new(
                    Socket::new(socket, LIMIT),
                    bridge::BridgeCodec::with_max(RELAY_CONTROL_BYTES),
                );
                browser
                    .send(Bytes::from(
                        serde_json::to_vec(&Hello {
                            version: RELAY_VERSION,
                            channel: channel.into(),
                            channels: vec![RELAY_CHANNEL_CONTROL.into(), RELAY_CHANNEL_PLAY.into()],
                            resume: capability.resume.clone(),
                            generation: capability.generation,
                            join: 1,
                        })
                        .unwrap(),
                    ))
                    .await
                    .unwrap();
                loop {
                    match browser.next().await {
                        Some(Ok(frame)) => frames.push(frame),
                        Some(Err(bridge::BridgeError::Io(_))) | None => {
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
        test_time::eventually(
            "mismatched Hello retires its actual accepted socket",
            || crate::registry::tests::retained_counts(&fixture.registry) == (1, 1),
        );
        let attachment = fixture
            .registry
            .open_play(&capability.resume, capability.generation, Instant::now())
            .unwrap();
        // The rejected Hello must not consume join1 or retain any account lease.
        let lease = attachment
            .session
            .begin_play(attachment.generation, 1)
            .await;
        let original_join_available = lease.as_ref().is_ok_and(|lease| lease.current().is_ok());
        drop(lease);
        let sent = control
            .send(Bytes::from(serde_json::to_vec(&Request::SignOut).unwrap()))
            .await;
        let mut idle_only = true;
        let control_usable = if sent.is_ok() {
            loop {
                match control.next().await {
                    Some(Ok(frame)) => match serde_json::from_slice::<Event>(&frame) {
                        Ok(Event::Account { .. }) => break true,
                        Ok(Event::Connect { connect }) => {
                            idle_only &= connect.stage == bridge::ConnectStage::Idle
                        }
                        _ => break false,
                    },
                    _ => break false,
                }
            }
        } else {
            false
        };
        let _ = control.close().await;
        drop(control);
        attachment.cancel.cancelled().await;
        drop(attachment);
        fixture.finish().await;
        assert!(
            upgrade_error.is_none(),
            "actual selected route did not upgrade: {upgrade_error:?}"
        );
        assert!(
            frames.is_empty() && complete_framing,
            "mismatched Hello emitted application output or partial framing"
        );
        assert!(
            original_join_available && control_usable && idle_only,
            "mismatched Hello consumed the original lease or replaced its controller"
        );
    })
    .await
    .expect("actual selected-route Hello mismatch fixture stalled");
}
