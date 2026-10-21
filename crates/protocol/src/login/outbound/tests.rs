use super::*;
use bedrock_protocol_1_26_51::{packets, wire};
use bytes::Bytes;
use futures::task::noop_waker;
use std::time::Duration;
use tokio::io::{AsyncReadExt, DuplexStream};

type TestSession = PlaySession<bridge::FramedReader<tokio::io::Empty>>;

/// Prepares the real bounded FIFO with an explicitly polled socket writer.
fn session() -> (TestSession, DuplexStream, impl Future<Output = ()>) {
    let (write, peer) = tokio::io::duplex(128 * 1024);
    let (reader, frames, writer) =
        bridge::tasks::prepare_connection(tokio::io::empty(), write, bridge::MAX_FRAME_LEN);
    let transport = crate::session_transport::SessionTransport::new(reader, frames, &[]);
    (
        PlaySession::new(transport, Some(crate::ClientBlobCache::default()), 1, false),
        peer,
        writer,
    )
}

/// Classifies one real cached candidate through the receive-side prefetch owner.
async fn classify(session: &mut TestSession, hash: u64, x: i32) {
    let mut packet = crate::login::tests::cached_level_chunk(vec![hash]);
    packet.chunk_position.x = wire::ZigZag32(x);
    session.pending_blob_prefetch = Some((Packet::new(packet), 32));
    session.finish_blob_prefetch().await.unwrap();
}

/// Polls a future once without giving the writer implicit progress or reading a clock.
fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    let waker = noop_waker();
    future.poll(&mut Context::from_waker(&waker))
}

/// Reads one whole bridge frame already flushed by the explicitly polled writer.
async fn read_frame(peer: &mut DuplexStream) -> Bytes {
    let size = peer.read_u32().await.unwrap();
    let mut frame = vec![0; size as usize];
    peer.read_exact(&mut frame).await.unwrap();
    frame.into()
}

/// Decodes the real protocol batch without assuming a particular encoded packet layout.
fn decode(frame: Bytes) -> Vec<Packet> {
    let body = crate::session_wire::batch_frame_body(&frame).expect("batch frame");
    crate::raw_packet::decode_batch_body(body, crate::session_transport::MAX_SESSION_BATCH_BYTES)
        .unwrap()
        .into_iter()
        .map(|raw| raw.decode().unwrap())
        .collect()
}

/// Produces an ordinary packet whose presence must survive status retirement.
fn ordinary() -> Packet {
    Packet::new(packets::SetTime {
        time: wire::ZigZag32(7),
    })
}

/// Places a decoded raw packet ahead of the fixture's reader through the real ingress FIFO.
fn queue_packet(session: &mut TestSession, packet: Packet) {
    let frame = crate::session_transport::batch_frame(&[packet]).unwrap();
    let body = crate::session_wire::batch_frame_body(&frame).unwrap();
    let raw = crate::raw_packet::decode_batch_body(
        body,
        crate::session_transport::MAX_SESSION_BATCH_BYTES,
    )
    .unwrap();
    session.transport.prepend(raw);
}

#[tokio::test]
async fn ordinary_frames_do_not_drain_and_status_only_opportunities_combine_classifications() {
    let (mut session, mut peer, writer) = session();
    classify(&mut session, 1, 0).await;
    classify(&mut session, 2, 1).await;
    let mut outbound = session.outbound().unwrap();
    let mut writer = std::pin::pin!(writer);
    {
        let mut send = std::pin::pin!(outbound.send_batch(vec![ordinary()], None));
        assert!(poll_once(send.as_mut()).is_pending());
        assert!(poll_once(writer.as_mut()).is_pending());
        assert!(matches!(poll_once(send.as_mut()), Poll::Ready(Ok(()))));
    }
    let frame = decode(read_frame(&mut peer).await);
    assert_eq!(frame.len(), 1);
    assert!(matches!(*frame[0].data, packets::Packet::SetTime(_)));
    {
        let mut send = std::pin::pin!(outbound.send_batch(Vec::new(), Some(Moment::default())));
        assert!(poll_once(send.as_mut()).is_pending());
        assert!(poll_once(writer.as_mut()).is_pending());
        assert!(matches!(poll_once(send.as_mut()), Poll::Ready(Ok(()))));
    }
    let frame = decode(read_frame(&mut peer).await);
    assert_eq!(frame.len(), 1);
    let packets::Packet::ClientCacheBlobStatus(status) = &*frame[0].data else {
        panic!("status")
    };
    assert_eq!(status.missing_ids, [wire::U64LE(1), wire::U64LE(2)]);
    assert!(status.found_ids.is_empty());
    outbound
        .send_batch(
            Vec::new(),
            Some(Moment::from_elapsed(Duration::from_secs(1))),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn reset_before_acceptance_retires_only_status_and_preserves_the_ordinary_batch() {
    let (mut session, mut peer, writer) = session();
    let frames = session.transport.frame_queue();
    let mut fillers = 0;
    loop {
        let mut accept = std::pin::pin!(frames.accept(Bytes::from_static(b"filler")));
        match poll_once(accept.as_mut()) {
            Poll::Ready(Ok(_)) => fillers += 1,
            Poll::Pending => break,
            Poll::Ready(Err(error)) => panic!("{error}"),
        }
    }
    classify(&mut session, 1, 0).await;
    let mut outbound = session.outbound().unwrap();
    {
        let mut send =
            std::pin::pin!(outbound.send_batch(vec![ordinary()], Some(Moment::default())));
        assert!(poll_once(send.as_mut()).is_pending());
    }
    session.reset_blob_cache_pending();
    classify(&mut session, 2, 1).await;
    let mut writer = std::pin::pin!(writer);
    assert!(poll_once(writer.as_mut()).is_pending());
    for _ in 0..fillers {
        assert_eq!(read_frame(&mut peer).await, b"filler"[..]);
    }
    {
        let mut resume = std::pin::pin!(outbound.send_batch(Vec::new(), None));
        assert!(poll_once(resume.as_mut()).is_pending());
        assert!(poll_once(writer.as_mut()).is_pending());
        assert!(matches!(poll_once(resume.as_mut()), Poll::Ready(Ok(()))));
    }
    let frame = decode(read_frame(&mut peer).await);
    assert_eq!(frame.len(), 1);
    assert!(matches!(*frame[0].data, packets::Packet::SetTime(_)));
    {
        let mut send = std::pin::pin!(outbound.send_batch(Vec::new(), Some(Moment::default())));
        assert!(poll_once(send.as_mut()).is_pending());
        assert!(poll_once(writer.as_mut()).is_pending());
        assert!(matches!(poll_once(send.as_mut()), Poll::Ready(Ok(()))));
    }
    let frame = decode(read_frame(&mut peer).await);
    let packets::Packet::ClientCacheBlobStatus(status) = &*frame[0].data else {
        panic!("status")
    };
    assert_eq!(status.missing_ids, [wire::U64LE(2)]);
}

#[cfg(unix)]
#[tokio::test]
async fn reset_before_acceptance_preserves_the_first_relay_replacement() {
    use crate::packet_relay::tests::{FakeRelay, body};
    use crate::session_transport::tests::RecordingTap;
    use std::sync::Arc;

    for replace in [false, true] {
        let (mut session, mut peer, writer) = session();
        let relay = Arc::new(FakeRelay::default());
        if replace {
            relay.state().outbound_replies = [Some(body(&[42])), Some(body(&[99]))].into();
        }
        let tap = Arc::new(RecordingTap::default());
        session.transport = session.transport.with_tap(Some(tap.clone()));
        session.transport.set_relay(relay.clone());
        let frames = session.transport.frame_queue();
        let mut fillers = 0;
        loop {
            let mut accept = std::pin::pin!(frames.accept(Bytes::from_static(b"filler")));
            match poll_once(accept.as_mut()) {
                Poll::Ready(Ok(_)) => fillers += 1,
                Poll::Pending => break,
                Poll::Ready(Err(error)) => panic!("{error}"),
            }
        }
        classify(&mut session, 1, 0).await;
        let mut outbound = session.outbound().unwrap();
        {
            let mut send =
                std::pin::pin!(outbound.send_batch(vec![ordinary()], Some(Moment::default())));
            assert!(poll_once(send.as_mut()).is_pending());
        }
        assert_eq!(relay.state().outbound.len(), 1);
        session.reset_blob_cache_pending();
        // Repeated epoch changes must neither replay the guest nor discard its replacement.
        for _ in 0..2 {
            {
                let mut resume = std::pin::pin!(outbound.send_batch(Vec::new(), None));
                assert!(poll_once(resume.as_mut()).is_pending());
            }
            session.reset_blob_cache_pending();
        }
        assert_eq!(relay.state().outbound.len(), 1);
        let mut writer = std::pin::pin!(writer);
        assert!(poll_once(writer.as_mut()).is_pending());
        for _ in 0..fillers {
            assert_eq!(read_frame(&mut peer).await, b"filler"[..]);
        }
        {
            let mut resume = std::pin::pin!(outbound.send_batch(Vec::new(), None));
            assert!(poll_once(resume.as_mut()).is_pending());
            assert!(poll_once(writer.as_mut()).is_pending());
            assert!(matches!(poll_once(resume.as_mut()), Poll::Ready(Ok(()))));
        }
        let packets = decode(read_frame(&mut peer).await);
        assert_eq!(packets.len(), 1);
        let expected = if replace { 42 } else { 7 };
        assert!(
            matches!(&*packets[0].data, packets::Packet::SetTime(time) if time.time.0 == expected)
        );
        assert_eq!(relay.state().outbound.len(), 1);
        assert_eq!(tap.events().len(), 1);
    }
}

#[tokio::test]
async fn cancelled_accepted_batches_flush_once_and_later_classifications_follow_the_epoch() {
    for rotate in [false, true] {
        let (mut session, mut peer, writer) = session();
        classify(&mut session, 1, 0).await;
        let mut outbound = session.outbound().unwrap();
        {
            let mut send =
                std::pin::pin!(outbound.send_batch(vec![ordinary()], Some(Moment::default())));
            assert!(poll_once(send.as_mut()).is_pending());
        }
        classify(&mut session, 9, 1).await;
        if rotate {
            session.reset_blob_cache_pending();
        }
        classify(&mut session, 2, 2).await;
        let mut writer = std::pin::pin!(writer);
        assert!(poll_once(writer.as_mut()).is_pending());
        outbound.send_batch(Vec::new(), None).await.unwrap();
        let frame = decode(read_frame(&mut peer).await);
        assert_eq!(frame.len(), 2);
        assert!(matches!(*frame[0].data, packets::Packet::SetTime(_)));
        let packets::Packet::ClientCacheBlobStatus(status) = &*frame[1].data else {
            panic!("status")
        };
        assert_eq!(status.missing_ids, [wire::U64LE(1)]);
        {
            let mut send = std::pin::pin!(outbound.send_batch(
                Vec::new(),
                Some(Moment::from_elapsed(Duration::from_secs(1)))
            ));
            assert!(poll_once(send.as_mut()).is_pending());
            assert!(poll_once(writer.as_mut()).is_pending());
            assert!(matches!(poll_once(send.as_mut()), Poll::Ready(Ok(()))));
        }
        let frame = decode(read_frame(&mut peer).await);
        assert_eq!(frame.len(), 1);
        let packets::Packet::ClientCacheBlobStatus(status) = &*frame[0].data else {
            panic!("status")
        };
        let expected = if rotate {
            vec![wire::U64LE(2)]
        } else {
            vec![wire::U64LE(9), wire::U64LE(2)]
        };
        assert_eq!(status.missing_ids, expected);
        outbound.send_batch(Vec::new(), None).await.unwrap();
    }
}

#[tokio::test]
async fn cancelled_accepted_write_reports_writer_failure_on_resume() {
    let (mut session, _peer, writer) = session();
    classify(&mut session, 1, 0).await;
    let mut outbound = session.outbound().unwrap();
    {
        let mut send =
            std::pin::pin!(outbound.send_batch(vec![ordinary()], Some(Moment::default())));
        assert!(poll_once(send.as_mut()).is_pending());
    }
    drop(writer);
    assert!(outbound.send_batch(Vec::new(), None).await.is_err());
}

#[tokio::test]
async fn full_status_intake_keeps_the_next_prefetch_unclassified_across_cancellation() {
    let (mut session, _peer, _writer) = session();
    let cache = session.blob_cache.as_ref().unwrap().cache().clone();
    let hit = cache.insert(b"ready").unwrap();
    let hashes: Vec<_> = (0..crate::MAX_CLIENT_BLOB_HASHES_PER_PACKET as u64).collect();
    assert!(!hashes.contains(&hit));
    let seed = crate::BlobCacheResolver::new(cache.clone())
        .accept_cached_packet(Packet::new(crate::login::tests::cached_level_chunk(
            hashes.clone(),
        )))
        .unwrap();
    for _ in (0..crate::MAX_CLIENT_BLOB_PENDING_BYTES / size_of::<u64>()).step_by(hashes.len()) {
        session.cache_status.lock().unwrap().push(seed.clone());
    }
    session.pending_blob_prefetch = Some((
        Packet::new(crate::login::tests::cached_level_chunk(vec![hit])),
        32,
    ));
    for _ in 0..2 {
        let mut receive = std::pin::pin!(session.recv_world_ingress_with_blob_cache(0));
        assert!(poll_once(receive.as_mut()).is_pending());
    }
    assert_eq!(session.blob_cache_stats().hashes_classified, 0);
    let selected = session
        .cache_status
        .lock()
        .unwrap()
        .select(Moment::default())
        .unwrap();
    assert_eq!(
        selected.missing_ids,
        hashes.into_iter().map(wire::U64LE).collect::<Vec<_>>()
    );
    {
        let mut receive = std::pin::pin!(session.recv_world_ingress_with_blob_cache(0));
        assert!(matches!(
            poll_once(receive.as_mut()),
            Poll::Ready(Ok(crate::login::WorldIngress::LevelChunk(_, _)))
        ));
    }
    assert_eq!(session.blob_cache_stats().hashes_classified, 1);
}

#[tokio::test]
async fn fast_transfer_arming_preserves_hits_and_confirmed_candidates_retire_them() {
    for candidate_cached in [None, Some(false), Some(true)] {
        let (mut session, mut peer, writer) = session();
        let cache = session.blob_cache.as_ref().unwrap().cache().clone();
        let old = cache.insert(b"old").unwrap();
        let new = cache.insert(b"new").unwrap();
        classify(&mut session, old, 0).await;
        assert!(session.blob_cache.as_mut().unwrap().pop_ready().is_some());
        assert_eq!(session.blob_cache_stats().pending_transactions, 0);
        session.arm_blob_cache_reset_for_fast_transfer();
        if let Some(cached) = candidate_cached {
            let mut candidate = crate::login::tests::cached_level_chunk(vec![new]);
            candidate.chunk_position.x = wire::ZigZag32(1);
            candidate.cache_enabled = cached;
            queue_packet(&mut session, Packet::new(candidate));
            let mut receive = std::pin::pin!(session.recv_world_ingress_with_blob_cache(0));
            assert!(matches!(
                poll_once(receive.as_mut()),
                Poll::Ready(Ok(crate::login::WorldIngress::LevelChunk(_, _)))
            ));
        }
        let mut outbound = session.outbound().unwrap();
        let mut writer = std::pin::pin!(writer);
        let mut send = std::pin::pin!(outbound.send_batch(Vec::new(), Some(Moment::default())));
        if candidate_cached == Some(false) {
            assert!(matches!(poll_once(send.as_mut()), Poll::Ready(Ok(()))));
        } else {
            assert!(poll_once(send.as_mut()).is_pending());
            assert!(poll_once(writer.as_mut()).is_pending());
            assert!(matches!(poll_once(send.as_mut()), Poll::Ready(Ok(()))));
            let frame = decode(read_frame(&mut peer).await);
            let packets::Packet::ClientCacheBlobStatus(status) = &*frame[0].data else {
                panic!("status")
            };
            assert!(status.missing_ids.is_empty());
            assert_eq!(
                status.found_ids,
                [wire::U64LE(if candidate_cached.is_some() {
                    new
                } else {
                    old
                })]
            );
        }
    }
}

#[tokio::test]
async fn dimension_and_terminal_boundaries_retire_queued_hit_only_status() {
    for terminal in [false, true] {
        let (mut session, _peer, _writer) = session();
        let hash = session
            .blob_cache
            .as_ref()
            .unwrap()
            .cache()
            .insert(b"hit")
            .unwrap();
        classify(&mut session, hash, 0).await;
        assert!(session.blob_cache.as_mut().unwrap().pop_ready().is_some());
        if terminal {
            assert!(
                session
                    .absorb_terminal(crate::session_transport::Terminal::Disconnect(
                        crate::session_wire::SessionDisconnect {
                            reason: 0,
                            message: String::new(),
                            filtered_message: String::new(),
                            hide_screen: true,
                        }
                    ))
                    .unwrap()
            );
        } else {
            queue_packet(
                &mut session,
                Packet::new(packets::ChangeDimension::default()),
            );
            let mut receive = std::pin::pin!(session.recv_world_ingress_with_blob_cache(0));
            assert!(matches!(
                poll_once(receive.as_mut()),
                Poll::Ready(Ok(crate::login::WorldIngress::Event(
                    crate::WorldEvent::ChangeDimension(_)
                )))
            ));
        }
        let mut outbound = session.outbound().unwrap();
        let mut send = std::pin::pin!(outbound.send_batch(Vec::new(), Some(Moment::default())));
        assert!(matches!(poll_once(send.as_mut()), Poll::Ready(Ok(()))));
    }
}

/// Detached sends must retain the reactor replacement and its single flush across cancellation.
#[cfg(unix)]
#[tokio::test]
async fn relay_replacements_survive_cancelled_outbound_flushes() {
    use crate::packet_relay::tests::{FakeRelay, body};
    use crate::session_transport::tests::RecordingTap;
    use std::sync::Arc;
    let (mut session, mut peer, writer) = session();
    let relay = Arc::new(FakeRelay::default());
    relay.state().outbound_replies.push_back(Some(body(&[42])));
    let tap = Arc::new(RecordingTap::default());
    session.transport = session.transport.with_tap(Some(tap.clone()));
    session.transport.set_relay(relay.clone());
    let mut outbound = session.outbound().unwrap();
    {
        let mut send = std::pin::pin!(outbound.send_batch(vec![ordinary()], None));
        assert!(poll_once(send.as_mut()).is_pending());
    }
    assert_eq!(relay.state().outbound.len(), 1);
    let mut writer = std::pin::pin!(writer);
    assert!(poll_once(writer.as_mut()).is_pending());
    outbound.send_batch(Vec::new(), None).await.unwrap();
    let packets = decode(read_frame(&mut peer).await);
    assert!(matches!(&*packets[0].data, packets::Packet::SetTime(time) if time.time.0 == 42));
    assert_eq!(relay.state().outbound.len(), 1);
    assert_eq!(
        tap.events().len(),
        1,
        "only the accepted replacement is recorded"
    );
}
