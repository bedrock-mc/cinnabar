use super::*;

impl PlaySession {
    /// Keeps packet ownership across a cancelled background read, then uses normal resolver
    /// classification and recovery. A disk key alone can never produce a found-ID status.
    pub(super) async fn finish_blob_prefetch(&mut self) -> Result<(), ProtocolError> {
        let (packet, packet_bytes) = self
            .pending_blob_prefetch
            .as_ref()
            .expect("pending prefetch");
        if *packet_bytes <= crate::MAX_CLIENT_BLOB_PENDING_BYTES {
            self.blob_cache
                .as_ref()
                .expect("enabled cache")
                .cache()
                .prefetch_packet(packet)
                .await;
        }
        let (packet, packet_bytes) = self
            .pending_blob_prefetch
            .take()
            .expect("retained prefetch");
        let mut status = self
            .blob_cache
            .as_mut()
            .expect("enabled cache")
            .accept_cached_packet_with_size(packet, packet_bytes)?;
        let recovery = status.take_recovery();
        let admission = status.take_admission();
        let mut events = VecDeque::with_capacity(2);
        if let Some(admission) = admission {
            events.push_back(WorldEvent::SubChunkReplyAdmission(admission));
        }
        if let Some(recovery) = recovery {
            events.push_back(WorldEvent::ChunkResync(recovery));
        }
        self.pending_blob_cache_delivery = Some(PendingBlobCacheDelivery {
            status_packets: status.into_packets().into_iter().map(Packet::new).collect(),
            status_send_in_flight: false,
            events,
        });
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{BlobCacheStore, WeakClientBlobCache};
    use futures::task::noop_waker;
    use std::{
        future::Future,
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        task::{Context, Poll},
    };
    use tokio::net::UnixListener;

    #[derive(Debug)]
    struct ControlledRead {
        ready: AtomicBool,
        cache: WeakClientBlobCache,
    }

    impl BlobCacheStore for ControlledRead {
        /// This read-only fixture never persists a body.
        fn persist(&self, _: u64, _: Arc<[u8]>) {}
        /// This fixture does not evict by age.
        fn touch(&self, _: &[u64]) {}
        /// Leaves the future pending until the test explicitly completes the read.
        fn prefetch(&self, hashes: Vec<u64>) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            Box::pin(std::future::poll_fn(move |_| {
                if !self.ready.load(Ordering::Acquire) {
                    return Poll::Pending;
                }
                for hash in &hashes {
                    self.cache
                        .admit(*hash, Arc::from(b"body".as_slice()))
                        .unwrap();
                }
                Poll::Ready(())
            }))
        }
    }

    /// Cancelling ingress during a disk read retains the cached packet for one later publication.
    #[tokio::test]
    async fn blob_cache_prefetch_cancellation_retains_packet_and_status() {
        let dir = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(bridge::session_endpoint_path(dir.path())).unwrap();
        let (reader, frames) = bridge::connect_session(dir.path()).await.unwrap();
        let (_peer, _) = listener.accept().await.unwrap();
        let cache = ClientBlobCache::default();
        let disk = Arc::new(ControlledRead {
            ready: AtomicBool::new(false),
            cache: cache.downgrade(),
        });
        let hash = crate::client_blob_hash(b"body");
        let packet = Packet::new(super::super::tests::cached_level_chunk(vec![hash]));
        let encoded = crate::session_transport::tests::packet_bytes(&packet);
        let transport = SessionTransport::new(reader, frames, &[encoded]);
        let mut session = PlaySession::new(
            transport,
            Some(cache.with_store(disk.clone())),
            ResourcePackHandoff::default(),
            1,
            false,
        );
        let waker = noop_waker();
        let mut context = Context::from_waker(&waker);
        {
            let mut recv = std::pin::pin!(session.recv_world_ingress_with_blob_cache(0));
            assert!(recv.as_mut().poll(&mut context).is_pending());
        }
        assert!(session.pending_blob_prefetch.is_some());
        assert!(session.pending_blob_cache_delivery.is_none());
        disk.ready.store(true, Ordering::Release);
        session.finish_blob_prefetch().await.unwrap();
        assert!(session.pending_blob_prefetch.is_none());
        let delivery = session.pending_blob_cache_delivery.as_ref().unwrap();
        assert_eq!(delivery.status_packets.len(), 1);
        let packets::Packet::ClientCacheBlobStatus(status) = &*delivery.status_packets[0].data
        else {
            panic!("status")
        };
        assert_eq!(status.found_ids, [wire::U64LE(hash)]);
        assert!(status.missing_ids.is_empty());
        let resolver = session.blob_cache.as_mut().unwrap();
        assert!(resolver.pop_ready().is_some());
        assert!(resolver.pop_ready().is_none());
    }
}
