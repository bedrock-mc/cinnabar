//! Label text through the client's own text stack: the compiled Cinnangles Sans
//! carrier, `ui`'s layout cache, and `ui-engine`'s label metrics, measurement
//! and localization. Without a carrier, text measures with a fixed-advance
//! fallback and paints as bars.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use assets::CompiledFontCatalog;
use json_ui::TextMeasure;
use ui::{DpiScale, FONT_DESIGN_PIXEL_TEXELS, TextLayout, TextLayoutCache, TextLayoutRequest};
use ui_engine::engine::text_paint::{self, UNWRAPPED_LOGICAL};
use ui_engine::text_metrics::TextMetrics;

const CACHE_ENTRIES: usize = 4_096;
const CACHE_BYTES: usize = 32 * 1024 * 1024;
/// Fallback metrics in GUI px: Minecraft's 6 px average advance and 9 px pitch.
const FALLBACK_ADVANCE: f64 = 6.0;
const FALLBACK_LINE: f64 = 9.0;

/// The font source manifest the carrier was compiled against.
const FONT_SOURCE_MANIFEST: &[u8] = include_bytes!("../../../assets/cinnangles-sans-source.json");

pub struct Fonts {
    font: Option<CompiledFontCatalog>,
    revision: u64,
    cache: RefCell<TextLayoutCache>,
    lang: Arc<HashMap<String, String>>,
}

impl Default for Fonts {
    fn default() -> Self {
        Self {
            font: None,
            revision: 0,
            cache: RefCell::new(TextLayoutCache::new(CACHE_ENTRIES, CACHE_BYTES)),
            lang: Arc::default(),
        }
    }
}

impl Fonts {
    /// Load a compiled font carrier built from the pinned Cinnangles Sans manifest.
    pub fn load(&mut self, bytes: &[u8]) -> Result<(), String> {
        let manifest = assets::canonical_source_manifest_sha256(FONT_SOURCE_MANIFEST);
        let font = CompiledFontCatalog::decode(bytes, manifest).map_err(|e| format!("{e:?}"))?;
        self.font = Some(font);
        self.revision = self.revision.wrapping_add(1);
        self.cache = RefCell::new(TextLayoutCache::new(CACHE_ENTRIES, CACHE_BYTES));
        Ok(())
    }

    /// Changes only when a validated font carrier replaces the active metrics.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub fn font(&self) -> Option<&CompiledFontCatalog> {
        self.font.as_ref()
    }

    pub fn set_lang(&mut self, lang: Arc<HashMap<String, String>>) {
        self.lang = lang;
    }

    pub fn translate(&self, key: &str) -> Option<Arc<str>> {
        self.lang.get(key).map(|value| Arc::from(value.as_str()))
    }

    /// `request` laid out through the shared cache; `None` when it cannot lay out.
    pub fn layout(&self, request: TextLayoutRequest<'_>) -> Option<Arc<TextLayout>> {
        self.cache.borrow_mut().layout(request).ok()
    }

    /// Runs `run` with the engine's label measurement, or the fixed-advance fallback without a
    /// font carrier.
    pub fn with_measure<R>(
        &self,
        metrics: TextMetrics,
        run: impl FnOnce(&dyn TextMeasure) -> R,
    ) -> R {
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let Some(font) = &self.font else {
            return run(&Fallback { fonts: self, px });
        };
        let translate = |key: &str| self.translate(key);
        let mut cache = self.cache.borrow_mut();
        let layouts = RefCell::new(&mut *cache);
        run(&text_paint::Measure {
            layouts: &layouts,
            font,
            metrics,
            px,
            translate: &translate,
        })
    }
}

/// The client's text metrics for a `size` physical-pixel window at DPI 1; `px` is the GUI scale
/// [`ui::gui_scale`] chose for that window.
pub fn metrics(size: [u32; 2], px: u32) -> TextMetrics {
    let dpi = DpiScale::new(1.0).expect("1 is a supported DPI scale");
    TextMetrics::for_viewport(size, dpi, Some(u8::try_from(px).unwrap_or(u8::MAX)))
}

/// Measurement without a font carrier.
struct Fallback<'a> {
    fonts: &'a Fonts,
    /// Logical px per GUI px.
    px: f32,
}

impl TextMeasure for Fallback<'_> {
    fn extent(&self, text: &str) -> [f64; 2] {
        self.wrapped(text, UNWRAPPED_LOGICAL / f64::from(self.px))
    }

    fn wrapped(&self, text: &str, max_width: f64) -> [f64; 2] {
        if text.is_empty() {
            return [0.0, 0.0];
        }
        fallback_extent(text, max_width)
    }

    fn localize<'t>(&self, text: &'t str) -> Cow<'t, str> {
        text_paint::localized(text, &|key: &str| self.fonts.translate(key))
    }
}

/// Fixed-advance extent in GUI px, wrapping greedily at `max_width`.
pub fn fallback_extent(text: &str, max_width: f64) -> [f64; 2] {
    let per_line = (max_width / FALLBACK_ADVANCE).floor().max(1.0) as usize;
    let mut width = 0usize;
    let mut lines = 0usize;
    for line in text.split('\n') {
        let count = line.chars().count();
        width = width.max(count.min(per_line));
        lines += count.div_ceil(per_line).max(1);
    }
    [
        width as f64 * FALLBACK_ADVANCE,
        lines as f64 * FALLBACK_LINE,
    ]
}
