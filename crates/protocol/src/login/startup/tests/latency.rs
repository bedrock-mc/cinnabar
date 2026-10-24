//! Requested stack-latency acknowledgments remain live before PlayerSpawn.

use super::*;
use bedrock_protocol_1_26_51::{enums::PlayStatusType, types::ActorRuntimeID};
use futures::StreamExt;
use std::io;

const RUNTIME_ID: u64 = 42;
const REQUESTED_TIME: u64 = 777;

/// Generates a deferred control whose wire order is checked after the actual spawn gate.
fn time(value: i32) -> Packet {
    Packet::new(SetTime {
        time: wire::ZigZag32(value),
    })
}

/// Reads the original framed peer output without adding another protocol or queue.
async fn receive(reader: &mut impl bridge::FrameRead) -> io::Result<Option<Vec<Packet>>> {
    let Some(frame) = reader.next().await else {
        return Ok(None);
    };
    let frame = frame.map_err(io::Error::other)?;
    if frame.first() != Some(&bridge::contract::KIND_BATCH) {
        return Err(io::Error::other("expected the original packet batch"));
    }
    let packets = decode_batch_body(frame.slice(1..), usize::MAX)
        .map_err(|_| io::Error::other("malformed fixture batch"))?;
    packets
        .into_iter()
        .map(|raw| {
            raw.decode()
                .map_err(|_| io::Error::other("malformed fixture packet"))
        })
        .collect::<io::Result<Vec<_>>>()
        .map(Some)
}

#[tokio::test]
async fn requested_latency_before_player_spawn_replies_without_losing_deferred_controls() {
    let (client, peer) = tokio::io::duplex(8192);
    let (client_read, client_write) = tokio::io::split(client);
    let (reader, frames, writer) =
        bridge::tasks::prepare_connection(client_read, client_write, bridge::MAX_FRAME_LEN);
    let mut transport = SessionTransport::new(reader, frames, &[]);
    let (peer_read, peer_write) = tokio::io::split(peer);
    let (mut peer_reader, peer_frames, peer_writer) =
        bridge::tasks::prepare_connection(peer_read, peer_write, bridge::MAX_FRAME_LEN);
    let writer = tokio::spawn(writer);
    let peer_writer = tokio::spawn(peer_writer);
    let unrequested = Packet::new(packets::NetworkStackLatency {
        creation_time: wire::U64LE(888),
        is_from_server: false,
    });
    let expected_deferred = vec![time(1), unrequested.clone(), time(2), time(3), time(4)];
    let deferred_count = expected_deferred.len();
    let client = async move {
        let result = async {
            let ready = await_spawn(&mut transport, DEFAULT_CHUNK_RADIUS, |_, _| {}).await?;
            let runtime_id = ready.runtime_id();
            transport.send(&loading_finished(ready)).await?;
            let mut deferred = Vec::new();
            for _ in 0..deferred_count {
                let Inbound::Packet(raw) = transport.recv().await? else {
                    return Err(SessionFailure::ConnectionClosed.into());
                };
                deferred.push(raw.decode()?);
            }
            Ok::<_, ProtocolError>((runtime_id, deferred))
        }
        .await;
        drop(transport);
        result
    };
    let peer = async move {
        peer_frames
            .send(
                batch_frame(&[
                    Packet::new(StartGame {
                        runtime_id: ActorRuntimeID(RUNTIME_ID),
                        ..Default::default()
                    }),
                    Packet::new(ItemRegistry::default()),
                    time(1),
                    unrequested,
                    time(2),
                    Packet::new(packets::NetworkStackLatency {
                        creation_time: wire::U64LE(REQUESTED_TIME),
                        is_from_server: true,
                    }),
                    time(3),
                ])
                .unwrap(),
            )
            .await
            .map_err(io::Error::other)?;
        let initial = receive(&mut peer_reader).await?;
        let reply = receive(&mut peer_reader).await?;
        if reply != Some(vec![crate::network_stack_latency_reply(REQUESTED_TIME)]) {
            return Err(io::Error::other(
                "requested startup probe was not acknowledged",
            ));
        }
        // Only the actual correctly scaled reply releases the original readiness prerequisite.
        peer_frames
            .send(
                batch_frame(&[
                    Packet::new(packets::PlayStatus {
                        status: PlayStatusType::Playerspawn,
                    }),
                    time(4),
                ])
                .unwrap(),
            )
            .await
            .map_err(io::Error::other)?;
        let mut after_reply = Vec::new();
        while let Some(batch) = receive(&mut peer_reader).await? {
            after_reply.push(batch);
        }
        Ok::<_, io::Error>((initial, after_reply))
    };
    let result =
        tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(client, peer) }).await;
    // Both futures own their original streams and queues, including on timeout.
    let writer = writer.await;
    let peer_writer = peer_writer.await;
    writer.expect("the original client writer must retire");
    peer_writer.expect("the original peer writer must retire");
    let (client, peer) =
        result.expect("startup waited for PlayerSpawn before replying to its probe");
    let (runtime_id, deferred) = client.expect("the real PlayerSpawn must complete startup");
    let (initial, after_reply) = peer.expect("the original peer exchange must complete");
    assert_eq!(runtime_id, RUNTIME_ID);
    assert_eq!(deferred, expected_deferred);
    assert_eq!(
        initial,
        Some(vec![
            crate::request_chunk_radius_packet(
                DEFAULT_CHUNK_RADIUS,
                render_api::MAX_VIEW_RADIUS_CHUNKS as u8,
            ),
            crate::loading_screen_packet(LoadingScreenPhase::Start, None),
        ])
    );
    assert_eq!(
        after_reply,
        vec![vec![
            crate::loading_screen_packet(LoadingScreenPhase::End, None),
            Packet::new(packets::SetLocalPlayerAsInitialized {
                player_id: ActorRuntimeID(RUNTIME_ID),
            }),
        ]],
        "the unrequested probe must not produce an extra reply"
    );
}
