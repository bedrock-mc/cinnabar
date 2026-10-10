#![cfg(unix)]

use std::path::PathBuf;

use futures::task::noop_waker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use valentine::bedrock::version::v1_26_51::McpePacketData;

use super::*;
use crate::BedrockSession;

/// A transport joined to a peer standing in for the core, in a directory removed afterwards.
struct Pair {
    transport: SessionTransport,
    peer: UnixStream,
    dir: PathBuf,
}

impl Drop for Pair {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(bridge::session_endpoint_path(&self.dir));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn pair(name: &str, startup: Vec<Bytes>) -> Pair {
    let dir = std::env::temp_dir().join(format!("cinnabar-session-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let listener = UnixListener::bind(bridge::session_endpoint_path(&dir)).unwrap();
    let (reader, frames) = bridge::connect_session(&dir).await.unwrap();
    let (peer, _) = listener.accept().await.unwrap();
    Pair {
        transport: SessionTransport::new(reader, frames, startup),
        peer,
        dir,
    }
}

async fn write_frame(peer: &mut UnixStream, frame: &[u8]) {
    peer.write_u32(frame.len() as u32).await.unwrap();
    peer.write_all(frame).await.unwrap();
}

async fn receive(
    transport: &mut SessionTransport,
) -> Option<Result<TransportRecvMessage, BridgeError>> {
    std::future::poll_fn(|cx| Pin::new(&mut *transport).poll_recv(cx)).await
}

/// Two sends cancelled while the peer is stalled both flush on drain, once each, in order, as Batch frames.
#[tokio::test]
async fn cancelled_sends_behind_a_pending_send_flush_in_order_on_drain() {
    let Pair {
        transport,
        peer,
        dir,
    } = &mut pair("drain", Vec::new()).await;
    let _ = dir;
    // Too large to flush while the peer is not reading.
    let mut first = vec![1; 16 * 1024 * 1024];
    first[0] = 0xfe;
    let first = Bytes::from(first);
    let second = Bytes::from_static(&[0xfe, 2, 2]);
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    for frame in [&first, &second] {
        let poll =
            Pin::new(&mut *transport).poll_send(&mut cx, TransportMessage::reliable(frame.clone()));
        assert!(poll.is_pending(), "the stalled peer must hold the send");
    }

    let mut received = Vec::new();
    let expected_len: usize = [&first, &second].iter().map(|frame| 4 + frame.len()).sum();
    received.resize(expected_len, 0);
    let drain = std::future::poll_fn(|cx| Pin::new(&mut *transport).poll_drain_send(cx));
    let (drained, read) = tokio::join!(drain, peer.read_exact(&mut received));
    drained.unwrap();
    read.unwrap();

    let mut expected = Vec::new();
    for frame in [&first, &second] {
        expected.extend_from_slice(&(frame.len() as u32).to_be_bytes());
        expected.push(2);
        expected.extend_from_slice(&frame[1..]);
    }
    assert!(
        received == expected,
        "both batches arrive once, in order, as Batch frames"
    );
}

/// The startup batch comes first, then each Batch frame reaches Jolyne as a Bedrock batch sharing its bytes.
#[tokio::test]
async fn batches_reach_jolyne_without_copying() {
    let startup = Bytes::from_static(&[0xfe, 1, 0x0b]);
    let Pair {
        transport, peer, ..
    } = &mut pair("batches", vec![startup.clone()]).await;
    write_frame(peer, &[2, 2, 0x09, 0x00]).await;

    let Some(Ok(TransportRecvMessage::Contiguous(first))) = receive(transport).await else {
        panic!("the startup batch comes first")
    };
    assert_eq!(first, startup);
    let Some(Ok(TransportRecvMessage::SplitFirst { first, rest })) = receive(transport).await
    else {
        panic!("a batch reaches Jolyne split, without copying")
    };
    assert_eq!((first, &rest[..]), (0xfe, &[2, 0x09, 0x00][..]));
}

/// A terminal message arrives as the server packet it stands for, then the stream ends.
#[tokio::test]
async fn terminal_messages_become_server_packets_then_end() {
    for (frame, check) in [
        (
            &br#"{"address":"play.example.test","port":19134,"reload_world":true}"#[..],
            5u8,
        ),
        (
            br#"{"reason":0,"message":"kicked","filtered_message":"","hide_screen":false}"#,
            6,
        ),
    ] {
        let Pair {
            transport, peer, ..
        } = &mut pair(&format!("terminal-{check}"), Vec::new()).await;
        let mut message = vec![check];
        message.extend_from_slice(frame);
        write_frame(peer, &message).await;

        let Some(Ok(TransportRecvMessage::Contiguous(batch))) = receive(transport).await else {
            panic!("a terminal message reaches Jolyne as a batch")
        };
        let packets =
            crate::codec::decode_batch(batch, &BedrockSession { shield_item_id: 0 }).unwrap();
        match (&packets[0].data, check) {
            (McpePacketData::TransferPacket(transfer), 5) => {
                assert_eq!(
                    (
                        transfer.server_address.as_str(),
                        transfer.server_port,
                        transfer.reload_world
                    ),
                    ("play.example.test", 19134, true)
                );
            }
            (McpePacketData::DisconnectPacket(disconnect), 6) => {
                assert_eq!(disconnect.messages.message, "kicked");
            }
            (other, _) => panic!("unexpected packet {:?}", other.packet_id()),
        }
        assert!(
            receive(transport).await.is_none(),
            "the session ends after its terminal message"
        );
    }
}

/// A setup message after the handoff is a malformed session, not a packet.
#[tokio::test]
async fn setup_messages_after_the_handoff_are_rejected() {
    let Pair {
        transport, peer, ..
    } = &mut pair("setup", Vec::new()).await;
    write_frame(peer, &[4, 0, 0, 0, 0, 1]).await;
    assert!(matches!(
        receive(transport).await,
        Some(Err(BridgeError::InvalidSessionMessage { .. }))
    ));
}
