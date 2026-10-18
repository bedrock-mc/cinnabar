use super::*;
use std::{
    future::{Future, poll_fn},
    task::{Context, Poll, ready},
};

impl<R: bridge::FrameRead> PlaySession<R> {
    /// Keeps packet ownership across a cancelled background read, then uses normal resolver
    /// classification and recovery. A disk key alone can never produce a found-ID status.
    pub(super) async fn finish_blob_prefetch(&mut self) -> Result<(), ProtocolError> {
        let (packet, packet_bytes) = self
            .pending_blob_prefetch
            .as_ref()
            .expect("pending prefetch");
        if self
            .blob_cache
            .as_ref()
            .expect("enabled cache")
            .can_prefetch(packet, *packet_bytes)
        {
            let prefetch = self
                .blob_cache
                .as_ref()
                .expect("enabled cache")
                .cache()
                .prefetch_packet(packet);
            prefetch_while_reading(prefetch, |cx| {
                self.transport
                    .poll_read_ahead_one(crate::session_transport::MAX_READ_AHEAD_BYTES, cx)
            })
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
        if !status.accepted() {
            self.transport.reject_current(PacketOutcome::Skipped);
        }
        let recovery = status.take_recovery();
        let admission = status.take_admission();
        let mut events = VecDeque::with_capacity(2);
        if let Some(admission) = admission {
            events.push_back(WorldEvent::SubChunkReplyAdmission(admission));
        }
        if let Some(recovery) = recovery {
            events.push_back(WorldEvent::ChunkResync(recovery));
        }
        self.pending_blob_cache_events = events;
        self.cache_status
            .lock()
            .expect("cache status lock")
            .push(status);
        Ok(())
    }
}

/// Polls the same disk future before each bounded raw-reader step. The step returns true
/// after one item when reading may continue, or false to stop intake at a bound or terminal.
/// After stopping, only the original future is polled; no request or deadline is restarted.
pub(super) async fn prefetch_while_reading(
    prefetch: impl Future<Output = ()>,
    mut read_one: impl FnMut(&mut Context<'_>) -> Poll<bool>,
) {
    let mut prefetch = std::pin::pin!(prefetch);
    let mut reading = true;
    poll_fn(|cx| {
        loop {
            if prefetch.as_mut().poll(cx).is_ready() {
                return Poll::Ready(());
            }
            if !reading {
                return Poll::Pending;
            }
            reading = ready!(read_one(cx));
        }
    })
    .await;
}

#[cfg(test)]
mod overlap_tests;

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
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        task::{Context, Poll},
    };
    use tokio::net::UnixListener;

    #[derive(Debug)]
    struct ControlledRead {
        ready: AtomicBool,
        starts: AtomicUsize,
        cache: WeakClientBlobCache,
    }

    impl BlobCacheStore for ControlledRead {
        /// This read-only fixture never persists a body.
        fn persist(&self, _: u64, _: Arc<[u8]>) {}
        /// This fixture does not evict by age.
        fn touch(&self, _: &[u64]) {}
        /// Leaves the future pending until the test explicitly completes the read.
        fn prefetch(&self, hashes: Vec<u64>) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            self.starts.fetch_add(1, Ordering::Relaxed);
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
            starts: AtomicUsize::new(0),
            cache: cache.downgrade(),
        });
        let hash = crate::client_blob_hash(b"body");
        let packet = Packet::new(super::super::tests::cached_level_chunk(vec![hash]));
        let encoded = crate::session_transport::tests::packet_bytes(&packet);
        let transport = SessionTransport::new(reader, frames, &[encoded]);
        let mut session =
            PlaySession::new(transport, Some(cache.with_store(disk.clone())), 1, false);
        let waker = noop_waker();
        let mut context = Context::from_waker(&waker);
        {
            let mut recv = std::pin::pin!(session.recv_world_ingress_with_blob_cache(0));
            assert!(recv.as_mut().poll(&mut context).is_pending());
        }
        assert!(session.pending_blob_prefetch.is_some());
        assert!(session.pending_blob_cache_events.is_empty());
        assert_eq!(disk.starts.load(Ordering::Relaxed), 1);
        assert_eq!(session.blob_cache_stats().hashes_classified, 0);
        disk.ready.store(true, Ordering::Release);
        session.finish_blob_prefetch().await.unwrap();
        assert_eq!(
            disk.starts.load(Ordering::Relaxed),
            2,
            "only cancellation starts another attempt"
        );
        assert!(session.pending_blob_prefetch.is_none());
        let status = session
            .cache_status
            .lock()
            .unwrap()
            .select(simulation_clock::Moment::default())
            .expect("queued classification");
        assert_eq!(status.found_ids, [wire::U64LE(hash)]);
        assert!(status.missing_ids.is_empty());
        let resolver = session.blob_cache.as_mut().unwrap();
        assert!(resolver.pop_ready().is_some());
        assert!(resolver.pop_ready().is_none());
    }

    /// Byte pressure finishes classification without waiting for a disk read, even when the
    /// decoded body is larger than the caller's raw-size hint.
    #[tokio::test]
    async fn blob_cache_prefetch_pressure_uses_normal_miss_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(bridge::session_endpoint_path(dir.path())).unwrap();
        let (reader, frames) = bridge::connect_session(dir.path()).await.unwrap();
        let (_peer, _) = listener.accept().await.unwrap();
        let cache = ClientBlobCache::default();
        let disk = Arc::new(ControlledRead {
            ready: AtomicBool::new(false),
            starts: AtomicUsize::new(0),
            cache: cache.downgrade(),
        });
        let transport = SessionTransport::new(reader, frames, &[]);
        let mut session =
            PlaySession::new(transport, Some(cache.with_store(disk.clone())), 1, false);
        let mut earlier = super::super::tests::cached_level_chunk(vec![1]);
        earlier.chunk_position.x = wire::ZigZag32(1);
        session
            .blob_cache
            .as_mut()
            .unwrap()
            .accept_cached_packet_with_size(
                Packet::new(earlier),
                crate::MAX_CLIENT_BLOB_PENDING_BYTES / 2,
            )
            .unwrap();
        let mut candidate = super::super::tests::cached_level_chunk(vec![2]);
        candidate.serialized_chunk_data = vec![0; crate::MAX_CLIENT_BLOB_PENDING_BYTES / 2].into();
        session.pending_blob_prefetch = Some((Packet::new(candidate), 1));
        let waker = noop_waker();
        let mut context = Context::from_waker(&waker);
        {
            let mut finish = std::pin::pin!(session.finish_blob_prefetch());
            assert!(matches!(
                finish.as_mut().poll(&mut context),
                Poll::Ready(Ok(()))
            ));
        }
        assert!(session.pending_blob_prefetch.is_none());
        assert_eq!(
            disk.starts.load(Ordering::Relaxed),
            0,
            "pressure bypasses disk waiting"
        );
        assert!(session.blob_cache_stats().pending_bytes <= crate::MAX_CLIENT_BLOB_PENDING_BYTES);
        assert_eq!(session.blob_cache_stats().pending_transactions, 1);
        let status = session
            .cache_status
            .lock()
            .unwrap()
            .select(simulation_clock::Moment::default())
            .expect("queued classification");
        assert_eq!(status.missing_ids, [wire::U64LE(2)]);
        assert!(status.found_ids.is_empty());
        assert!(!session.pending_blob_cache_events.is_empty());
    }
}
