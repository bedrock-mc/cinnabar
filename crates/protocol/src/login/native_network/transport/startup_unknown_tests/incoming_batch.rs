//! The application wire allowance applies to complete incoming reliable messages.

use super::*;

const SCRIPT_BYTES: usize = 3 * 1024 * 1024;

struct Served {
    encoded_bytes: usize,
    suffix: io::Result<()>,
}

struct Received {
    runtime_id: u64,
    startup: Vec<Packet>,
    prefix: Packet,
    live: Result<[Packet; 2], ProtocolError>,
    closed: Result<(), ProtocolError>,
}

/// Sends generated siblings around one complete, genuinely uncompressed reliable body.
async fn send_live(peer: &mut AcceptedSession) -> io::Result<Served> {
    serve_compressed(peer, Case::Healthy, CompressionAlgorithm::None).await?;
    let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default()).negotiate(
        CompressionSettings {
            algorithm: CompressionAlgorithm::None,
            threshold: u16::MAX,
        },
    );
    send(peer, &codec, Case::Healthy, Stage::Live, &[time(3)]).await?;
    let packet =
        crate::session_transport::tests::packet_bytes(&Packet::new(packets::ScriptMessage {
            message_id: LIVE_SCRIPT_CHANNEL.into(),
            message_value: vec![LIVE_SCRIPT_BYTE; SCRIPT_BYTES].into(),
        }));
    let mut messages = codec.encode(&[packet]).map_err(io::Error::other)?;
    if messages.len() != 1 {
        return Err(io::Error::other(
            "generated script split across reliable bodies",
        ));
    }
    let message = messages.pop().expect("one encoded body was checked");
    if message.first() != Some(&u8::MAX) || message.len() <= SCRIPT_BYTES {
        return Err(io::Error::other(
            "script body was not genuinely uncompressed",
        ));
    }
    let encoded_bytes = message.len();
    peer.session.send(message).await.map_err(io::Error::other)?;
    let suffix = send(peer, &codec, Case::Healthy, Stage::Live, &[time(4)]).await;
    Ok(Served {
        encoded_bytes,
        suffix,
    })
}

/// Retires the real play, accepted peer and listener before returning any behavioral oracle.
async fn exercise_encoded_live() -> (Result<Received, ProtocolError>, io::Result<Served>) {
    tokio::time::timeout(Duration::from_secs(20), async {
        let fixture = fixture_route(Route::Lan).await;
        let mut listener = fixture.listener;
        let (rejected, mut rejection) = tokio::sync::oneshot::channel();
        let (delivered, delivery) = tokio::sync::oneshot::channel();
        let server = async {
            let mut peer = listener.accept().await.map_err(io::Error::other)?;
            let result = tokio::select! { biased;
                result = async {
                    let served = send_live(&mut peer).await?;
                    // Retain the original peer until the client observes its body or terminal.
                    delivery.await.map_err(|_| io::Error::other("original live read was dropped"))?;
                    Ok(served)
                } => result,
                Ok(()) = &mut rejection => Err(io::Error::other("client rejected original startup")),
            };
            let retired = peer.close();
            retired.session.closed().await;
            drop(retired);
            result
        };
        let client = async {
            let settings = crate::LoginSettings::default();
            let result = async {
                let (mut play, runtime_id) = LoginSequence::connect_native(
                    &fixture.target,
                    SessionLogin {
                        #[cfg(not(target_arch = "wasm32"))]
                        execution: None,
                        display_name: "Fixture",
                        client_random_id: crate::ClientRandomId::new(1).unwrap(),
                        cache: None,
                        skin: None,
                        settings: &settings,
                        tap: None,
                        #[cfg(not(target_arch = "wasm32"))]
                        relay: None,
                        target: None,
                    },
                    |_, game| game.world.local_player_runtime_id,
                ).await?;
                let received = async {
                    let mut startup = Vec::new();
                    for _ in 0..3 {
                        startup.push(play.recv().await?);
                    }
                    play.finish_loading().await?;
                    let prefix = play.recv().await?;
                    let live = async {
                        Ok::<_, ProtocolError>([play.recv().await?, play.recv().await?])
                    }.await;
                    Ok::<_, ProtocolError>((startup, prefix, live))
                }.await;
                let _ = delivered.send(());
                let closed = play.close().await;
                drop(play);
                let (startup, prefix, live) = received?;
                Ok(Received { runtime_id, startup, prefix, live, closed })
            }.await;
            if result.is_err() {
                let _ = rejected.send(());
            }
            result
        };
        let (server, client) = tokio::join!(server, client);
        listener.close().await.unwrap();
        drop(listener);
        (client, server)
    }).await.expect("finite original encoded live fixture stalled")
}

#[tokio::test]
async fn native_live_uncompressed_batch_above_transport_default_keeps_complete_delivery() {
    let (client, server) = exercise_encoded_live().await;
    let served = server.expect("original peer did not admit the generated reliable body");
    let received = client.expect("original startup, initialization or accepted prefix failed");
    assert_eq!(received.runtime_id, 42);
    assert_eq!(received.startup, [time(1), radius(), time(2)]);
    assert_eq!(received.prefix, time(3));
    assert!(served.encoded_bytes > SCRIPT_BYTES);
    assert!(served.encoded_bytes <= BatchLimits::default().max_encoded_bytes);
    let [script, suffix] = received.live.expect(
        "complete encoded body within the application wire allowance was rejected by transport",
    );
    received
        .closed
        .expect("original play did not close normally");
    served.suffix.expect("original suffix was not admitted");
    assert_eq!(script.header.id, packets::PacketId::ScriptMessage);
    assert_eq!(script.header.from_subclient, 0);
    assert_eq!(script.header.to_subclient, 0);
    let packets::Packet::ScriptMessage(value) = script.data.as_ref() else {
        panic!("original generated script was replaced");
    };
    assert_eq!(value.message_id.to_str().unwrap(), LIVE_SCRIPT_CHANNEL);
    assert_eq!(value.message_value.len(), SCRIPT_BYTES);
    assert!(
        value
            .message_value
            .iter()
            .all(|byte| *byte == LIVE_SCRIPT_BYTE)
    );
    assert_eq!(suffix, time(4));
}
