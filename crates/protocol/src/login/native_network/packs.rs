//! Runtime-only acquisition around the reusable caller-driven pack negotiation.

mod acquisition;
pub(super) mod verified;

use super::Connection;
use crate::ProtocolError;
use bedrock_client::handshake::ClientPackOffer;
use bedrock_client_tokio::packs::PackIo;
#[cfg(test)]
use bedrock_client_tokio::packs::wait_io;
use bedrock_packs::ResourcePackHandoff;
use bedrock_protocol_1_26_51::types::PackInfoData;
use futures::{StreamExt, stream};
use resource_pack::cache::ArchiveCache;
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub(super) const MAX_CDN_DOWNLOADS: usize = 4;

#[cfg(test)]
tokio::task_local! {
    /// Reports real HTTP attempts only within one fixture-owned native join future.
    pub(super) static HTTP_ATTEMPTS: tokio::sync::mpsc::Sender<uuid::Uuid>;
    /// Captures one owning fixture join's original numeric HTTP endpoint into its client only.
    pub(super) static PUBLIC_CDN_ENDPOINT: std::net::SocketAddr;
}

/// Fixed runtime acquisition policy around the same verified protocol archive owner.
#[derive(Clone, Copy)]
pub(super) enum Acquisition<'a> {
    Desktop {
        cache_root: Option<&'a Path>,
        parallel_urls: bool,
    },
    #[cfg(feature = "test-support")]
    InBand,
    Public,
    Hosted,
}

/// Original verified archive handoff and opt-in upstream metadata from the same pack owner.
pub(super) struct Acquired {
    pub(super) handoff: ResourcePackHandoff,
    pub(super) metadata: Option<Box<bedrock_client::packs::PackMetadata>>,
    pub(super) preloaded: BTreeMap<uuid::Uuid, verified::ArchiveFacts>,
}

/// Supplies application-owned cache/CDN work to the sole reusable wire driver.
pub(super) async fn acquire(
    connection: &mut Connection,
    offer: ClientPackOffer,
    policy: Acquisition<'_>,
) -> Result<Acquired, ProtocolError> {
    let mut io = AcquisitionIo {
        policy,
        cancelled: acquisition::Cancellation::new(),
        cache: None,
        stores: acquisition::Stores::new(),
        preloaded: BTreeMap::new(),
    };
    let result =
        bedrock_client_tokio::packs::acquire(connection, offer, &mut io, &mut |_| {}).await;
    let acquired = super::connection::driver_result(connection, result).await?;
    Ok(Acquired {
        handoff: acquired.handoff,
        metadata: acquired.metadata,
        preloaded: io.preloaded,
    })
}

/// The existing application acquisition policy; it owns no packet or negotiation state.
struct AcquisitionIo<'a> {
    policy: Acquisition<'a>,
    cancelled: acquisition::Cancellation,
    cache: Option<Arc<ArchiveCache>>,
    stores: acquisition::Stores,
    preloaded: BTreeMap<uuid::Uuid, verified::ArchiveFacts>,
}

impl PackIo for AcquisitionIo<'_> {
    async fn preload(&mut self, offers: &[PackInfoData]) -> Vec<Option<Vec<u8>>> {
        self.cache = if offers.is_empty() || !matches!(self.policy, Acquisition::Desktop { .. }) {
            None
        } else {
            let Acquisition::Desktop { cache_root, .. } = self.policy else {
                unreachable!("in-band transfer never opens cache")
            };
            acquisition::open_cache(cache_root).await
        };
        let client = if offers.iter().any(|offer| !offer.cdn_url.is_empty()) {
            match self.policy {
                Acquisition::Desktop { .. } => acquisition::http_client().ok(),
                Acquisition::Public | Acquisition::Hosted => acquisition::public_http_client().ok(),
                #[cfg(feature = "test-support")]
                Acquisition::InBand => None,
            }
        } else {
            None
        };
        let concurrency = match self.policy {
            Acquisition::Desktop {
                parallel_urls: true,
                ..
            } => MAX_CDN_DOWNLOADS,
            _ => 1,
        };
        let preloads = stream::iter((0..offers.len()).map(|index| {
            let offer = &offers[index];
            let (cache, client, cancelled) = (
                self.cache.clone(),
                client.clone(),
                Arc::clone(&self.cancelled.0),
            );
            async move {
                let result = acquisition::preload(offer, cache, client, cancelled).await;
                (index, result)
            }
        }))
        .buffer_unordered(concurrency)
        .collect::<Vec<_>>()
        .await;
        let mut preloads = preloads;
        preloads.sort_unstable_by_key(|(index, _)| *index);
        preloads
            .into_iter()
            .map(|(index, preload)| {
                let offer = &offers[index];
                if self.cache.is_some() && !preload.cache_hit {
                    self.stores.register(offer);
                }
                if matches!(self.policy, Acquisition::Hosted)
                    && preload.archive.is_some()
                    && let Some(facts) = preload.facts
                {
                    self.preloaded
                        .insert(offer.pack_id_version.pack_uuid, facts);
                }
                preload.archive
            })
            .collect()
    }

    fn completed(&mut self, id: uuid::Uuid, archive: Arc<Vec<u8>>) {
        self.stores.queue_completed(
            &self.cache,
            |candidate| (candidate == id).then(|| Arc::clone(&archive)),
            &self.cancelled.0,
        );
    }

    async fn flush(&mut self) {
        self.stores.finish().await;
    }
}

#[cfg(test)]
mod tests;
