//! Worker-owned content interning and unchanged nearest-neighbour cape sampling.
use render_api::{CapeImage, SkinRgba8};
use render_model::{MAX_RENDERED_PLAYERS, STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE};
use std::{
    collections::{HashMap, VecDeque},
    hash::{BuildHasher, Hash, Hasher},
    sync::Arc,
};

#[derive(Clone, Debug)]
struct ContentKey {
    source: CapeImage,
    hash: u64,
}

impl PartialEq for ContentKey {
    /// Exact worker-side equality prevents hash collisions from exchanging cape art.
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.source == other.source
    }
}
impl Eq for ContentKey {}

impl Hash for ContentKey {
    /// Reuses the source hash during content-cache operations.
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

#[derive(Debug, Default)]
pub(super) struct WorkerCache {
    entries: HashMap<ContentKey, Option<SkinRgba8>>,
    order: VecDeque<ContentKey>,
    source_bytes: usize,
    #[cfg(test)]
    pub(super) normalized_bytes: usize,
    #[cfg(test)]
    pub(super) hits: usize,
}

impl WorkerCache {
    /// Reports cache retention for deterministic capacity tests.
    #[cfg(test)]
    pub(super) fn entries_len(&self) -> usize {
        self.entries.len()
    }
    /// Empty owners have no retained pixels to retire.
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Hashes cold sources on the worker and shares layers across independent wire allocations.
    pub(super) fn prepare(&mut self, source: &CapeImage) -> Option<SkinRgba8> {
        let span = tracing::info_span!(
            "cape.prepare",
            source_bytes = source.rgba8.len(),
            cache_hit = tracing::field::Empty
        );
        let _entered = span.enter();
        #[cfg(test)]
        CONTENT_BYTES_ON_THREAD.with(|bytes| bytes.set(bytes.get() + source.rgba8.len()));
        let key = ContentKey {
            source: source.clone(),
            hash: self
                .entries
                .hasher()
                .hash_one((source.width, source.height, &source.rgba8)),
        };
        if let Some(pixels) = self.entries.get(&key) {
            span.record("cache_hit", true);
            #[cfg(test)]
            {
                self.hits += 1;
            }
            return pixels.clone();
        }
        span.record("cache_hit", false);
        let pixels = cape_layer(source.width, source.height, &source.rgba8).map(SkinRgba8::new);
        #[cfg(test)]
        if pixels.is_some() {
            self.normalized_bytes += STANDARD_SKIN_BYTES;
        }
        while self.entries.len() >= MAX_RENDERED_PLAYERS
            || self.source_bytes.saturating_add(source.rgba8.len())
                > protocol::MAX_PLAYER_LIST_SKIN_BYTES
        {
            let Some(oldest) = self.order.pop_front() else {
                return pixels;
            };
            self.source_bytes -= oldest.source.rgba8.len();
            self.entries.remove(&oldest);
        }
        self.source_bytes += source.rgba8.len();
        self.order.push_back(key.clone());
        self.entries.insert(key, pixels.clone());
        pixels
    }
}

/// Resamples the full rectangle onto one standard layer without changing texels or UV mapping.
pub(super) fn cape_layer(width: u32, height: u32, rgba8: &[u8]) -> Option<Arc<[u8]>> {
    let (width, height) = (width as usize, height as usize);
    let expected = width.checked_mul(height)?.checked_mul(4)?;
    if width == 0 || height == 0 || rgba8.len() != expected {
        return None;
    }
    #[cfg(test)]
    CONTENT_BYTES_ON_THREAD.with(|bytes| bytes.set(bytes.get() + STANDARD_SKIN_BYTES * 2));
    let _span = tracing::info_span!("cape.normalize", bytes = STANDARD_SKIN_BYTES).entered();
    let mut layer = Vec::with_capacity(STANDARD_SKIN_BYTES);
    for y in 0..STANDARD_SKIN_SIDE {
        let source_y = y * height / STANDARD_SKIN_SIDE;
        for x in 0..STANDARD_SKIN_SIDE {
            let source_x = x * width / STANDARD_SKIN_SIDE;
            let offset = (source_y * width + source_x) * 4;
            layer.extend_from_slice(&rgba8[offset..offset + 4]);
        }
    }
    Some(layer.into())
}

#[cfg(test)]
thread_local! {
    /// Counts hashing and normalization work so caller-thread regressions fail deterministically.
    pub(super) static CONTENT_BYTES_ON_THREAD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_hashes_do_not_exchange_different_cape_content() {
        let first = ContentKey {
            source: CapeImage {
                width: 1,
                height: 1,
                rgba8: Arc::from([1, 2, 3, 4]),
            },
            hash: 7,
        };
        let mut changed = first.clone();
        changed.source.rgba8 = Arc::from([5, 6, 7, 8]);
        let entries = HashMap::from([(first.clone(), 1), (changed.clone(), 2)]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[&first], 1);
        assert_eq!(entries[&changed], 2);
    }
}
