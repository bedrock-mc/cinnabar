//! Runtime-only HTTP and persistent archive I/O for native direct joins.

mod http;

pub(super) use http::{Client, http_client, public_http_client};

use bedrock_protocol_1_26_51::types::PackInfoData;
use resource_pack::cache::{ArchiveCache, ArchiveCacheKey, DEFAULT_CACHE_QUOTA};
use std::{
    collections::BTreeMap,
    io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{sync::Semaphore, task::JoinSet};

/// Cancels any remaining bounded filesystem work when the join future is dropped.
pub(super) struct Cancellation(pub(super) Arc<AtomicBool>);
impl Cancellation {
    /// Creates cancellation state independent of any Tokio runtime or task handle.
    pub(super) fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
}
impl Drop for Cancellation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Opens the configured directory off the async executor; cache failures remain misses.
pub(super) async fn open_cache(root: Option<&Path>) -> Option<Arc<ArchiveCache>> {
    let root = root?.to_owned();
    match tokio::task::spawn_blocking(move || ArchiveCache::open(&root, DEFAULT_CACHE_QUOTA)).await
    {
        Ok(Ok(cache)) => Some(Arc::new(cache)),
        _ => {
            tracing::warn!("native resource-pack cache is unavailable");
            None
        }
    }
}

/// The result of caller-owned cache/CDN acquisition, before protocol admission.
pub(super) struct Preload {
    pub(super) archive: Option<Vec<u8>>,
    pub(super) cache_hit: bool,
    pub(super) facts: Option<super::verified::ArchiveFacts>,
}

/// Tries an exact verified cache hit, then CDN; failure retains in-band fallback.
pub(super) async fn preload(
    offer: &PackInfoData,
    cache: Option<Arc<ArchiveCache>>,
    client: Option<Client>,
    cancelled: Arc<AtomicBool>,
) -> Preload {
    let id = offer.pack_id_version.pack_uuid;
    let version = offer
        .pack_id_version
        .pack_version
        .version
        .to_string_lossy()
        .into_owned();
    let size = offer.pack_size.0;
    let key = ArchiveCacheKey::new(id, &version, size).ok();
    if let (Some(cache), Some(key)) = (&cache, &key) {
        let (cache, key, cancelled) = (Arc::clone(cache), key.clone(), Arc::clone(&cancelled));
        match tokio::task::spawn_blocking(move || {
            cache.load(&key, &cancelled, std::time::SystemTime::now())
        })
        .await
        {
            Ok(Ok(Some(bytes))) => {
                return Preload {
                    archive: Some(bytes),
                    cache_hit: true,
                    facts: None,
                };
            }
            Ok(Ok(None)) => {}
            _ => tracing::warn!(%id, "native pack cache read failed; downloading again"),
        }
    }
    if !offer.cdn_url.is_empty()
        && let Some(client) = client
    {
        let result = async {
            let url = offer
                .cdn_url
                .to_str()
                .map_err(|_| invalid("invalid pack CDN URL"))?;
            let request = client.get(url)?;
            #[cfg(test)]
            let _ = super::HTTP_ATTEMPTS.try_with(|attempts| {
                attempts
                    .try_send(id)
                    .expect("fixture HTTP-attempt capacity");
            });
            let mut response = request
                .send()
                .await
                .map_err(|_| invalid("pack CDN request failed"))?;
            if response.status() != reqwest::StatusCode::OK {
                return Err(invalid("pack CDN returned an unsuccessful response"));
            }
            if response
                .content_length()
                .is_some_and(|length| length > size)
            {
                return Err(invalid("pack CDN length exceeds advertised size"));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| invalid("pack CDN body failed"))?
            {
                if bytes.len() as u64 + chunk.len() as u64 > size {
                    return Err(invalid("pack CDN body exceeds advertised size"));
                }
                bytes
                    .try_reserve(chunk.len())
                    .map_err(|_| invalid("pack CDN allocation failed"))?;
                bytes.extend_from_slice(&chunk);
            }
            verify(bytes, offer.clone()).await
        }
        .await;
        match result {
            Ok((bytes, facts)) => {
                return Preload {
                    archive: Some(bytes),
                    cache_hit: false,
                    facts: Some(facts),
                };
            }
            Err(_) => {
                tracing::warn!(%id, "native pack CDN acquisition failed; requesting game chunks")
            }
        }
    }
    Preload {
        archive: None,
        cache_hit: false,
        facts: None,
    }
}

/// Moves original bytes through off-thread ZIP/manifest identity validation without copying.
async fn verify(
    bytes: Vec<u8>,
    offer: PackInfoData,
) -> io::Result<(Vec<u8>, super::verified::ArchiveFacts)> {
    tokio::task::spawn_blocking(move || {
        let facts = super::verified::describe(&bytes, &offer)?;
        Ok((bytes, facts))
    })
    .await
    .map_err(|_| invalid("pack validation worker failed"))?
}

/// Joins bounded cache writes before releasing the application's original archive bytes.
pub(super) struct Stores {
    jobs: JoinSet<()>,
    slot: Arc<Semaphore>,
    pending: BTreeMap<uuid::Uuid, (ArchiveCacheKey, u64)>,
}

impl Stores {
    /// Serializes the cache's blocking filesystem worker while other negotiation continues.
    pub(super) fn new() -> Self {
        Self {
            jobs: JoinSet::new(),
            slot: Arc::new(Semaphore::new(1)),
            pending: BTreeMap::new(),
        }
    }

    /// Registers one admitted non-cache-hit offer for a future best-effort write.
    pub(super) fn register(&mut self, offer: &PackInfoData) {
        let id = offer.pack_id_version.pack_uuid;
        if let Ok(key) = ArchiveCacheKey::new(
            id,
            &offer.pack_id_version.pack_version.version.to_string_lossy(),
            offer.pack_size.0,
        ) {
            self.pending.insert(id, (key, offer.pack_size.0));
        }
    }

    /// Queues each completed original archive once, without copying or delaying responses.
    pub(super) fn queue_completed(
        &mut self,
        cache: &Option<Arc<ArchiveCache>>,
        completed_archive: impl Fn(uuid::Uuid) -> Option<Arc<Vec<u8>>>,
        cancelled: &Arc<AtomicBool>,
    ) {
        let Some(cache) = cache else {
            return;
        };
        let ready = self
            .pending
            .keys()
            .filter_map(|id| completed_archive(*id).map(|bytes| (*id, bytes)))
            .collect::<Vec<_>>();
        for (id, bytes) in ready {
            let (key, size) = self.pending.remove(&id).expect("registered cache identity");
            if bytes.len() as u64 != size {
                continue;
            }
            let (cache, cancelled, slot) = (
                Arc::clone(cache),
                Arc::clone(cancelled),
                Arc::clone(&self.slot),
            );
            self.jobs.spawn(async move {
                let Ok(_permit) = slot.acquire_owned().await else {
                    return;
                };
                if !matches!(
                    tokio::task::spawn_blocking(move || cache.store(&key, &bytes, &cancelled))
                        .await,
                    Ok(Ok(()))
                ) {
                    tracing::warn!("native pack cache write failed");
                }
            });
        }
    }

    /// Waits until all workers have dropped their shared archive references.
    pub(super) async fn finish(&mut self) {
        while self.jobs.join_next().await.is_some() {}
    }
}

/// Produces a bounded failure without exposing signed URLs or response content.
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod service_tests;

#[cfg(test)]
mod raknet_tests;
