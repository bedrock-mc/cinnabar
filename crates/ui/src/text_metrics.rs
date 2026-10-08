//! Per-frame text metrics shared by every HUD, chat, scoreboard and nametag run.

use crate::{DpiScale, TextLayoutRequest, TextShadow, TextStyle, UiScale};
use assets::RuntimeFontCatalog;

use crate::gui_scale;

use crate::{
    FONT_DESIGN_PIXEL_TEXELS, TEXT_BASELINE_64, TEXT_LINE_HEIGHT_64, TEXT_SHADOW_OFFSET_64,
};

/// Per-frame text metrics shared by every HUD, chat, and scoreboard run so a
/// single frame cannot mix scales or line pitches. Font atlas texels are two
/// texels per GUI design pixel, while sprite geometry uses one GUI pixel.
#[derive(Clone, Copy)]
pub struct TextMetrics {
    pub scale: UiScale,
    /// Physical pixels per GUI unit: the GUI scale, on whose pixel grid draws land.
    pub gui_scale: f32,
    pub dpi_scale: DpiScale,
    pub line_height_64: u32,
    pub baseline_64: u32,
    shadow: TextShadow,
}

impl TextMetrics {
    /// Uses the same GUI-scale choice as sprite geometry. The font atlas
    /// is authored at two texels per GUI design pixel, so its logical scale is
    /// half the sprite scale before the platform DPI is removed.
    pub fn for_viewport(
        physical_size: [u32; 2],
        dpi_scale: DpiScale,
        preference: Option<u8>,
    ) -> Self {
        let dpi = dpi_scale.get();
        let k = gui_scale(physical_size, preference) as f32;
        let scale = k / (FONT_DESIGN_PIXEL_TEXELS as f32 * dpi);
        Self {
            scale: UiScale::new_display(scale)
                .expect("supported GUI scale and DPI produce a valid display scale"),
            gui_scale: k,
            dpi_scale,
            line_height_64: TEXT_LINE_HEIGHT_64,
            baseline_64: TEXT_BASELINE_64,
            shadow: TextShadow::Offset64(TEXT_SHADOW_OFFSET_64),
        }
    }

    pub fn request<'a>(
        &self,
        text: &'a str,
        width_64: u32,
        font: &'a RuntimeFontCatalog,
    ) -> TextLayoutRequest<'a> {
        TextLayoutRequest {
            text,
            style: TextStyle::default(),
            width_64,
            line_height_64: self.line_height_64,
            baseline_64: self.baseline_64,
            scale: self.scale,
            font,
            wrap: Default::default(),
        }
    }

    pub const fn shadow(&self) -> TextShadow {
        self.shadow
    }
}

pub const DEFAULT_TEXT_CACHE_ENTRIES: usize = 1_024;
pub const DEFAULT_TEXT_CACHE_BYTES: usize = 8 * 1024 * 1024;
