use bedrock_protocol_1_26_51::packets::{PacketId, SetTime};
use bedrock_protocol_1_26_51::wire;

use super::*;

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
mod native_terminal;

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
impl<R: bridge::FrameRead> SessionTransport<R> {
    /// Observes actual native read-ahead commitment without consuming its accepted FIFO.
    pub(crate) fn has_retained_native_batch(&self) -> bool {
        self.ahead
            .iter()
            .any(|entry| matches!(entry.item.as_ref(), Some(Ok(ReadItem::Native(_)))))
    }

    /// Observes a terminal already split by the original startup reader, without removing it.
    pub(crate) fn has_retained_native_terminal(&self) -> bool {
        self.received
            .iter()
            .any(|packet| matches!(packet.id, PacketId::Disconnect | PacketId::Transfer))
    }
}

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
        tapped_pair(name, startup, None).await
    }

    /// [`pair`] whose transport reports to `tap`.
    pub(crate) async fn tapped_pair(
        name: &str,
        startup: &[Bytes],
        tap: Option<Arc<dyn PacketTap>>,
    ) -> Pair {
        let dir =
            std::env::temp_dir().join(format!("cinnabar-session-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(bridge::session_endpoint_path(&dir)).unwrap();
        let (reader, frames) = bridge::connect_session(&dir).await.unwrap();
        let (peer, _) = listener.accept().await.unwrap();
        Pair {
            transport: SessionTransport::new(reader, frames, startup).with_tap(tap),
            peer,
            dir,
        }
    }

    pub(crate) async fn write_frame(peer: &mut UnixStream, frame: &[u8]) {
        peer.write_u32(frame.len() as u32).await.unwrap();
        peer.write_all(frame).await.unwrap();
    }

    #[tokio::test]
    async fn relay_injections_progress_while_upstream_frames_are_ready() {
        use crate::packet_relay::tests::{FakeRelay, body};
        let mut pair = pair("fair-injections", &[]).await;
        pair.transport.set_relay(Arc::new(FakeRelay::default()));
        let (send, receive) = tokio::sync::mpsc::channel(1);
        pair.transport.relay.as_mut().unwrap().injected = receive;
        send.send(Bytes::from(body(&[99]))).await.unwrap();
        let frame = batch_frame(&[Packet::new(SetTime {
            time: wire::ZigZag32(1),
        })])
        .unwrap();
        for _ in 0..4 {
            write_frame(&mut pair.peer, &frame).await;
            assert!(poll_fn(|cx| pair.transport.poll_read_ahead_one(usize::MAX, cx)).await);
        }
        let mut times = Vec::new();
        for _ in 0..2 {
            let Inbound::Packet(raw) = pair.transport.recv().await.unwrap() else {
                panic!("expected a packet");
            };
            let packets::Packet::SetTime(time) = *raw.decode().unwrap().data else {
                panic!("expected SetTime");
            };
            times.push(time.time.0);
        }
        assert!(times.contains(&1), "upstream must progress");
        assert!(times.contains(&99), "injections must progress");
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
        let ahead_bytes = transport.ahead_bytes;
        assert!(matches!(
            transport.buffered_startup_terminal().unwrap(),
            Some(Terminal::Transfer(_))
        ));
        assert_eq!(transport.ahead.len(), 3);
        assert_eq!(transport.ahead_bytes, ahead_bytes);

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

    /// Each delivered packet names the core batch frame and slot it came from, counting the
    /// startup packets as frame 0, so trace joins line up with the core's frame numbers.
    #[tokio::test]
    async fn delivered_packets_name_their_frame_and_index() {
        let time = |time| {
            Packet::new(SetTime {
                time: wire::ZigZag32(time),
            })
        };
        let Pair {
            transport, peer, ..
        } = &mut pair("trace-position", &[packet_bytes(&time(0))]).await;
        write_frame(peer, &batch_frame(&[time(1), time(2)]).unwrap()).await;
        write_frame(peer, &batch_frame(&[time(3)]).unwrap()).await;

        let mut positions = Vec::new();
        for _ in 0..4 {
            let Inbound::Packet(_) = transport.recv().await.unwrap() else {
                panic!("only packets were written")
            };
            let (_, frame, index) = transport.trace_position();
            positions.push((frame, index));
        }
        assert_eq!(positions, [(0, 0), (1, 0), (1, 1), (2, 0)]);
    }

    /// Packets deferred and prepended at startup keep the frame and slot they arrived in.
    #[tokio::test]
    async fn prepended_packets_keep_their_trace_position() {
        let time = |time| {
            Packet::new(SetTime {
                time: wire::ZigZag32(time),
            })
        };
        let Pair {
            transport, peer, ..
        } = &mut pair("trace-prepend", &[]).await;
        write_frame(peer, &batch_frame(&[time(1), time(2)]).unwrap()).await;
        write_frame(peer, &batch_frame(&[time(3)]).unwrap()).await;

        let mut deferred = Vec::new();
        for _ in 0..2 {
            let Inbound::Packet(raw) = transport.recv().await.unwrap() else {
                panic!("only packets were written")
            };
            deferred.push(raw);
        }
        let Inbound::Packet(_) = transport.recv().await.unwrap() else {
            panic!("only packets were written")
        };
        assert_eq!(transport.trace_position().1, 2);
        transport.prepend(deferred);
        let mut positions = Vec::new();
        for _ in 0..2 {
            transport.recv().await.unwrap();
            let (_, frame, index) = transport.trace_position();
            positions.push((frame, index));
        }
        assert_eq!(positions, [(1, 0), (1, 1)]);
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

    /// The tap sees each packet once, in order, in both directions; an outcome names only the
    /// packet the session is processing, and a prepended packet is not recorded again.
    #[tokio::test]
    async fn the_tap_records_each_packet_once_and_outcomes_name_the_current_packet() {
        let time = |time| {
            Packet::new(SetTime {
                time: wire::ZigZag32(time),
            })
        };
        let body = |packet: &Packet| packet_bytes(packet).slice(1..).to_vec();
        let tap = Arc::new(RecordingTap::default());
        let Pair {
            transport, peer, ..
        } = &mut tapped_pair("tap", &[packet_bytes(&time(1))], Some(tap.clone())).await;
        write_frame(peer, &batch_frame(&[time(2), time(3)]).unwrap()).await;

        next(transport).await;
        transport.reject_current(PacketOutcome::Skipped);
        transport.reject_current(PacketOutcome::Failed);
        let second = next(transport).await;
        next(transport).await;
        transport.reject_current(PacketOutcome::Accepted);
        transport.prepend(vec![second.into_compact()]);
        next(transport).await;
        transport.reject_current(PacketOutcome::Failed);
        transport.send(&[time(4)]).await.unwrap();

        let set_time = PacketId::SetTime as u32;
        assert_eq!(
            tap.events(),
            [
                TapEvent::Record(PacketDirection::Clientbound, set_time, body(&time(1))),
                TapEvent::Reject(0, set_time, PacketOutcome::Skipped, body(&time(1))),
                TapEvent::Record(PacketDirection::Clientbound, set_time, body(&time(2))),
                TapEvent::Record(PacketDirection::Clientbound, set_time, body(&time(3))),
                TapEvent::Reject(3, set_time, PacketOutcome::Accepted, body(&time(3))),
                TapEvent::Reject(2, set_time, PacketOutcome::Failed, body(&time(2))),
                TapEvent::Record(PacketDirection::Serverbound, set_time, body(&time(4))),
            ]
        );
    }

    /// A detached in-flight packet must not keep unrelated batch siblings through its observer.
    #[tokio::test]
    async fn detaching_the_current_packet_releases_its_old_batch_before_settlement() {
        let time = |value| {
            Packet::new(SetTime {
                time: wire::ZigZag32(value),
            })
        };
        let tap = Arc::new(RecordingTap::default());
        let Pair { transport, .. } = &mut tapped_pair(
            "compact-current",
            &[packet_bytes(&time(1)), packet_bytes(&time(2))],
            Some(tap.clone()),
        )
        .await;
        drop(next(transport).await);
        let raw = next(transport).await;
        let backing = raw.body().clone();
        let raw = raw.into_retention_bounded();
        transport.compact_current(&raw);
        assert!(backing.is_unique(), "the observer kept the unrelated batch");
        let expected = raw.body().to_vec();
        drop(raw);
        transport.reject_current(PacketOutcome::Failed);
        assert!(matches!(tap.events().last(), Some(TapEvent::Reject(
            _, _, PacketOutcome::Failed, body,
        )) if body == &expected));
    }

    async fn next(transport: &mut SessionTransport) -> RawPacket {
        match transport.recv().await.unwrap() {
            Inbound::Packet(raw) => raw,
            Inbound::Terminal(_) => panic!("a packet arrives before any terminal message"),
        }
    }

    /// A batch that cannot be split reaches the tap as one failed batch, not as packets.
    #[tokio::test]
    async fn an_unsplittable_batch_is_reported_once() {
        let tap = Arc::new(RecordingTap::default());
        let Pair {
            transport, peer, ..
        } = &mut tapped_pair("tap-batch", &[], Some(tap.clone())).await;
        // The entry declares five bytes but carries one.
        write_frame(peer, &[bridge::contract::KIND_BATCH, 5, 1]).await;
        assert!(transport.recv().await.is_err());
        assert_eq!(tap.events(), [TapEvent::RejectBatch(2)]);
    }
}

/// What a [`RecordingTap`] saw; a record's handle is its index in the event list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TapEvent {
    Record(PacketDirection, u32, Vec<u8>),
    Reject(u64, u32, PacketOutcome, Vec<u8>),
    RejectBatch(usize),
}

/// Keeps every tap call for inspection.
#[derive(Default)]
pub(crate) struct RecordingTap(Mutex<Vec<TapEvent>>);

impl RecordingTap {
    pub(crate) fn events(&self) -> Vec<TapEvent> {
        self.lock().clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<TapEvent>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl PacketTap for RecordingTap {
    fn record(
        &self,
        direction: PacketDirection,
        packet_id: u32,
        body: PacketBody<'_>,
    ) -> PacketRecordId {
        let mut events = self.lock();
        events.push(TapEvent::Record(
            direction,
            packet_id,
            body.as_slice().to_vec(),
        ));
        PacketRecordId(events.len() as u64 - 1)
    }

    fn reject(
        &self,
        record: PacketRecordId,
        packet_id: u32,
        outcome: PacketOutcome,
        body: PacketBody<'_>,
    ) {
        self.lock().push(TapEvent::Reject(
            record.0,
            packet_id,
            outcome,
            body.as_slice().to_vec(),
        ));
    }

    fn reject_batch(&self, len: usize) {
        self.lock().push(TapEvent::RejectBatch(len));
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
