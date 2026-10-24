//! Well-framed unused IDs differ from malformed framing at consuming and raw-forwarding edges.

use super::*;
use crate::codec::SessionFailure;
use crate::session_transport::SessionTransport;
use bedrock_protocol_1_26_51::types::DisconnectMessages;
use futures::StreamExt;

/// Derives an unused ID from the actual generated table and keeps its original subclient header.
fn unknown() -> Bytes {
    let id = (0..=wire::PacketHeader::ID_MASK)
        .rev()
        .find(|id| packets::PacketId::from_raw(*id).is_none())
        .expect("the selected protocol has an unused ID");
    let mut raw = Vec::new();
    wire::Writer::new(&mut raw)
        .write_var_u32(
            wire::PacketHeader {
                id,
                sender_subclient: 2,
                target_subclient: 3,
            }
            .to_raw(),
        )
        .unwrap();
    raw.extend_from_slice(b"opaque unused payload");
    let mut entry = Vec::new();
    wire::Writer::new(&mut entry)
        .write_len_var_u32(raw.len())
        .unwrap();
    entry.extend_from_slice(&raw);
    entry.into()
}

/// Keeps actual known packet headers and a primary terminal around the unused entry.
fn expected() -> [Packet; 3] {
    let mut second = set_time(2);
    second.header.from_subclient = 1;
    second.header.to_subclient = 2;
    [
        set_time(1),
        second,
        Packet::new(packets::Disconnect {
            messages: DisconnectMessages::DisconnectPacketMessages {
                message: "original terminal".into(),
                filtered_message: "original filtered terminal".into(),
            },
            ..Default::default()
        }),
    ]
}

/// Coalesces original encoded known siblings without decoding or rebuilding the injected entry.
fn body(extra: &[u8]) -> Bytes {
    let expected = expected();
    let first = batch_frame(&expected[..1]).unwrap();
    let rest = batch_frame(&expected[1..]).unwrap();
    let mut body = Vec::new();
    body.extend_from_slice(&first[1..]);
    body.extend_from_slice(extra);
    body.extend_from_slice(&rest[1..]);
    body.into()
}

/// Reads the actual shared session frame and joins both original writers before returning outcomes.
async fn consume(body: Bytes) -> Vec<Result<Packet, crate::SessionError>> {
    let (client, peer) = tokio::io::duplex(4096);
    let (client_read, client_write) = tokio::io::split(client);
    let (reader, frames, writer) =
        bridge::tasks::prepare_connection(client_read, client_write, bridge::MAX_FRAME_LEN);
    let mut transport = SessionTransport::new(reader, frames, &[]);
    let (peer_read, peer_write) = tokio::io::split(peer);
    let (peer_reader, peer_frames, peer_writer) =
        bridge::tasks::prepare_connection(peer_read, peer_write, bridge::MAX_FRAME_LEN);
    let writer = tokio::spawn(writer);
    let peer_writer = tokio::spawn(peer_writer);
    let mut frame = vec![bridge::contract::KIND_BATCH];
    frame.extend_from_slice(&body);
    peer_frames.send(frame.into()).await.unwrap();
    drop(peer_frames);
    let mut received = Vec::new();
    for _ in 0..expected().len() {
        let packet = match transport.recv().await {
            Ok(Inbound::Packet(raw)) => raw.decode(),
            Ok(Inbound::Terminal(_)) => Err(SessionFailure::ConnectionClosed.into()),
            Err(error) => Err(error),
        };
        let failed = packet.is_err();
        received.push(packet);
        if failed {
            break;
        }
    }
    drop(transport);
    drop(peer_reader);
    writer.await.expect("original client writer must retire");
    peer_writer.await.expect("original peer writer must retire");
    received
}

#[tokio::test]
async fn unknown_frame_boundary_shared_unknown_keeps_known_fifo_and_terminal() {
    let received = consume(body(&unknown())).await;
    assert_eq!(
        received
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .expect("well-framed unused ID ended the shared session"),
        expected(),
        "unused input lost original known siblings, subclients or terminal",
    );
}

#[tokio::test]
async fn unknown_frame_boundary_shared_known_keeps_fifo_and_terminal() {
    let received = consume(body(&[])).await;
    assert_eq!(
        received.into_iter().collect::<Result<Vec<_>, _>>().unwrap(),
        expected()
    );
}

#[tokio::test]
async fn unknown_frame_boundary_shared_malformed_header_still_rejects() {
    let received = consume(body(&[1, 0x80])).await;
    assert!(
        matches!(
            received.as_slice(),
            [Err(crate::SessionError(SessionFailure::MalformedEntry(_)))]
        ),
        "an incomplete header must remain a framing error"
    );
}

/// Observes actual relay replacement output, then retires both original writers and the relay.
async fn forwarded(replacement: Bytes) -> (Option<Bytes>, bool) {
    let (client, peer) = tokio::io::duplex(4096);
    let (client_read, client_write) = tokio::io::split(client);
    let (reader, frames, writer) =
        bridge::tasks::prepare_connection(client_read, client_write, bridge::MAX_FRAME_LEN);
    let (peer_read, peer_write) = tokio::io::split(peer);
    let (mut peer_reader, peer_frames, peer_writer) =
        bridge::tasks::prepare_connection(peer_read, peer_write, bridge::MAX_FRAME_LEN);
    let relay = Arc::new(FakeRelay::default());
    relay
        .state()
        .outbound_replies
        .push_back(Some(replacement.to_vec()));
    let shared = Arc::new(Shared {
        relay: relay.clone(),
        gate: Arc::new(Mutex::new(())),
        active: AtomicBool::new(true),
    });
    let sender = BatchWriter {
        sender: crate::session_transport::PacketSender::Framed(frames),
        relay: Some(shared),
        tap: None,
    };
    let writer = tokio::spawn(writer);
    let peer_writer = tokio::spawn(peer_writer);
    let sent = sender.send(body(&[])).await;
    let output = if sent.is_ok() {
        peer_reader.next().await.transpose().unwrap()
    } else {
        None
    };
    let disabled = relay.state().stopped;
    drop(sender);
    drop(reader);
    drop(peer_reader);
    drop(peer_frames);
    writer.await.expect("original relay writer must retire");
    peer_writer.await.expect("original peer writer must retire");
    relay.stop();
    sent.expect("original relay send must flush");
    (output, disabled)
}

#[tokio::test]
async fn unknown_frame_boundary_raw_outgoing_keeps_exact_unknown_bytes() {
    let replacement = body(&unknown());
    let (actual, disabled) = forwarded(replacement.clone()).await;
    let mut expected = vec![bridge::contract::KIND_BATCH];
    expected.extend_from_slice(&replacement);
    assert_eq!(
        actual.as_deref(),
        Some(expected.as_slice()),
        "raw relay discarded a well-framed unused packet or changed its exact bytes"
    );
    assert!(!disabled, "valid opaque output disabled the original relay");
}

#[tokio::test]
async fn unknown_frame_boundary_raw_malformed_still_preserves_original() {
    let (actual, disabled) = forwarded(Bytes::from_static(&[1, 0x80])).await;
    let mut expected = vec![bridge::contract::KIND_BATCH];
    expected.extend_from_slice(&body(&[]));
    assert_eq!(actual.as_deref(), Some(expected.as_slice()));
    assert!(
        disabled,
        "malformed relay framing must still disable the relay"
    );
}
