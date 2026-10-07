//! Shared, demand-filled outline fallback pages and bounded off-thread requests.

use super::*;
use std::{
    collections::BTreeSet,
    sync::{Condvar, Mutex},
    time::Duration,
};

#[derive(Debug, Default)]
struct Requests {
    seen: BTreeSet<char>,
    pending: BTreeSet<char>,
    locale: String,
    changed: bool,
}

#[derive(Debug, Default)]
pub struct FontGlyphRequests {
    state: Mutex<Requests>,
    wake: Condvar,
    ready: Mutex<Option<Arc<RuntimeFontCatalog>>>,
}

impl PartialEq for FontGlyphRequests {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
impl Eq for FontGlyphRequests {}

impl FontGlyphRequests {
    pub fn seed(&self, characters: impl IntoIterator<Item = char>) {
        if let Ok(mut state) = self.state.lock() {
            state
                .seen
                .extend(characters.into_iter().take(MAX_FONT_GLYPHS));
        }
    }

    /// Evicted glyphs can be requested again; unsupported scalars remain deduplicated.
    pub fn forget(&self, characters: impl IntoIterator<Item = char>) {
        if let Ok(mut state) = self.state.lock() {
            for ch in characters {
                state.seen.remove(&ch);
            }
        }
    }

    fn request(&self, ch: char) {
        if let Ok(mut state) = self.state.lock()
            && state.seen.len() < MAX_FONT_GLYPHS
            && state.seen.insert(ch)
        {
            state.pending.insert(ch);
            self.wake.notify_one();
        }
    }

    pub fn set_locale(&self, locale: &str) {
        let locale = match locale {
            value if value.starts_with("ja") => "ja",
            value if value.starts_with("ko") => "ko",
            value if value.starts_with("zh_TW") => "zh_TW",
            value if value.starts_with("zh") => "zh_CN",
            value if value.starts_with("ar") => "ar",
            _ => "",
        };
        if let Ok(mut state) = self.state.lock()
            && state.locale != locale
        {
            state.locale = locale.into();
            state.changed = true;
            self.wake.notify_one();
        }
    }

    /// The worker alone waits; the render thread only queues missing characters.
    pub fn wait(&self) -> Option<(String, Vec<char>)> {
        let state = self.state.lock().ok()?;
        let (mut state, _) = self
            .wake
            .wait_timeout_while(state, Duration::from_millis(500), |s| {
                s.pending.is_empty() && !s.changed
            })
            .ok()?;
        if state.pending.is_empty() && !state.changed {
            return None;
        }
        state.changed = false;
        Some((
            state.locale.clone(),
            std::mem::take(&mut state.pending).into_iter().collect(),
        ))
    }

    pub fn publish(&self, font: Arc<RuntimeFontCatalog>) {
        if let Ok(mut ready) = self.ready.lock() {
            *ready = Some(font);
        }
    }

    pub fn take_ready(&self) -> Option<Arc<RuntimeFontCatalog>> {
        self.ready.try_lock().ok()?.take()
    }
}

impl CompiledFontCatalog {
    /// Preserves the primary face's metrics and resolves missing characters through its fallback.
    pub fn glyph_source(&self, ch: char) -> Option<(&Self, GlyphMetrics)> {
        if let Some(glyph) = self.glyph(ch) {
            return Some((self, *glyph));
        }
        if let Some(fallback) = &self.fallback {
            if let Some(glyph) = fallback.glyph(ch) {
                return Some((fallback, *glyph));
            }
            if let Some(requests) = &self.requests {
                requests.request(ch);
            }
        }
        None
    }

    pub fn glyph_requests(&self) -> Option<&Arc<FontGlyphRequests>> {
        self.requests.as_ref()
    }

    /// Shares one appended fallback atlas across all native families and exact raster sizes.
    pub fn with_shared_fallback(
        &self,
        font: &Self,
        requests: Arc<FontGlyphRequests>,
    ) -> Result<Self, FontCatalogError> {
        let mut pages = self.pages.to_vec();
        let mut fallback = runtime::append(font, &mut pages)?;
        let pages: Arc<[FontTexturePage]> = pages.into();
        runtime::share_pages(&mut fallback, &pages);
        let mut result = self.with_named_fallback(Arc::new(fallback), requests);
        result.pages = pages;
        let pages = Arc::clone(&result.pages);
        for alias in Arc::make_mut(&mut result.named).values_mut() {
            runtime::share_pages(alias, &pages);
        }
        Ok(result)
    }

    /// Dynamic page indices are owned by presentation; primary and HUD atlas indices stay fixed.
    pub fn with_named_fallback(&self, font: Arc<Self>, requests: Arc<FontGlyphRequests>) -> Self {
        let mut result = self.clone();
        result.requests = Some(Arc::clone(&requests));
        for alias in Arc::make_mut(&mut result.named).values_mut() {
            assign(alias, &font, &requests);
        }
        let mut hash = Sha256::new();
        hash.update(self.identity.carrier_sha256);
        hash.update(font.identity.carrier_sha256);
        result.identity.carrier_sha256 = hash.finalize().into();
        result
    }

    pub fn with_page_offset(&self, offset: u16) -> Result<Self, FontCatalogError> {
        let mut result = self.clone();
        for glyph in &mut result.glyphs {
            glyph.page = glyph
                .page
                .checked_add(offset)
                .ok_or_else(|| invalid_catalog("fallback page exceeds bounds"))?;
        }
        let mut hash = Sha256::new();
        hash.update(self.identity.carrier_sha256);
        hash.update(offset.to_le_bytes());
        result.identity.carrier_sha256 = hash.finalize().into();
        Ok(result)
    }
}

fn assign(
    font: &mut CompiledFontCatalog,
    fallback: &Arc<CompiledFontCatalog>,
    requests: &Arc<FontGlyphRequests>,
) {
    if font.line_metrics.is_some() {
        font.fallback = Some(Arc::clone(fallback));
        font.requests = Some(Arc::clone(requests));
        let mut hash = Sha256::new();
        hash.update(font.identity.carrier_sha256);
        hash.update(fallback.identity.carrier_sha256);
        font.identity.carrier_sha256 = hash.finalize().into();
    }
    for alternate in Arc::make_mut(&mut font.sizes).values_mut() {
        assign(alternate, fallback, requests);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_characters_are_deduplicated_and_evicted_characters_can_return() {
        let requests = FontGlyphRequests::default();
        requests.request('日');
        requests.request('日');
        requests.request('한');
        assert_eq!(requests.wait().unwrap().1, vec!['日', '한']);
        requests.forget(['日']);
        requests.request('日');
        assert_eq!(requests.wait().unwrap().1, vec!['日']);
    }

    #[test]
    fn locale_changes_invalidate_once_and_group_equivalent_locale_codes() {
        let requests = FontGlyphRequests::default();
        requests.set_locale("ja_JP");
        let (locale, glyphs) = requests.wait().unwrap();
        assert_eq!(locale, "ja");
        assert!(glyphs.is_empty());
        requests.set_locale("ja_JP");
        assert!(!requests.state.lock().unwrap().changed);
        requests.set_locale("zh_TW");
        assert_eq!(requests.wait().unwrap().0, "zh_TW");
    }
}
