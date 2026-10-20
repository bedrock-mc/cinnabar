//! Runtime-only acquisition around the reusable caller-driven pack negotiation.

mod acquisition;
pub(super) mod verified;

use super::{Connection, PacketConnection, apply_key_exchange, failure, handshake_failure};
use crate::ProtocolError;
use bedrock_client::{
    handshake::{ClientPackOffer, PacksEvent},
    packs::PackError,
};
use bedrock_packs::ResourcePackHandoff;
use bedrock_protocol_1_26_51::packets;
use futures::{StreamExt, stream};
use std::{collections::BTreeMap, io, path::Path, sync::Arc};

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

/// Drives the same typed pack negotiation without constructing HTTP or filesystem work for an in-band caller.
pub(super) async fn acquire(
    connection: &mut Connection,
    mut preloading: ClientPackOffer,
    policy: Acquisition<'_>,
) -> Result<Acquired, ProtocolError> {
    let cancelled = acquisition::Cancellation::new();
    let offers = preloading.offers().cloned().collect::<Vec<_>>();
    let cache = if offers.is_empty() || !matches!(policy, Acquisition::Desktop { .. }) {
        None
    } else {
        let Acquisition::Desktop { cache_root, .. } = policy else {
            unreachable!("in-band transfer never opens cache")
        };
        wait_io(connection, acquisition::open_cache(cache_root)).await
    };
    let client = if offers.iter().any(|offer| !offer.cdn_url.is_empty()) {
        match policy {
            Acquisition::Desktop { .. } => acquisition::http_client().ok(),
            Acquisition::Public | Acquisition::Hosted => acquisition::public_http_client().ok(),
            #[cfg(feature = "test-support")]
            Acquisition::InBand => None,
        }
    } else {
        None
    };
    let concurrency = match policy {
        Acquisition::Desktop {
            parallel_urls: true,
            ..
        } => MAX_CDN_DOWNLOADS,
        _ => 1,
    };
    let preloads = stream::iter(offers.into_iter().enumerate().map(|(index, offer)| {
        let (cache, client, cancelled) = (cache.clone(), client.clone(), Arc::clone(&cancelled.0));
        async move {
            let result = acquisition::preload(&offer, cache, client, cancelled).await;
            (index, offer, result)
        }
    }))
    .buffer_unordered(concurrency)
    .collect::<Vec<_>>();
    let mut preloads = wait_io(connection, preloads).await;
    preloads.sort_unstable_by_key(|(index, _, _)| *index);
    let mut stores = acquisition::Stores::new();
    let mut preloaded = BTreeMap::new();
    for (_, offer, preload) in preloads {
        if cache.is_some() && !preload.cache_hit {
            stores.register(&offer);
        }
        if let Some(bytes) = preload.archive {
            if matches!(policy, Acquisition::Hosted)
                && let Some(facts) = preload.facts
            {
                preloaded.insert(offer.pack_id_version.pack_uuid, facts);
            }
            preloading
                .supply_preloaded(offer.pack_id_version.pack_uuid, bytes)
                .map_err(pack_failure)?;
        }
    }
    let (mut negotiation, response) = preloading.start().map_err(pack_failure)?;
    stores.queue_completed(&cache, |id| negotiation.completed_archive(id), &cancelled.0);
    connection.send(response).await?;
    loop {
        let packet = connection.next_packet().await.map_err(failure)?;
        if matches!(&*packet.data, packets::Packet::ResourcePackStack(_)) {
            // The library transfers exclusive archive ownership at the stack boundary.
            wait_io(connection, stores.finish()).await;
        }
        match negotiation
            .receive(*packet.data)
            .map_err(handshake_failure)?
        {
            PacksEvent::Send { packs, packets } => {
                negotiation = packs;
                stores.queue_completed(
                    &cache,
                    |id| negotiation.completed_archive(id),
                    &cancelled.0,
                );
                send_packets(connection, packets).await?;
            }
            PacksEvent::Finished(complete) => {
                let (response, handoff, metadata) = if matches!(policy, Acquisition::Hosted) {
                    let (response, handoff, metadata) = complete.into_parts_with_metadata();
                    (response, handoff, metadata.map(Box::new))
                } else {
                    let (response, handoff) = complete.into_parts();
                    (response, handoff, None)
                };
                connection.send(response).await?;
                return Ok(Acquired {
                    handoff,
                    metadata,
                    preloaded,
                });
            }
            PacksEvent::KeyExchange(exchange) => {
                negotiation = apply_key_exchange(connection, exchange).await?;
            }
            PacksEvent::Unhandled { packs, packet } => {
                connection.defer_unhandled(*packet)?;
                negotiation = packs;
            }
        }
    }
}

/// Drives service work beside one retained I/O wait; ordinary reads still own staged terminals.
async fn wait_io<T>(
    connection: &mut impl PacketConnection,
    operation: impl std::future::Future<Output = T>,
) -> T {
    tokio::pin!(operation);
    loop {
        tokio::select! {
            biased;
            () = connection.maintain() => {},
            output = &mut operation => return output,
        }
    }
}

/// Sends each generated response immediately under the existing batch packet limit.
pub(super) async fn send_packets(
    connection: &mut Connection,
    packets: Vec<packets::Packet>,
) -> Result<(), ProtocolError> {
    let packets = packets
        .into_iter()
        .map(crate::Packet::new)
        .collect::<Vec<_>>();
    for batch in packets.chunks(bedrock_net::BatchLimits::default().max_packets) {
        connection.send_packets(batch).await?;
    }
    Ok(())
}

/// Retains the existing native transport failure classification without exposing server data.
fn pack_failure(error: PackError) -> ProtocolError {
    failure(io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests;
