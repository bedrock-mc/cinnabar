//! Label text through the client's own text stack: the compiled Cinnangles Sans
//! carrier, `ui`'s layout cache and metrics, and the engine's localization.
//! Without a carrier, text measures with a fixed-advance fallback and paints
//! as bars.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use assets::CompiledFontCatalog;
use ui::{
    FONT_DESIGN_PIXEL_TEXELS, TEXT_BASELINE_64, TEXT_LINE_HEIGHT_64, TEXT_SHADOW_OFFSET_64,
    TextLayout, TextLayoutCache, TextLayoutRequest, TextShadow, TextStyle, UiScale,
};

/// Largest wrap width handed to the layout (logical px), for "no wrap".
const UNWRAPPED_LOGICAL: f64 = 65_536.0;
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

    /// A label's text after the vanilla localization rules; empty lines drop,
    /// as the vanilla label splits on newlines and discards them.
    pub fn localized<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let text = json_ui::localize_text(text, &|key| self.translate(key));
        if text.contains("\n\n") || text.starts_with('\n') || text.ends_with('\n') {
            let lines: Vec<&str> = text.split('\n').filter(|line| !line.is_empty()).collect();
            return Cow::Owned(lines.join("\n"));
        }
        text
    }

    /// Lay out `text` at `width` logical px for GUI scale `px`, `factor` times its size.
    pub fn layout(&self, text: &str, width: f64, px: f32, factor: f32) -> Option<Arc<TextLayout>> {
        let font = self.font.as_ref()?;
        let base = (px / FONT_DESIGN_PIXEL_TEXELS as f32).clamp(UiScale::MIN, UiScale::MAX);
        let mut scale = UiScale::new(base).ok()?;
        // As the client: a factor the scale range cannot hold draws unscaled.
        if factor != 1.0
            && let Ok(scaled) = UiScale::new(base * factor)
        {
            scale = scaled;
        }
        let request = TextLayoutRequest {
            text,
            style: TextStyle::default(),
            width_64: width_64(width),
            line_height_64: TEXT_LINE_HEIGHT_64,
            baseline_64: TEXT_BASELINE_64,
            scale,
            font,
            wrap: Default::default(),
        };
        self.cache.borrow_mut().layout(request).ok()
    }

    pub fn shadow(&self) -> TextShadow {
        TextShadow::Offset64(TEXT_SHADOW_OFFSET_64)
    }

    /// The engine's measurement backend at GUI scale `px`.
    pub fn measure(&self, px: f32) -> Measure<'_> {
        Measure { fonts: self, px }
    }
}

/// Rounded up, so text laid out at its own measured width does not wrap.
pub fn width_64(logical: f64) -> u32 {
    (logical.clamp(1.0, UNWRAPPED_LOGICAL) * 64.0).ceil() as u32
}

pub struct Measure<'a> {
    fonts: &'a Fonts,
    px: f32,
}

impl json_ui::TextMeasure for Measure<'_> {
    fn extent(&self, text: &str) -> [f64; 2] {
        self.wrapped(text, UNWRAPPED_LOGICAL / f64::from(self.px))
    }

    fn wrapped(&self, text: &str, max_width: f64) -> [f64; 2] {
        if text.is_empty() {
            return [0.0, 0.0];
        }
        let px = f64::from(self.px);
        if self.fonts.font.is_none() {
            return fallback_extent(text, max_width);
        }
        match self.fonts.layout(text, max_width * px, self.px, 1.0) {
            Some(layout) => {
                let [w, h] = layout.size_64();
                [f64::from(w) / 64.0 / px, f64::from(h) / 64.0 / px]
            }
            None => [0.0, 0.0],
        }
    }

    fn localize<'t>(&self, text: &'t str) -> Cow<'t, str> {
        self.fonts.localized(text)
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
