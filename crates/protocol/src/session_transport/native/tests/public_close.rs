//! The ordinary close borrow must finish before the same receiver starts draining.

use super::*;
use crate::session_transport::Inbound;
use bedrock_protocol_1_26_51::{packets, wire};

/// Public close preserves all four queued bodies and the already accepted fifth for later reads.
#[tokio::test(start_paused = true)]
async fn public_close_preserves_the_pending_fifth_body_before_any_receive() {
    tokio::time::timeout(test_time::DEFAULT_TIMEOUT, async {
        let (incoming, receive) = mpsc::channel(QUEUED_BATCHES + 1);
        let (fifth_read, received_fifth) = oneshot::channel();
        let (closed, observed_close) = oneshot::channel();
        let mut transport = SessionTransport::from_native(ReplayConnection {
            incoming: receive,
            fifth_read: Some(fifth_read),
            reads: 0,
            closed: Some(closed),
        });
        for index in 0..=QUEUED_BATCHES {
            incoming
                .send(
                    crate::session_transport::batch_body(&[crate::Packet::new(packets::SetTime {
                        time: wire::ZigZag32(i32::try_from(index).unwrap()),
                    })])
                    .unwrap(),
                )
                .await
                .unwrap();
        }
        received_fifth.await.unwrap();
        let close = transport.close().await;
        observed_close.await.unwrap();
        let mut received = Vec::new();
        for _ in 0..=QUEUED_BATCHES {
            match transport.recv().await {
                Ok(Inbound::Packet(packet)) => received.push(packet),
                _ => break,
            }
        }
        drop(transport);
        drop(incoming);
        assert!(
            close.is_ok(),
            "ordinary close waited for its exclusively borrowed receiver: {close:?}"
        );
        assert_eq!(received.len(), QUEUED_BATCHES + 1);
        for (index, raw) in received.into_iter().enumerate() {
            let packet = raw.decode().unwrap();
            let packets::Packet::SetTime(actual) = &*packet.data else {
                panic!("original SetTime body")
            };
            assert_eq!(actual.time.0, i32::try_from(index).unwrap());
        }
    })
    .await
    .expect("ordinary close and its accepted input did not retire");
}

/// Cancelling a close borrow cannot extend the same driver's original retirement cutoff.
#[tokio::test(start_paused = true)]
async fn cancelled_close_retry_keeps_the_original_driver_cutoff() {
    use crate::{ProtocolError, SessionError, codec::SessionFailure};

    let (entered, closing) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let (dropped, retired) = oneshot::channel();
    let mut transport = SessionTransport::from_native(HeldClose {
        entered: Some(entered),
        release: held,
        dropped: Some(dropped),
    });
    let cutoff = tokio::time::Instant::now() + CLOSE_TIMEOUT;
    {
        let mut first = std::pin::pin!(transport.close());
        assert!(futures::poll!(first.as_mut()).is_pending());
        closing.await.unwrap();
        tokio::time::advance(CLOSE_TIMEOUT / 2).await;
        assert!(futures::poll!(first.as_mut()).is_pending());
    }
    let at_original_cutoff = {
        let mut retry = std::pin::pin!(transport.close());
        assert!(futures::poll!(retry.as_mut()).is_pending());
        tokio::time::advance(CLOSE_TIMEOUT / 2).await;
        let completed =
            tokio::time::timeout_at(cutoff + test_time::DEFAULT_TIMEOUT, retry.as_mut()).await;
        let completed_at = tokio::time::Instant::now();
        match completed {
            Ok(result) if completed_at <= cutoff => Some(result),
            _ => None,
        }
    };

    // Release a still-pending close and join the original task before checking the cutoff.
    let _ = release.send(());
    let Reader::Native(reader) = &mut transport.reader else {
        panic!("native fixture reader")
    };
    let joined = std::future::poll_fn(|cx| reader.poll_driver(cx)).await;
    retired.await.unwrap();
    drop(transport);
    assert!(joined.is_ok() || joined.is_err_and(|error| error.kind() == io::ErrorKind::TimedOut));
    assert!(
        matches!(
            &at_original_cutoff,
            Some(Err(ProtocolError::Session(SessionError(SessionFailure::NativeTransport(error)))))
                if error.kind() == io::ErrorKind::TimedOut
        ),
        "cancelled close retry extended the original driver cutoff: {at_original_cutoff:?}"
    );
}

/// Holds the actual pump's final close until released or the original driver is aborted.
struct HeldClose {
    entered: Option<oneshot::Sender<()>>,
    release: oneshot::Receiver<()>,
    dropped: Option<oneshot::Sender<()>>,
}

impl PacketConnection for HeldClose {
    async fn send_body(&mut self, _: Bytes) -> io::Result<()> {
        Ok(())
    }

    async fn recv_body(&mut self) -> io::Result<Bytes> {
        std::future::pending().await
    }

    async fn close(&mut self) -> io::Result<()> {
        self.entered.take().unwrap().send(()).unwrap();
        let _ = (&mut self.release).await;
        Ok(())
    }
}

impl Drop for HeldClose {
    fn drop(&mut self) {
        if let Some(dropped) = self.dropped.take() {
            let _ = dropped.send(());
        }
    }
}

/// Dropping the close borrow cannot leave its original actor alive beyond the first cutoff.
#[tokio::test(start_paused = true)]
async fn abandoned_close_borrow_retires_the_original_driver_without_retry() {
    let (entered, closing) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let (dropped, retired) = oneshot::channel();
    let mut transport = SessionTransport::from_native(HeldClose {
        entered: Some(entered),
        release: held,
        dropped: Some(dropped),
    });
    let cutoff = tokio::time::Instant::now() + CLOSE_TIMEOUT;
    {
        let mut close = std::pin::pin!(transport.close());
        assert!(futures::poll!(close.as_mut()).is_pending());
        closing.await.unwrap();
    }
    tokio::time::advance(CLOSE_TIMEOUT).await;
    let completion = {
        let Reader::Native(reader) = &mut transport.reader else {
            panic!("native fixture reader")
        };
        tokio::time::timeout_at(
            cutoff + test_time::DEFAULT_TIMEOUT,
            std::future::poll_fn(|cx| reader.poll_driver(cx)),
        )
        .await
    };
    let completed_at = completion
        .as_ref()
        .ok()
        .map(|_| tokio::time::Instant::now());

    // Any still-held actor is released and joined before the cutoff assertion.
    let _ = release.send(());
    let joined = match completion {
        Ok(joined) => joined,
        Err(_) => {
            let Reader::Native(reader) = &mut transport.reader else {
                panic!("native fixture reader")
            };
            std::future::poll_fn(|cx| reader.poll_driver(cx)).await
        }
    };
    retired.await.unwrap();
    drop(transport);
    assert!(joined.is_ok() || joined.is_err_and(|error| error.kind() == io::ErrorKind::TimedOut));
    assert!(
        completed_at.is_some_and(|completed| completed <= cutoff),
        "abandoned close retained the original driver past its cutoff: {completed_at:?}"
    );
}
