//! Ordinary upstream play keeps known headers while skipping unused, well-formed IDs.

use super::*;
use crate::codec::SessionFailure;
use bedrock_protocol_1_26_51::types::DisconnectMessages;

#[derive(Clone, Copy)]
enum LiveCase {
    Unknown,
    Healthy,
    MalformedTerminal,
}

/// Preserves both primary markers, a known secondary header and the original primary terminal.
fn expected() -> [Packet; 4] {
    let mut secondary = time(5);
    secondary.header.from_subclient = 1;
    secondary.header.to_subclient = 2;
    [
        time(3),
        time(4),
        secondary,
        Packet::new(packets::Disconnect {
            messages: DisconnectMessages::DisconnectPacketMessages {
                message: "original live terminal".into(),
                filtered_message: "original filtered terminal".into(),
            },
            ..Default::default()
        }),
    ]
}

/// Inserts one unused or malformed entry between actual generated primary packets.
fn entries(case: LiveCase) -> Vec<Bytes> {
    let packets = expected();
    let mut entries = vec![crate::session_transport::tests::packet_bytes(&packets[0])];
    match case {
        LiveCase::Unknown => entries.push(Case::Stack.entry()),
        LiveCase::MalformedTerminal => {
            let mut header = Vec::new();
            wire::Writer::new(&mut header)
                .write_var_u32(
                    wire::PacketHeader::new(packets::PacketId::Disconnect as u32).to_raw(),
                )
                .unwrap();
            entries.push(header.into());
        }
        LiveCase::Healthy => {}
    }
    entries.extend(
        packets[1..]
            .iter()
            .map(crate::session_transport::tests::packet_bytes),
    );
    entries
}

type Received = (u64, Vec<Packet>, Vec<Result<Packet, ProtocolError>>);

/// Reuses the real Initialized handshake and retires every original owner before returning results.
async fn exercise_live(case: LiveCase) -> (Result<Received, ProtocolError>, io::Result<()>) {
    tokio::time::timeout(Duration::from_secs(20), async {
        let fixture = fixture_route(Route::Lan).await;
        let mut listener = fixture.listener;
        let (rejected, mut rejection) = tokio::sync::oneshot::channel();
        let (delivered, delivery) = tokio::sync::oneshot::channel();
        let server = async {
            let mut peer = listener.accept().await.map_err(io::Error::other)?;
            let result = tokio::select! { biased;
                result = async {
                    serve(&mut peer, Case::Healthy).await?;
                    let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default())
                        .negotiate(CompressionSettings {
                            algorithm: CompressionAlgorithm::Deflate,
                            threshold: u16::MAX,
                        });
                    let messages = codec.encode(&entries(case)).map_err(io::Error::other)?;
                    if messages.len() != 1 {
                        return Err(io::Error::other("live siblings did not share one original batch"));
                    }
                    for message in messages {
                        peer.session.send(message).await.map_err(io::Error::other)?;
                    }
                    delivery.await.map_err(|_| io::Error::other("original live read was dropped"))?;
                    Ok(())
                } => result,
                Ok(()) = &mut rejection => Err(io::Error::other("client rejected original fixture startup")),
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
                    let mut live = Vec::new();
                    for _ in 0..expected().len() {
                        let packet = play.recv().await;
                        let failed = packet.is_err();
                        live.push(packet);
                        if failed {
                            break;
                        }
                    }
                    Ok::<_, ProtocolError>((runtime_id, startup, live))
                }.await;
                let _ = delivered.send(());
                let closed = play.close().await;
                drop(play);
                closed?;
                received
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
    }).await.expect("finite actual desktop live ingress fixture stalled")
}

/// Verifies original public join/Initialized facts before inspecting retained live read outcomes.
fn startup(result: Result<Received, ProtocolError>) -> Vec<Result<Packet, ProtocolError>> {
    let (runtime, packets, live) = result.expect("original fixture startup or cleanup failed");
    assert_eq!(runtime, 42);
    assert_eq!(packets, [time(1), radius(), time(2)]);
    live
}

#[tokio::test]
async fn native_desktop_live_unknown_keeps_primary_fifo_headers_and_terminal() {
    let (client, server) = exercise_live(LiveCase::Unknown).await;
    server.unwrap();
    let live = startup(client);
    let actual = live.into_iter().collect::<Result<Vec<_>, _>>();
    assert_eq!(
        actual.expect("well-formed unused live ID ended the ordinary desktop session"),
        expected(),
        "unused live input lost a known packet, its original header or the primary terminal"
    );
}

#[tokio::test]
async fn native_desktop_live_known_secondary_headers_and_primary_terminal_keep_fifo() {
    let (client, server) = exercise_live(LiveCase::Healthy).await;
    server.unwrap();
    let live = startup(client);
    assert_eq!(
        live.into_iter().collect::<Result<Vec<_>, _>>().unwrap(),
        expected()
    );
}

#[tokio::test]
async fn native_desktop_live_malformed_terminal_keeps_prefix_and_original_wire_error() {
    let (client, server) = exercise_live(LiveCase::MalformedTerminal).await;
    server.unwrap();
    let mut live = startup(client).into_iter();
    assert_eq!(live.next().unwrap().unwrap(), time(3));
    assert!(matches!(
        live.next().unwrap(),
        Err(ProtocolError::Session(crate::SessionError(
            SessionFailure::PacketDecode {
                packet_id: packets::PacketId::Disconnect,
                source: wire::DecodeError::UnexpectedEof { .. },
                ..
            }
        )))
    ));
    assert!(live.next().is_none());
}
