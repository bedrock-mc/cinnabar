use super::*;
use crate::LoginSequence;
use futures::{FutureExt, StreamExt};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

/// Keeps deadline control with the test host without requiring a timer runtime.
fn pending_timers() -> bridge::tasks::Timers {
    bridge::tasks::Timers(|_| Box::pin(futures::future::pending()))
}

/// Declares one web archive whose hash covers the compressed bytes.
fn web_handoff(data: &[u8]) -> SessionHandoff {
    let mut handoff = handoff_with_sizes(&[data.len() as u64]);
    handoff.pack_stream = Some(9);
    handoff.packs[0].sha256 = Some(Sha256::digest(data).into());
    handoff.startup = vec![Bytes::from_static(&[0x0b])];
    handoff
}

/// Encodes a join prefix and the same indexed PackData framing used by Go.
fn pack_stream(join: u64, data: &[u8]) -> Vec<u8> {
    let frame = encode_core_message(&CoreMessage::PackData {
        index: 0,
        data: Bytes::copy_from_slice(data),
    })
    .unwrap();
    let mut wire = join.to_be_bytes().to_vec();
    wire.extend_from_slice(&(frame.len() as u32).to_be_bytes());
    wire.extend_from_slice(&frame);
    wire
}

#[test]
fn web_pack_receiver_checks_digest_admission_and_chunks() {
    let handoff = web_handoff(b"abc");
    let mut receiver = HandoffPackReceiver::web(&handoff).unwrap();
    receiver.accept(0, b"a").unwrap();
    assert!(!receiver.is_complete());
    receiver.accept(0, b"bc").unwrap();
    assert_eq!(receiver.into_archives().unwrap(), [b"abc".to_vec()]);
    let mut receiver = HandoffPackReceiver::web(&handoff).unwrap();
    assert!(receiver.accept(0, b"abd").is_err());
    assert!(receiver.into_archives().is_err());
    let mut missing = web_handoff(b"abc");
    missing.packs[0].sha256 = None;
    assert!(HandoffPackReceiver::web(&missing).is_err());
}

#[test]
fn bulk_pack_stream_rejects_stale_truncated_extra_and_corrupt_bytes() {
    futures::executor::block_on(async {
        let good = pack_stream(9, b"abc");
        for wire in [
            pack_stream(8, b"abc"),
            pack_stream(9, b"abd"),
            good[..good.len() - 1].to_vec(),
            [good.clone(), good[8..].to_vec()].concat(),
        ] {
            let mut receiver = HandoffPackReceiver::web(&web_handoff(b"abc")).unwrap();
            assert!(receiver.receive_stream(9, wire.as_slice()).await.is_err());
        }
        let mut receiver = HandoffPackReceiver::web(&web_handoff(b"abc")).unwrap();
        let (read, mut write) = tokio::io::duplex(2);
        let (received, sent) = futures::join!(receiver.receive_stream(9, read), async {
            write.write_all(&good).await.unwrap();
            write.shutdown().await
        });
        received.unwrap();
        sent.unwrap();
        assert_eq!(receiver.into_archives().unwrap(), [b"abc".to_vec()]);
    });
}

#[test]
fn bulk_pack_completion_requires_fin() {
    let mut receiver = HandoffPackReceiver::web(&web_handoff(b"abc")).unwrap();
    let (read, mut write) = tokio::io::duplex(64);
    futures::executor::block_on(write.write_all(&pack_stream(9, b"abc"))).unwrap();
    let mut receiving = Box::pin(receiver.receive_stream(9, read));
    assert!(receiving.as_mut().now_or_never().is_none());
    drop(write);
    futures::executor::block_on(receiving).unwrap();
    assert!(receiver.is_complete());
}

#[test]
fn web_handoff_preserves_play_batches_and_inline_fallback() {
    futures::executor::block_on(async {
        for bulk in [true, false] {
            let mut handoff = web_handoff(b"abc");
            if !bulk {
                handoff.pack_stream = None;
            }
            let first = encode_core_message(&CoreMessage::Handoff(handoff)).unwrap();
            let batch = encode_batch([b"\x09\x00"]).unwrap();
            let inline = encode_core_message(&CoreMessage::PackData {
                index: 0,
                data: Bytes::from_static(b"abc"),
            })
            .unwrap();
            let frames = if bulk {
                vec![Ok(first), Ok(batch.clone())]
            } else {
                vec![Ok(first), Ok(inline), Ok(batch.clone())]
            };
            let reader = futures::stream::iter(frames).chain(futures::stream::pending());
            let wire = pack_stream(9, b"abc");
            let (_, archives, mut reader) = LoginSequence::receive_web_handoff(
                reader,
                9,
                async { Ok(bulk.then_some(wire.as_slice())) },
                pending_timers(),
            )
            .await
            .unwrap();
            assert_eq!(archives, [b"abc".to_vec()]);
            assert_eq!(reader.next().await.unwrap().unwrap(), batch);
        }
    });
}

#[test]
fn bulk_handoff_ready_archives_precede_ready_play_batches() {
    futures::executor::block_on(async {
        for batch in [encode_batch([b"\x09\x00"]).unwrap(), full_budget_batch()] {
            let handoff = encode_core_message(&CoreMessage::Handoff(web_handoff(b"abc"))).unwrap();
            let count = 32;
            let frames = std::iter::once(Ok(handoff)).chain((0..count).map(|_| Ok(batch.clone())));
            let reader = futures::stream::iter(frames).chain(futures::stream::pending());
            let wire = pack_stream(9, b"abc");
            let (_, archives, mut reader) = LoginSequence::receive_web_handoff(
                reader,
                9,
                async { Ok(Some(wire.as_slice())) },
                pending_timers(),
            )
            .now_or_never()
            .expect("ready archives must complete despite ready gameplay")
            .expect("ready gameplay must not reject completed archive delivery");
            assert_eq!(archives, [b"abc".to_vec()]);
            for _ in 0..count {
                assert_eq!(reader.next().await.unwrap().unwrap(), batch);
            }
        }
    });
}

#[test]
fn bulk_handoff_replays_many_play_batches_while_archives_are_pending() {
    futures::executor::block_on(async {
        let handoff = encode_core_message(&CoreMessage::Handoff(web_handoff(b"abc"))).unwrap();
        let batch = encode_batch([b"\x09\x00"]).unwrap();
        let count = 32;
        let frames = std::iter::once(Ok(handoff)).chain((0..count).map(|_| Ok(batch.clone())));
        let reader = futures::stream::iter(frames).chain(futures::stream::pending());
        let wire = pack_stream(9, b"abc");
        let (ready, packs) = futures::channel::oneshot::channel();
        let receiving = LoginSequence::receive_web_handoff(
            reader,
            9,
            async {
                packs.await.unwrap();
                Ok(Some(wire.as_slice()))
            },
            pending_timers(),
        );
        tokio::pin!(receiving);
        assert!(receiving.as_mut().now_or_never().is_none());
        ready.send(()).unwrap();
        let (_, archives, mut reader) = receiving.await.unwrap();
        assert_eq!(archives, [b"abc".to_vec()]);
        for _ in 0..count {
            assert_eq!(reader.next().await.unwrap().unwrap(), batch);
        }
    });
}

#[test]
fn terminal_play_message_cancels_pending_bulk_open() {
    futures::executor::block_on(async {
        let handoff = encode_core_message(&CoreMessage::Handoff(web_handoff(b"abc"))).unwrap();
        let terminal = encode_core_message(&CoreMessage::Disconnect(SessionDisconnect {
            reason: 0,
            message: "closed".into(),
            filtered_message: String::new(),
            hide_screen: false,
        }))
        .unwrap();
        let reader =
            futures::stream::iter([Ok(handoff), Ok(terminal)]).chain(futures::stream::pending());
        let result = LoginSequence::receive_web_handoff::<_, &[u8]>(
            reader,
            9,
            futures::future::pending(),
            pending_timers(),
        )
        .await;
        assert!(result.is_err());
    });
}

/// Returns one play batch large enough to fill the shared read-ahead byte budget.
fn full_budget_batch() -> Bytes {
    encode_batch([vec![9; crate::session_transport::MAX_READ_AHEAD_BYTES]]).unwrap()
}

/// Fills the play byte budget and retains an archive peer to observe reader cancellation.
fn full_bulk_queue(
    pending_open: bool,
) -> (
    futures::channel::mpsc::UnboundedSender<Result<Bytes, BridgeError>>,
    impl std::future::Future<Output = crate::ProtocolError>,
    tokio::io::DuplexStream,
) {
    let (send, reader) = futures::channel::mpsc::unbounded();
    let handoff = encode_core_message(&CoreMessage::Handoff(web_handoff(b"abc"))).unwrap();
    send.unbounded_send(Ok(handoff)).unwrap();
    send.unbounded_send(Ok(full_budget_batch())).unwrap();
    let (read, write) = tokio::io::duplex(1);
    let receiving = async move {
        LoginSequence::receive_web_handoff(
            reader,
            9,
            async move {
                if pending_open {
                    futures::future::pending::<()>().await;
                }
                Ok(Some(read))
            },
            pending_timers(),
        )
        .await
        .err()
        .expect("a terminal or overflow must end the join")
    };
    (send, receiving, write)
}

#[test]
fn bulk_handoff_full_queue_preserves_terminal_messages() {
    for pending_open in [false, true] {
        for terminal in [
            CoreMessage::Disconnect(SessionDisconnect {
                reason: 0,
                message: "closed during pack delivery".into(),
                filtered_message: "filtered disconnect text".into(),
                hide_screen: false,
            }),
            CoreMessage::Transfer(SessionTransfer {
                address: "next.example.test".into(),
                port: 19133,
                reload_world: true,
            }),
        ] {
            let (send, receiving, mut archive) = full_bulk_queue(pending_open);
            tokio::pin!(receiving);
            assert!(receiving.as_mut().now_or_never().is_none());
            send.unbounded_send(Ok(encode_core_message(&terminal).unwrap()))
                .unwrap();
            let error = receiving
                .now_or_never()
                .expect("a full play queue must not hide terminal messages");
            match terminal {
                CoreMessage::Disconnect(expected) => {
                    let actual = error.server_disconnect().expect("retain the server reason");
                    assert_eq!(actual.message.as_deref(), Some(expected.message.as_str()));
                    assert_eq!(
                        actual.filtered_message.as_deref(),
                        Some(expected.filtered_message.as_str())
                    );
                }
                CoreMessage::Transfer(expected) => {
                    let actual = error.server_transfer().expect("retain the transfer target");
                    assert_eq!(actual.host, expected.address);
                    assert_eq!(actual.port, expected.port);
                    assert_eq!(actual.reload_world, expected.reload_world);
                }
                _ => unreachable!(),
            }
            assert!(
                archive.write_all(b"x").now_or_never().unwrap().is_err(),
                "release the pending archive lane"
            );
            assert!(send.is_closed(), "release the play lane");
        }
    }
}

#[test]
fn bulk_handoff_full_queue_rejects_nonterminal_overflow() {
    for pending_open in [false, true] {
        for message in [
            encode_batch([b"\x09\x00"]).unwrap(),
            encode_core_message(&CoreMessage::PackData {
                index: 0,
                data: Bytes::from_static(b"abc"),
            })
            .unwrap(),
        ] {
            let (send, receiving, mut archive) = full_bulk_queue(pending_open);
            tokio::pin!(receiving);
            assert!(receiving.as_mut().now_or_never().is_none());
            send.unbounded_send(Ok(message)).unwrap();
            let error = receiving
                .now_or_never()
                .expect("overflow must fail without waiting for the archive");
            assert!(matches!(
                error,
                crate::ProtocolError::HandoffReadAheadOverflow
            ));
            assert!(archive.write_all(b"x").now_or_never().unwrap().is_err());
            assert!(send.is_closed());
        }
    }
}

#[test]
fn bulk_handoff_full_queue_observes_relay_cancellation_before_archive_publication() {
    let (send, receiving, mut archive) = full_bulk_queue(true);
    tokio::pin!(receiving);
    assert!(receiving.as_mut().now_or_never().is_none());
    // Cancelling the relay's archive open closes play without ever publishing an archive.
    drop(send);
    let error = receiving
        .now_or_never()
        .expect("relay closure must end an unpublished archive wait");
    assert!(
        error
            .to_string()
            .contains("play closed during pack delivery")
    );
    assert!(archive.write_all(b"x").now_or_never().unwrap().is_err());
}

#[test]
fn bulk_handoff_replays_batches_crossing_the_byte_budget() {
    futures::executor::block_on(async {
        let handoff = encode_core_message(&CoreMessage::Handoff(web_handoff(b"abc"))).unwrap();
        let budget = crate::session_transport::MAX_READ_AHEAD_BYTES;
        let batches: Vec<_> = [budget * 5 / 8, budget * 15 / 32]
            .into_iter()
            .map(|size| encode_batch([vec![9; size]]).unwrap())
            .collect();
        let polls = std::cell::Cell::new(0);
        let frames = std::iter::once(Ok(handoff)).chain(batches.iter().cloned().map(Ok));
        let (send, extra) = futures::channel::mpsc::unbounded();
        let reader = futures::stream::iter(frames)
            .inspect(|_| polls.set(polls.get() + 1))
            .chain(extra);
        let (ready, packs) = futures::channel::oneshot::channel();
        let wire = pack_stream(9, b"abc");
        let receiving = LoginSequence::receive_web_handoff(
            reader,
            9,
            async {
                packs.await.unwrap();
                Ok(Some(wire.as_slice()))
            },
            pending_timers(),
        );
        tokio::pin!(receiving);
        assert!(receiving.as_mut().now_or_never().is_none());
        assert_eq!(
            polls.get(),
            3,
            "retain both batches while archives remain pending"
        );
        ready.send(()).unwrap();
        let extra = encode_batch([b"\x09\x00"]).unwrap();
        send.unbounded_send(Ok(extra.clone())).unwrap();
        let (_, archives, mut reader) = receiving.await.unwrap();
        assert_eq!(archives, [b"abc".to_vec()]);
        for batch in &batches {
            assert_eq!(&reader.next().await.unwrap().unwrap(), batch);
        }
        assert_eq!(reader.next().await.unwrap().unwrap(), extra);
    });
}

/// Delivers a terminal play message only after the independent archive lane has failed.
async fn terminal_after_bulk_failure(
    terminal: CoreMessage,
    full_queue: bool,
    fail_open: bool,
) -> crate::ProtocolError {
    let (send, reader) = futures::channel::mpsc::unbounded();
    let handoff = encode_core_message(&CoreMessage::Handoff(web_handoff(b"abc"))).unwrap();
    let batch = encode_batch([b"\x09\x00"]).unwrap();
    send.unbounded_send(Ok(handoff)).unwrap();
    if full_queue {
        send.unbounded_send(Ok(full_budget_batch())).unwrap();
    }
    let receiving = LoginSequence::receive_web_handoff(
        reader,
        9,
        async {
            if fail_open {
                failed_pack_open().await
            } else {
                Ok(Some(tokio::io::empty()))
            }
        },
        pending_timers(),
    );
    tokio::pin!(receiving);
    if let Some(result) = receiving.as_mut().now_or_never() {
        return result.err().expect("archive failure ends the join");
    }
    if !full_queue {
        send.unbounded_send(Ok(batch)).unwrap();
    }
    send.unbounded_send(Ok(encode_core_message(&terminal).unwrap()))
        .unwrap();
    receiving
        .await
        .err()
        .expect("a terminal message must end the join")
}

#[test]
fn bulk_failure_preserves_later_disconnect() {
    futures::executor::block_on(async {
        for full_queue in [false, true] {
            for fail_open in [false, true] {
                let terminal = CoreMessage::Disconnect(SessionDisconnect {
                    reason: 0,
                    message: "closed during pack delivery".into(),
                    filtered_message: "filtered disconnect text".into(),
                    hide_screen: false,
                });
                let error = terminal_after_bulk_failure(terminal, full_queue, fail_open).await;
                let disconnect = error.server_disconnect().expect("retain the server reason");
                assert_eq!(
                    disconnect.message.as_deref(),
                    Some("closed during pack delivery")
                );
                assert_eq!(
                    disconnect.filtered_message.as_deref(),
                    Some("filtered disconnect text")
                );
            }
        }
    });
}

#[test]
fn bulk_failure_preserves_later_transfer() {
    futures::executor::block_on(async {
        for full_queue in [false, true] {
            for fail_open in [false, true] {
                let terminal = CoreMessage::Transfer(SessionTransfer {
                    address: "next.example.test".into(),
                    port: 19133,
                    reload_world: true,
                });
                let error = terminal_after_bulk_failure(terminal, full_queue, fail_open).await;
                let transfer = error.server_transfer().expect("retain the transfer target");
                assert_eq!(transfer.host, "next.example.test");
                assert_eq!(transfer.port, 19133);
                assert!(transfer.reload_world);
            }
        }
    });
}

/// Fails archive opening with a distinct I/O error to verify fallback preserves its cause.
async fn failed_pack_open() -> Result<Option<tokio::io::Empty>, crate::ProtocolError> {
    Err(crate::ProtocolError::Bridge(
        BridgeError::Io(std::io::Error::other("archive open failed")).into(),
    ))
}

#[test]
fn bulk_failure_keeps_original_error_when_play_closes_or_fails() {
    futures::executor::block_on(async {
        for last in [
            None,
            Some(Err(BridgeError::Io(std::io::Error::other("play failed")))),
            Some(Ok(Bytes::new())),
            Some(Ok(encode_core_message(&CoreMessage::PackData {
                index: 0,
                data: Bytes::from_static(b"abc"),
            })
            .unwrap())),
        ] {
            let handoff = encode_core_message(&CoreMessage::Handoff(web_handoff(b"abc"))).unwrap();
            let (send, reader) = futures::channel::mpsc::unbounded();
            send.unbounded_send(Ok(handoff)).unwrap();
            let receiving =
                LoginSequence::receive_web_handoff(reader, 9, failed_pack_open(), pending_timers());
            tokio::pin!(receiving);
            assert!(receiving.as_mut().now_or_never().is_none());
            if let Some(last) = last {
                send.unbounded_send(last).unwrap();
            }
            drop(send);
            let error = receiving
                .now_or_never()
                .expect("failed play must end the wait")
                .err()
                .expect("the archive failure remains fatal");
            assert_eq!(
                error.to_string(),
                "bridge connection failed: bridge I/O error: archive open failed"
            );
        }
    });
}

#[test]
fn bulk_failure_uses_host_deadline_even_with_ready_play_batches() {
    futures::executor::block_on(async {
        for ready_play in [false, true] {
            let handoff = encode_core_message(&CoreMessage::Handoff(web_handoff(b"abc"))).unwrap();
            let batch = encode_batch([b"\x09\x00"]).unwrap();
            let failed = std::cell::Cell::new(false);
            let reader =
                futures::stream::iter([Ok(handoff)]).chain(futures::stream::poll_fn(|_| {
                    if ready_play && failed.get() {
                        std::task::Poll::Ready(Some(Ok(batch.clone())))
                    } else {
                        std::task::Poll::Pending
                    }
                }));
            let timers = bridge::tasks::Timers(|duration| {
                assert_eq!(duration, crate::WRITE_FAILURE_DRAIN);
                Box::pin(futures::future::ready(()))
            });
            let error = LoginSequence::receive_web_handoff(
                reader,
                9,
                async {
                    failed.set(true);
                    failed_pack_open().await
                },
                timers,
            )
            .now_or_never()
            .expect("an expired host deadline must stop the drain")
            .err()
            .expect("the archive failure remains fatal");
            assert_eq!(
                error.to_string(),
                "bridge connection failed: bridge I/O error: archive open failed"
            );
        }
    });
}

#[test]
fn bulk_failure_wait_releases_play_on_cancellation() {
    let (send, reader) = futures::channel::mpsc::unbounded();
    let handoff = encode_core_message(&CoreMessage::Handoff(web_handoff(b"abc"))).unwrap();
    send.unbounded_send(Ok(handoff)).unwrap();
    let mut receiving = Box::pin(LoginSequence::receive_web_handoff(
        reader,
        9,
        failed_pack_open(),
        pending_timers(),
    ));
    assert!(receiving.as_mut().now_or_never().is_none());
    assert!(!send.is_closed(), "play stays open for a terminal message");
    drop(receiving);
    assert!(send.is_closed(), "cancellation releases the play reader");
}

#[test]
fn bulk_failure_wait_does_not_renew_the_deadline_for_each_batch() {
    let handoff = encode_core_message(&CoreMessage::Handoff(web_handoff(b"abc"))).unwrap();
    let batch = encode_batch([b"\x09\x00"]).unwrap();
    let failed = std::cell::Cell::new(false);
    let reader = futures::stream::iter([Ok(handoff)]).chain(futures::stream::poll_fn(|_| {
        if failed.get() {
            std::task::Poll::Ready(Some(Ok(batch.clone())))
        } else {
            std::task::Poll::Pending
        }
    }));
    let timers = bridge::tasks::Timers(|duration| {
        assert_eq!(duration, crate::WRITE_FAILURE_DRAIN);
        let mut expires = false;
        Box::pin(futures::future::poll_fn(move |_| {
            if std::mem::replace(&mut expires, true) {
                std::task::Poll::Ready(())
            } else {
                std::task::Poll::Pending
            }
        }))
    });
    let error = LoginSequence::receive_web_handoff(
        reader,
        9,
        async {
            failed.set(true);
            failed_pack_open().await
        },
        timers,
    )
    .now_or_never()
    .expect("ready gameplay cannot extend the host deadline")
    .err()
    .expect("the archive failure remains fatal");
    assert_eq!(
        error.to_string(),
        "bridge connection failed: bridge I/O error: archive open failed"
    );
}
