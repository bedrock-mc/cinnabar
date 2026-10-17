use super::*;
use crate::LoginSequence;
use futures::{FutureExt, StreamExt};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

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
            let (_, archives, mut reader) = LoginSequence::receive_web_handoff(reader, 9, async {
                Ok(bulk.then_some(wire.as_slice()))
            })
            .await
            .unwrap();
            assert_eq!(archives, [b"abc".to_vec()]);
            assert_eq!(reader.next().await.unwrap().unwrap(), batch);
        }
    });
}

#[test]
fn bulk_handoff_backpressures_a_full_play_queue() {
    futures::executor::block_on(async {
        let handoff = encode_core_message(&CoreMessage::Handoff(web_handoff(b"abc"))).unwrap();
        let batch = encode_batch([b"\x09\x00"]).unwrap();
        let frames = std::iter::once(Ok(handoff)).chain((0..40).map(|_| Ok(batch.clone())));
        let reader = futures::stream::iter(frames).chain(futures::stream::pending());
        let wire = pack_stream(9, b"abc");
        let (_, archives, mut reader) =
            LoginSequence::receive_web_handoff(reader, 9, async { Ok(Some(wire.as_slice())) })
                .await
                .unwrap();
        assert_eq!(archives, [b"abc".to_vec()]);
        for _ in 0..40 {
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
        let result =
            LoginSequence::receive_web_handoff::<_, &[u8]>(reader, 9, futures::future::pending())
                .await;
        assert!(result.is_err());
    });
}
