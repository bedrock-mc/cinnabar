//! Original signed actor coverage of the relocated duplex inbox owner.
use bedrock_client_tokio::transport::inbox::Inbox;
use bytes::Bytes;
use futures::stream;
use p384::{ecdsa::SigningKey, elliptic_curve::Generate};
use std::{io, sync::Arc, time::Duration};
use tokio::sync::oneshot;

#[tokio::test]
async fn native_nether_inbox_original_signed_actor_drives_pending_write() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let (mut pair, _) = bedrock_client_tokio::server::test_support::signed_pair_with_key(
            Arc::new(SigningKey::generate()),
            crate::MAX_DEFERRED_PACKET_BYTES,
        )
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
