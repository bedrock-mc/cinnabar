//! Bounded text retention with verified content and linked LRU order.

use super::*;

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct CacheKey {
    content_hash: u128,
    // The content digest is zero here: the fast hash selects a candidate and
    // retained text verifies it. Published layout keys still contain SHA-256.
    parameters: TextLayoutKey,
    single_line_width: Option<u32>,
}

struct CacheEntry {
    text: Box<str>,
    layout: Arc<TextLayout>,
    retained_bytes: usize,
    previous: Option<CacheKey>,
    next: Option<CacheKey>,
}

pub struct TextLayoutCache {
    entry_cap: usize,
    byte_cap: usize,
    retained_bytes: usize,
    next_id: u64,
    clock: u64,
    visual_overflows: u64,
    entries: BTreeMap<CacheKey, CacheEntry>,
    oldest: Option<CacheKey>,
    newest: Option<CacheKey>,
    #[cfg(test)]
    work: CacheWork,
}

#[cfg(test)]
#[derive(Default)]
struct CacheWork {
    sha256_calls: usize,
    eviction_candidates: usize,
    forced_content_hash: Option<u128>,
}

impl TextLayoutCache {
    pub fn new(entry_cap: usize, byte_cap: usize) -> Self {
        Self {
            entry_cap,
            byte_cap,
            retained_bytes: 0,
            next_id: 1,
            clock: 0,
            visual_overflows: 0,
            entries: BTreeMap::new(),
            oldest: None,
            newest: None,
            #[cfg(test)]
            work: CacheWork::default(),
        }
    }

    /// Lays out the longest UTF-8 prefix within the text byte limit.
    /// Cache identity and retention use that same prefix.
    pub fn layout(
        &mut self,
        mut request: TextLayoutRequest<'_>,
    ) -> Result<Arc<TextLayout>, TextError> {
        request.text = text_prefix(request.text, crate::UiLimits::MAX_TEXT_BYTES);
        validate_request(request)?;
        let key = self.cache_key(request, None);
        self.advance_clock()?;
        if let Some(layout) = self.lookup(&key, request.text) {
            return Ok(layout);
        }

        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(TextError::CacheCounterOverflow)?;
        #[cfg(test)]
        {
            self.work.sha256_calls += 1;
        }
        let digest = Sha256::digest(request.text.as_bytes()).into();
        let layout = Arc::new(build_layout(id, layout_key(request, digest), request)?);
        if layout.size_64()[0] > request.width_64 {
            self.visual_overflows = self.visual_overflows.saturating_add(1);
        }
        self.retain(key, request.text, layout)
    }

    /// Counts newly shaped layouts with ink wider than their wrapping control.
    pub const fn visual_overflow_count(&self) -> u64 {
        self.visual_overflows
    }

    /// Counts shaping attempts, including failed and uncached layouts.
    pub const fn built_layout_count(&self) -> u64 {
        self.next_id - 1
    }

    /// Indexes content without calculating the published SHA-256 layout digest.
    pub(super) fn cache_key(
        &self,
        request: TextLayoutRequest<'_>,
        single_line_width: Option<u32>,
    ) -> CacheKey {
        let content_hash = xxhash_rust::xxh3::xxh3_128(request.text.as_bytes());
        #[cfg(test)]
        let content_hash = self.work.forced_content_hash.unwrap_or(content_hash);
        CacheKey {
            content_hash,
            parameters: layout_key(request, [0; 32]),
            single_line_width,
        }
    }

    /// Verifies a hash candidate's source bytes before updating its LRU position.
    pub(super) fn lookup(&mut self, key: &CacheKey, text: &str) -> Option<Arc<TextLayout>> {
        let entry = self.entries.get(key)?;
        if entry.text.as_ref() != text {
            return None;
        }
        let layout = Arc::clone(&entry.layout);
        if self.newest.as_ref() != Some(key) {
            self.unlink(key);
            self.link_newest(key);
        }
        Some(layout)
    }

    /// Shares one bounded budget between wrapped and normalized layouts.
    pub(super) fn retain(
        &mut self,
        key: CacheKey,
        text: &str,
        layout: Arc<TextLayout>,
    ) -> Result<Arc<TextLayout>, TextError> {
        let retained_bytes = retained_layout_bytes(&layout, text.len())?;
        if self.entry_cap == 0 || retained_bytes > self.byte_cap {
            return Ok(layout);
        }

        // A collision may replace a candidate, but can never reuse its output.
        self.remove(&key);
        while self.entries.len() >= self.entry_cap
            || self
                .retained_bytes
                .checked_add(retained_bytes)
                .is_none_or(|bytes| bytes > self.byte_cap)
        {
            if !self.evict_lru() {
                return Ok(layout);
            }
        }
        self.retained_bytes = self
            .retained_bytes
            .checked_add(retained_bytes)
            .ok_or(TextError::FixedPointOverflow)?;
        self.entries.insert(
            key.clone(),
            CacheEntry {
                text: text.into(),
                layout: Arc::clone(&layout),
                retained_bytes,
                previous: None,
                next: None,
            },
        );
        self.link_newest(&key);
        Ok(layout)
    }

    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Preserves checked request ordering even for uncached and failed layouts.
    pub(super) fn advance_clock(&mut self) -> Result<(), TextError> {
        self.clock = self
            .clock
            .checked_add(1)
            .ok_or(TextError::CacheCounterOverflow)?;
        Ok(())
    }

    /// Removes a retained entry from the LRU chain without reallocating it.
    fn unlink(&mut self, key: &CacheKey) {
        let entry = self.entries.get_mut(key).expect("retained LRU entry");
        let previous = entry.previous.take();
        let next = entry.next.take();
        if let Some(previous) = &previous {
            self.entries
                .get_mut(previous)
                .expect("previous LRU entry")
                .next = next.clone();
        } else {
            self.oldest = next.clone();
        }
        if let Some(next) = &next {
            self.entries.get_mut(next).expect("next LRU entry").previous = previous;
        } else {
            self.newest = previous;
        }
    }

    /// Appends an unlinked retained entry after the newest entry.
    fn link_newest(&mut self, key: &CacheKey) {
        let previous = self.newest.replace(key.clone());
        if let Some(previous) = &previous {
            self.entries
                .get_mut(previous)
                .expect("newest LRU entry")
                .next = Some(key.clone());
        } else {
            self.oldest = Some(key.clone());
        }
        self.entries
            .get_mut(key)
            .expect("retained LRU entry")
            .previous = previous;
    }

    /// Removes one indexed entry and releases its complete retention charge.
    fn remove(&mut self, key: &CacheKey) {
        if self.entries.contains_key(key) {
            self.unlink(key);
            let removed = self.entries.remove(key).expect("retained LRU entry");
            self.retained_bytes -= removed.retained_bytes;
        }
    }

    /// Selects the oldest entry directly, without walking the retained map.
    fn evict_lru(&mut self) -> bool {
        let Some(key) = self.oldest.clone() else {
            return false;
        };
        #[cfg(test)]
        {
            self.work.eviction_candidates += 1;
        }
        self.remove(&key);
        true
    }
}

/// Conservatively charges the layout, verified source, map storage and LRU links.
fn retained_layout_bytes(layout: &TextLayout, source_bytes: usize) -> Result<usize, TextError> {
    let glyph_bytes = layout
        .glyphs
        .len()
        .checked_mul(size_of::<GlyphQuad>())
        .ok_or(TextError::FixedPointOverflow)?;
    let arc_allocation = conservative_allocation_bytes(
        size_of::<TextLayout>()
            .checked_add(2 * size_of::<usize>())
            .ok_or(TextError::FixedPointOverflow)?,
    )?;
    [
        arc_allocation,
        conservative_allocation_bytes(glyph_bytes)?,
        conservative_allocation_bytes(
            layout
                .source_indices
                .len()
                .checked_mul(size_of::<Option<usize>>())
                .ok_or(TextError::FixedPointOverflow)?,
        )?,
        conservative_allocation_bytes(source_bytes)?,
        conservative_allocation_bytes(
            layout
                .caret_stops_64
                .len()
                .checked_mul(size_of::<(usize, i32)>())
                .ok_or(TextError::FixedPointOverflow)?,
        )?,
        size_of::<CacheKey>(),
        size_of::<CacheEntry>(),
        CONSERVATIVE_BTREE_NODE_BYTES,
        ALLOCATOR_METADATA_BYTES,
    ]
    .into_iter()
    .try_fold(0usize, |total, bytes| total.checked_add(bytes))
    .ok_or(TextError::FixedPointOverflow)
}
