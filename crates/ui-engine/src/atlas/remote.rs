//! Downloaded sources are frozen for a frame and retire artwork only for replaced URLs.

use std::{collections::BTreeMap, sync::Arc};

use render_api::BodyGeneration;

use super::{Decodes, RemoteImage, ServerAtlas, Source, source};

#[cfg(test)]
mod tests;

#[cfg(test)]
thread_local! {
    pub(super) static BODY_INSPECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Default)]
struct Metadata {
    generation: Option<BodyGeneration>,
    size: Option<[u32; 2]>,
    snapshot: Option<Source>,
    observed: bool,
    used: bool,
}

impl Metadata {
    /// Freezes one host observation, reusing header dimensions for an unchanged generation.
    fn refresh(&mut self, state: RemoteImage) -> (bool, bool) {
        self.observed = true;
        let was_ready = self.snapshot.is_some();
        let RemoteImage::Ready(generation, bytes) = state else {
            self.snapshot = None;
            return (false, was_ready);
        };
        let changed = self.generation != Some(generation);
        let replaced = changed && self.generation.is_some();
        if changed {
            #[cfg(test)]
            BODY_INSPECTIONS.with(|calls| calls.set(calls.get() + 1));
            self.size = source(Arc::clone(&bytes)).map(|source| source.size);
            self.generation = Some(generation);
        }
        self.snapshot = self.size.map(|size| Source {
            bytes,
            size,
            packed: super::fitted(size),
        });
        (replaced, changed || was_ready != self.snapshot.is_some())
    }
}

/// Header metadata and host-owned bodies borrowed for the current frame's snapshot.
#[derive(Default)]
pub(super) struct RemoteSources {
    metadata: BTreeMap<String, Metadata>,
}

impl ServerAtlas {
    /// Includes standalone callers' known dependencies in the snapshot before starting a frame.
    pub(super) fn begin_required_frame<'a>(&mut self, keys: impl Iterator<Item = &'a str>) {
        for key in keys {
            if let Some(metadata) = self.remote_sources.get_mut().metadata.get_mut(key) {
                metadata.used = true;
            }
        }
        self.begin_frame();
    }

    /// Reads the frozen source; a URL with no previous artwork may join on first use.
    pub(super) fn remote_source(&self, key: &str) -> Option<Source> {
        let mut sources = self.remote_sources.borrow_mut();
        if let Some(metadata) = sources.metadata.get_mut(key) {
            metadata.used = true;
            if !metadata.observed {
                metadata.observed = true;
                let state = self.remote.as_ref()?.state(key);
                if matches!(&state, RemoteImage::Ready(generation, _) if Some(*generation) == metadata.generation)
                {
                    metadata.refresh(state);
                }
            }
            return metadata.snapshot.clone();
        }
        let state = self.remote.as_ref()?.state(key);
        if sources.metadata.len() >= super::MAX_EXTRA {
            let victim = sources.metadata.iter().find_map(|(key, metadata)| {
                (!metadata.used && !self.has_remote_artwork(key)).then(|| key.clone())
            });
            if let Some(victim) = victim {
                sources.metadata.remove(&victim);
            }
        }
        let mut metadata = Metadata::default();
        metadata.refresh(state);
        metadata.used = true;
        let snapshot = metadata.snapshot.clone();
        sources.metadata.insert(key.to_owned(), metadata);
        snapshot
    }

    /// Keeps layout on the frozen dimensions even if an inactive URL changed before first use.
    pub(super) fn remote_size(&self, key: &str) -> Option<[u32; 2]> {
        self.remote_source(key);
        self.remote_sources.borrow().metadata.get(key)?.size
    }

    /// Starts a presented frame before any screen lays out or emits geometry.
    /// All screens must use `require_current` until the next frame starts.
    pub fn begin_frame(&mut self) -> bool {
        self.clock += 1;
        let mut sources = std::mem::take(self.remote_sources.get_mut());
        while sources.metadata.len() > super::MAX_EXTRA {
            let victim = sources.metadata.iter().find_map(|(key, metadata)| {
                (!metadata.used && !self.has_remote_artwork(key)).then(|| key.clone())
            });
            let Some(victim) = victim else { break };
            sources.metadata.remove(&victim);
        }
        let mut changed = false;
        if let Some(remote) = self.remote.clone() {
            for (key, metadata) in &mut sources.metadata {
                if std::mem::take(&mut metadata.used) {
                    let (replaced, refreshed) = metadata.refresh(remote.state(key));
                    changed |= refreshed;
                    if replaced {
                        self.remove_remote_artwork(key);
                    }
                } else {
                    metadata.snapshot = None;
                    metadata.observed = false;
                }
            }
        }
        *self.remote_sources.get_mut() = sources;
        if changed {
            self.metadata_revision = self.metadata_revision.wrapping_add(1);
            self.metadata_dirty = true;
        }
        changed
    }

    /// Keeps generation metadata as long as any representation can still be reused.
    fn has_remote_artwork(&self, key: &str) -> bool {
        self.resident.contains_key(key)
            || self.decodes.pixels.contains_key(key)
            || self.decodes.failed.iter().any(|failed| failed == key)
            || self.decodes.jobs.pending() == Some(key)
            || self.native.contains(key)
            || self.outlines.contains(key)
            || self.oversized.iter().any(|entry| entry.0 == key)
    }

    /// Drops only a replaced URL's derived state, leaving page reclamation to normal eviction.
    fn remove_remote_artwork(&mut self, key: &str) {
        self.resident.remove(key);
        for page in &mut self.pages {
            page.keys.retain(|resident| resident != key);
        }
        self.decodes.remove(key);
        self.native.remove(key);
        self.outlines.remove(key);
        self.oversized.remove(key);
    }
}

impl Decodes {
    /// Retires cached pixels, failures and pending work only for this URL.
    fn remove(&mut self, key: &str) {
        if let Some(pixels) = self.pixels.remove(key) {
            self.bytes -= pixels.len();
        }
        self.order.retain(|cached| cached != key);
        self.failed.retain(|failed| failed != key);
        self.jobs.remove(key);
    }
}
