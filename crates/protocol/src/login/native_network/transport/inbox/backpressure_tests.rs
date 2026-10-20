//! Retained input at the existing duplex write boundary, with original actor coverage.

use super::*;
use futures::stream;
use p384::{ecdsa::SigningKey, elliptic_curve::Generate};
use std::{sync::Arc, time::Duration};
use tokio::sync::oneshot;

#[tokio::test]
async fn native_nether_inbox_count_pressure_keeps_completion_and_fifo() {
    let mut inbox = Inbox::default();
    let count = bedrock_net::BatchLimits::default().max_packets;
    let prefix = Bytes::from_static(b"accepted-count-prefix");
    for _ in 0..count {
        inbox.push(prefix.clone()).unwrap();
    }
    let head = Bytes::from_static(b"original-head");
    let tail = Bytes::from_static(b"original-tail");
    let mut incoming = stream::iter([Ok(head.clone()), Ok(tail.clone())]);
    let (complete, completion) = oneshot::channel();
    let result = {
        let mut writing = Box::pin(inbox.send(
            async { completion.await.map_err(io::Error::other) },
            &mut incoming,
        ));
        let first = futures::poll!(writing.as_mut());
        let _ = complete.send(());
        match first {
            std::task::Poll::Ready(result) => result,
            std::task::Poll::Pending => writing.await,
        }
    };
    for _ in 0..count {
        assert_eq!(inbox.pop().unwrap(), prefix);
    }
    let mut retained = Vec::new();
    while let Some(message) = inbox.pop() {
        retained.push(message);
    }
    while let Some(message) = incoming.next().await {
        retained.push(message.unwrap());
    }
    assert!(
        result.is_ok() && retained == [head, tail],
        "aggregate count pressure failed the original write or discarded its unread head: {result:?}"
    );
}

#[tokio::test]
async fn native_nether_inbox_byte_pressure_keeps_canceled_head_and_fifo() {
    let mut inbox = Inbox::default();
    let maximum = bedrock_net::BatchLimits::default().max_encoded_bytes;
    let prefix = Bytes::from(vec![0xfe; maximum.checked_sub(1).unwrap()]);
    inbox.push(prefix.clone()).unwrap();
    assert!(!inbox.is_full());
    let head = Bytes::from_static(b"original-byte-head");
    let tail = Bytes::from_static(b"original-byte-tail");
    assert!(head.len() <= maximum);
    let mut incoming = stream::iter([Ok(head.clone()), Ok(tail.clone())]);
    let (complete, completion) = oneshot::channel::<()>();
    let pending = {
        let mut writing = Box::pin(inbox.send(
            async { completion.await.map_err(io::Error::other) },
            &mut incoming,
        ));
        futures::poll!(writing.as_mut()).is_pending()
    };
    drop(complete);
    assert_eq!(inbox.pop().unwrap(), prefix);
    let mut retained = Vec::new();
    while let Some(message) = inbox.pop() {
        retained.push(message);
    }
    while let Some(message) = incoming.next().await {
        retained.push(message.unwrap());
    }
    assert!(
        pending && retained == [head, tail],
        "aggregate byte pressure lost the original head when its pending write was canceled"
    );
}

#[tokio::test]
async fn native_nether_inbox_original_signed_actor_drives_pending_write() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let (mut pair, _) =
            crate::login::native_network::accepting::send::tests::signed_pair_with_key(Arc::new(
                SigningKey::generate(),
            ))
            .await;
        let marker = Bytes::from_static(b"original-signed-reliable-input");
        pair.sender.send(marker.clone()).await.unwrap();
        let (observed, completion) = oneshot::channel();
        let incoming = stream::unfold(
            (&mut pair.client, Some(observed)),
            |(client, mut observed)| async move {
                let message = match client.recv().await {
                    Ok(Some(message)) => {
                        if let Some(observed) = observed.take() {
                            observed.send(()).unwrap();
                        }
                        Ok(message)
                    }
                    Ok(None) => return None,
                    Err(error) => Err(io::Error::other(error)),
                };
                Some((message, (client, observed)))
            },
        );
        let mut inbox = Inbox::default();
        let result = inbox
            .send(
                async { completion.await.map_err(io::Error::other) },
                incoming,
            )
            .await;
        let retained = inbox.pop();
        let empty = inbox.pop().is_none();
        let retired = pair.finish().await;
        assert!(retired, "original signed actor/listener/HTTP owners leaked");
        result.unwrap();
        assert_eq!(retained, Some(marker));
        assert!(empty, "original reliable input was replayed");
    })
    .await
    .expect("finite original signed Inbox write fixture stalled");
}
