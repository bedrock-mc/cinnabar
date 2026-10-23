//! Downloaded sources use the host's generation for metadata and derived-artwork invalidation.

use std::{collections::BTreeMap, sync::Arc};

use render_api::BodyGeneration;

use super::{RemoteImage, ServerAtlas, Source, is_remote, source};

#[cfg(test)]
thread_local! {
    pub(super) static BODY_INSPECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct Metadata {
    generation: BodyGeneration,
    size: Option<[u32; 2]>,
}

/// Only generations and dimensions stay here; the host retains downloaded bodies.
#[derive(Default)]
pub(super) struct RemoteSources {
    metadata: BTreeMap<String, Metadata>,
    changed: bool,
}

impl ServerAtlas {
    /// Reads a body's header once per generation, without retaining its encoded bytes.
    pub(super) fn remote_source(&self, key: &str) -> Option<Source> {
        let mut sources = self.remote_sources.borrow_mut();
        let (generation, bytes) = match self.remote.as_ref()?.state(key) {
            RemoteImage::Ready(generation, bytes) => (generation, bytes),
            RemoteImage::Failed | RemoteImage::Loading => {
                sources.changed |= sources.metadata.remove(key).is_some();
                return None;
            }
        };
        let size = match sources.metadata.get(key) {
            Some(metadata) if metadata.generation == generation => metadata.size,
            _ => {
                #[cfg(test)]
                BODY_INSPECTIONS.with(|calls| calls.set(calls.get() + 1));
                let size = source(Arc::clone(&bytes)).map(|source| source.size);
                if !sources.metadata.contains_key(key) && sources.metadata.len() >= super::MAX_EXTRA
                {
                    sources.metadata.pop_first();
                    sources.changed = true;
                }
                let replaced = sources
                    .metadata
                    .insert(key.to_owned(), Metadata { generation, size });
                sources.changed |= replaced.is_some();
                size
            }
        }?;
        Some(Source {
            bytes,
            size,
            packed: super::fitted(size),
        })
    }

    /// Checks drawn URLs before any resident sprite, sheet or decode can be reused.
    pub(super) fn prepare_remote_sources<'a>(&mut self, keys: impl Iterator<Item = &'a str>) {
        for key in keys.filter(|key| is_remote(key)) {
            self.remote_source(key);
        }
        self.invalidate_remote_sources();
    }

    /// Retires all derived artwork together, including queued results from an older generation.
    pub(super) fn invalidate_remote_sources(&mut self) -> bool {
        if !std::mem::take(&mut self.remote_sources.get_mut().changed) {
            return false;
        }
        self.reset_derived_artwork();
        true
    }
}
