use bedrock_protocol_1_26_51::packets::{PacketId, SetTime};
use bedrock_protocol_1_26_51::wire;

use super::*;

#[cfg(unix)]
pub(crate) mod unix {
    use std::path::PathBuf;

    use bedrock_protocol_1_26_51::packets::{self, ScriptMessage};
    use futures::task::noop_waker;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{UnixListener, UnixStream};

    use super::*;

    /// A transport joined to a peer standing in for the core, in a directory removed afterwards.
    pub(crate) struct Pair {
        pub(crate) transport: SessionTransport,
        pub(crate) peer: UnixStream,
        dir: PathBuf,
    }

    impl Drop for Pair {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(bridge::session_endpoint_path(&self.dir));
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    pub(crate) async fn pair(name: &str, startup: &[Bytes]) -> Pair {
        let dir =
            std::env::temp_dir().join(format!("cinnabar-session-{name}-{}", std::process::id()));
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

    pub(crate) async fn write_frame(peer: &mut UnixStream, frame: &[u8]) {
        peer.write_u32(frame.len() as u32).await.unwrap();
        peer.write_all(frame).await.unwrap();
    }

    /// Two sends cancelled while the peer is stalled both flush on drain, once each, in order.
    #[tokio::test]
    async fn cancelled_sends_behind_a_pending_send_flush_in_order_on_drain() {
        let Pair {
            transport, peer, ..
        } = &mut pair("drain", &[]).await;
        // Too large to flush while the peer is not reading.
        let first = vec![Packet::new(ScriptMessage {
            message_id: "large".into(),
            message_value: vec![1; 16 * 1024 * 1024].into(),
        })];
        let second = vec![Packet::new(SetTime {
            time: wire::ZigZag32(2),
        })];
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        for packets in [&first, &second] {
            let mut send = std::pin::pin!(transport.send(packets));
            assert!(
                send.as_mut().poll(&mut cx).is_pending(),
                "the stalled peer must hold the send"
            );
        }

        let mut expected = Vec::new();
        for packets in [&first, &second] {
            let frame = batch_frame(packets).unwrap();
            expected.extend_from_slice(&(frame.len() as u32).to_be_bytes());
            expected.extend_from_slice(&frame);
        }
        let mut received = vec![0; expected.len()];
        let (drained, read) = tokio::join!(transport.drain_send(), peer.read_exact(&mut received));
        drained.unwrap();
        read.unwrap();
        assert!(
            received == expected,
            "both batches arrive once, in order, as Batch frames"
        );
    }

    /// The startup packets come first; a Batch frame's packets then share the frame's bytes.
    #[tokio::test]
    async fn startup_packets_precede_batches_split_without_copying() {
        let startup = packet_bytes(&Packet::new(SetTime {
            time: wire::ZigZag32(1),
        }));
        let Pair {
            transport, peer, ..
        } = &mut pair("batches", &[startup]).await;
        let batch = [
            Packet::new(SetTime {
                time: wire::ZigZag32(2),
            }),
            Packet::new(SetTime {
                time: wire::ZigZag32(3),
            }),
        ];
        write_frame(peer, &batch_frame(&batch).unwrap()).await;

        let mut times = Vec::new();
        let mut frames = Vec::new();
        for _ in 0..3 {
            let Inbound::Packet(raw) = transport.recv().await.unwrap() else {
                panic!("a packet arrives before any terminal message")
            };
            frames.push(raw.inner_frame().clone());
            let packets::Packet::SetTime(time) = *raw.decode().unwrap().data else {
                panic!("the packets arrive in order")
            };
            times.push(time.time.0);
        }
        assert_eq!(times, [1, 2, 3]);
        assert_eq!(
            frames[2].as_ptr(),
            frames[1].as_ptr().wrapping_add(frames[1].len()),
            "both batch packets slice one frame"
        );
    }

    /// A terminal message arrives as itself, never as a packet, and then the stream ends.
    #[tokio::test]
    async fn terminal_messages_arrive_directly_then_end() {
        for kind in [5u8, 6] {
            let Pair {
                transport, peer, ..
            } = &mut pair(&format!("terminal-{kind}"), &[]).await;
            let body: &[u8] = if kind == 5 {
                br#"{"address":"play.example.test","port":19134,"reload_world":true}"#
            } else {
                br#"{"reason":0,"message":"kicked","filtered_message":"","hide_screen":false}"#
            };
            let mut message = vec![kind];
            message.extend_from_slice(body);
            write_frame(peer, &message).await;

            match transport.recv().await.unwrap() {
                Inbound::Terminal(Terminal::Transfer(transfer)) => assert_eq!(
                    (
                        transfer.address.as_str(),
                        transfer.port,
                        transfer.reload_world
                    ),
                    ("play.example.test", 19134, true)
                ),
                Inbound::Terminal(Terminal::Disconnect(disconnect)) => {
                    assert_eq!(disconnect.message, "kicked");
                }
                Inbound::Packet(raw) => panic!("a terminal message became packet {:?}", raw.id),
            }
            assert!(
                matches!(
                    transport.recv().await,
                    Err(SessionError(SessionFailure::ConnectionClosed))
                ),
                "the session ends after its terminal message"
            );
        }
    }

    /// Frames read ahead come out of `recv` in order, and reading ahead stops at the byte bound
    /// and at a terminal message without consuming anything after it.
    #[tokio::test]
    async fn read_ahead_keeps_order_and_stops_at_its_bound_and_the_terminal() {
        let Pair {
            transport, peer, ..
        } = &mut pair("read-ahead", &[]).await;
        let time = |time| {
            batch_frame(&[Packet::new(SetTime {
                time: wire::ZigZag32(time),
            })])
            .unwrap()
        };
        for frame in [time(1), time(2)] {
            write_frame(peer, &frame).await;
        }
        transport.read_ahead(1).await;
        assert_eq!(transport.ahead.len(), 1, "the bound stops the read-ahead");
        let mut transfer = vec![5u8];
        transfer.extend_from_slice(
            br#"{"address":"play.example.test","port":19134,"reload_world":false}"#,
        );
        write_frame(peer, &transfer).await;
        write_frame(peer, &time(3)).await;
        transport.read_ahead(usize::MAX).await;
        assert_eq!(transport.ahead.len(), 3, "the terminal ends the read-ahead");

        let mut times = Vec::new();
        for _ in 0..2 {
            let Inbound::Packet(raw) = transport.recv().await.unwrap() else {
                panic!("both packets precede the terminal")
            };
            let packets::Packet::SetTime(time) = *raw.decode().unwrap().data else {
                panic!("only the written packets arrive")
            };
            times.push(time.time.0);
        }
        assert_eq!(times, [1, 2]);
        assert!(matches!(
            transport.recv().await.unwrap(),
            Inbound::Terminal(Terminal::Transfer(_))
        ));
        assert_eq!(transport.ahead_bytes, 0);
    }

    /// A single step cannot consume siblings, and another wait shares the current byte charge.
    #[tokio::test]
    async fn read_ahead_steps_stage_one_frame_and_share_the_existing_cap() {
        let Pair {
            transport, peer, ..
        } = &mut pair("read-ahead-step", &[]).await;
        let first = batch_frame(&[Packet::new(SetTime {
            time: wire::ZigZag32(1),
        })])
        .unwrap();
        let second = batch_frame(&[Packet::new(SetTime {
            time: wire::ZigZag32(2),
        })])
        .unwrap();
        write_frame(peer, &first).await;
        write_frame(peer, &second).await;
        assert!(poll_fn(|cx| transport.poll_read_ahead_one(usize::MAX, cx)).await);
        assert_eq!(transport.ahead.len(), 1);
        assert_eq!(transport.ahead_bytes, first.len());
        assert!(!poll_fn(|cx| transport.poll_read_ahead_one(first.len(), cx)).await);
        assert_eq!(
            transport.ahead.len(),
            1,
            "a fresh wait cannot reset the shared charge"
        );
        assert!(!poll_fn(|cx| transport.poll_read_ahead_one(first.len() + 1, cx)).await);
        assert_eq!(transport.ahead.len(), 2);
        assert_eq!(
            transport.ahead_bytes,
            first.len() + second.len(),
            "whole-frame overshoot stays charged"
        );
        for expected in [1, 2] {
            let Inbound::Packet(raw) = transport.recv().await.unwrap() else {
                panic!("packet")
            };
            let packets::Packet::SetTime(time) = *raw.decode().unwrap().data else {
                panic!("time")
            };
            assert_eq!(time.time.0, expected);
        }
        assert_eq!(transport.ahead_bytes, 0);
    }

    /// Cancelling an incomplete frame keeps prior frames and the terminal in their original order.
    #[tokio::test]
    async fn cancelled_partial_read_ahead_preserves_frames_and_the_terminal() {
        let Pair {
            transport, peer, ..
        } = &mut pair("read-ahead-partial", &[]).await;
        let first = batch_frame(&[Packet::new(SetTime {
            time: wire::ZigZag32(1),
        })])
        .unwrap();
        let second = batch_frame(&[Packet::new(SetTime {
            time: wire::ZigZag32(2),
        })])
        .unwrap();
        write_frame(peer, &first).await;
        assert!(poll_fn(|cx| transport.poll_read_ahead_one(usize::MAX, cx)).await);
        peer.write_u32(second.len() as u32).await.unwrap();
        let split = second.len() / 2;
        peer.write_all(&second[..split]).await.unwrap();
        {
            let mut read = std::pin::pin!(transport.read_ahead(usize::MAX));
            let waker = noop_waker();
            assert!(
                read.as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending()
            );
        }
        assert_eq!(transport.ahead.len(), 1);
        assert_eq!(transport.ahead_bytes, first.len());
        peer.write_all(&second[split..]).await.unwrap();
        let mut terminal = vec![bridge::contract::KIND_DISCONNECT];
        terminal.extend_from_slice(
            br#"{"reason":0,"message":"fixture","filtered_message":"","hide_screen":false}"#,
        );
        write_frame(peer, &terminal).await;
        write_frame(peer, &first).await;
        transport.read_ahead(usize::MAX).await;
        assert_eq!(
            transport.ahead.len(),
            3,
            "no frame after the terminal is staged"
        );
        for expected in [1, 2] {
            let Inbound::Packet(raw) = transport.recv().await.unwrap() else {
                panic!("packet")
            };
            let packets::Packet::SetTime(time) = *raw.decode().unwrap().data else {
                panic!("time")
            };
            assert_eq!(time.time.0, expected);
        }
        assert!(matches!(
            transport.recv().await.unwrap(),
            Inbound::Terminal(Terminal::Disconnect(_))
        ));
        assert_eq!(transport.ahead_bytes, 0);
        assert!(matches!(
            transport.recv().await,
            Err(SessionError(SessionFailure::ConnectionClosed))
        ));
    }

    /// A setup message after the handoff is a malformed session, not a packet.
    #[tokio::test]
    async fn setup_messages_after_the_handoff_are_rejected() {
        let Pair {
            transport, peer, ..
        } = &mut pair("setup", &[]).await;
        write_frame(peer, &[4, 0, 0, 0, 0, 1]).await;
        assert!(matches!(
            transport.recv().await,
            Err(SessionError(SessionFailure::Transport(
                BridgeError::InvalidSessionMessage { .. }
            )))
        ));
    }
}

/// One packet's bytes, header included, as a handoff carries it.
pub(crate) fn packet_bytes(packet: &Packet) -> Bytes {
    let entry = batch_frame(std::slice::from_ref(packet))
        .unwrap()
        .slice(1..);
    let mut reader = wire::Reader::new(&entry);
    let length = reader.read_var_u32().unwrap();
    assert_eq!(reader.remaining(), length as usize);
    entry.slice(reader.position()..)
}

/// Startup batches respect both bounds and keep every packet in order.
#[test]
fn startup_batches_stay_within_the_packet_and_byte_bounds() {
    let small: Vec<Bytes> = (0..3_500u32)
        .map(|index| Bytes::from(index.to_le_bytes().to_vec()))
        .collect();
    let large: Vec<Bytes> = (0..5u8)
        .map(|index| Bytes::from(vec![index; 5 << 20]))
        .collect();
    for packets in [small, large] {
        let mut decoded = Vec::new();
        for batch in startup_batches(&packets) {
            assert!(
                batch.len() <= MAX_SESSION_BATCH_BYTES,
                "{} bytes",
                batch.len()
            );
            let mut frame = BytesMut::from(&[bridge::contract::KIND_BATCH][..]);
            frame.extend_from_slice(&batch);
            let CoreMessage::Batch(inner) =
                crate::session_wire::decode_core_message(frame.freeze()).unwrap()
            else {
                unreachable!("a Batch frame decodes as a batch")
            };
            assert!(inner.len() <= MAX_RAW_BATCH_PACKETS);
            decoded.extend(inner);
        }
        assert_eq!(decoded, packets);
    }
}

#[test]
fn startup_packets_keep_their_headers() {
    let packet = Packet::new(SetTime {
        time: wire::ZigZag32(9),
    });
    let bytes = packet_bytes(&packet);
    let batches = startup_batches(&[bytes]);
    let raw = decode_batch_body(batches[0].clone(), MAX_SESSION_BATCH_BYTES)
        .unwrap()
        .remove(0);
    assert_eq!(raw.id, PacketId::SetTime);
    assert_eq!(raw.decode().unwrap(), packet);
}

/// A packet the codec refuses is left out; the rest of the batch still goes, in order.
#[test]
fn an_unencodable_packet_does_not_lose_its_batch() {
    let time = |time| {
        Packet::new(SetTime {
            time: wire::ZigZag32(time),
        })
    };
    // The manifest bounds a LevelChunk at 64 sub-chunks, so the encoder refuses 65.
    let refused = Packet::new(bedrock_protocol_1_26_51::packets::LevelChunk {
        sub_chunks_count: wire::VarUInt(65),
        ..Default::default()
    });
    assert_eq!(
        batch_frame(&[time(1), refused.clone(), time(2)]),
        batch_frame(&[time(1), time(2)])
    );
    assert_eq!(batch_frame(&[refused]), None);
}
