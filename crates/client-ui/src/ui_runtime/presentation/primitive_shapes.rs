//! Retained debug text uses the same font layout and glyph atlas as world name tags.

use std::sync::Arc;

use assets::RuntimeFontCatalog;
use render_api::primitive_shapes::PrimitiveText;
use render_model::NAMETAG_ATLAS_SIDE;
use render_model::primitive_shapes::{PrimitiveShapeStore, PrimitiveTextRecord};
use ui::{FONT_DESIGN_PIXEL_TEXELS, FormattingPalette, TextLayoutCache};

use super::{UiPresentationRuntime, UiRuntime};
use view_presentation::nametag_atlas::{AtlasLine, GlyphPage, NametagAtlas, font_page};
use view_presentation::nametags::{EXTRA_LINE_LIFT, LINE_PITCH_PX, PLATE_COLOR};

/// One retained atlas, invalidated only by changed text, font or formatting colors.
#[derive(Default)]
pub struct PrimitiveTextRasterizer {
    atlas: NametagAtlas,
    font: Option<Arc<RuntimeFontCatalog>>,
    palette: Option<FormattingPalette>,
}

impl UiPresentationRuntime {
    /// Rasterizes changed debug strings with the current session font and text resolver.
    pub fn prepare_primitive_text(&mut self, store: &mut PrimitiveShapeStore, runtime: &UiRuntime) {
        let palette = self.formatting_palette().copied().unwrap_or_default();
        let (font, glyphs) = (&self.font, &self.session_glyphs);
        let dynamic_start = self.textures.dynamic_start();
        self.primitive_text.invalidate(store, font, palette);
        self.primitive_text
            .prepare(store, font, &mut self.layouts, runtime, &|page| {
                font_page(font, page).or_else(|| glyphs.page(dynamic_start, page))
            });
    }
}

impl PrimitiveTextRasterizer {
    /// Font and palette replacement invalidate all retained atlas coordinates together.
    fn invalidate(
        &mut self,
        store: &mut PrimitiveShapeStore,
        font: &Arc<RuntimeFontCatalog>,
        palette: FormattingPalette,
    ) {
        let font_changed = self.font.as_ref().is_none_or(|old| !Arc::ptr_eq(old, font));
        if font_changed || self.palette != Some(palette) {
            self.atlas.reset();
            self.atlas.set_palette(palette);
            self.font = Some(Arc::clone(font));
            self.palette = Some(palette);
            store.queue_all_text();
        }
    }

    /// Updates changed slots only; unchanged frames neither rasterize nor republish pixels.
    fn prepare<'p>(
        &mut self,
        store: &mut PrimitiveShapeStore,
        font: &RuntimeFontCatalog,
        layouts: &mut TextLayoutCache,
        runtime: &UiRuntime,
        pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
    ) {
        let mut changes = store.take_text_changes();
        if changes.is_empty() {
            return;
        }
        if !self.atlas.has_room_for(changes.len()) {
            self.atlas.reset();
            store.queue_all_text();
            changes = store.take_text_changes();
        }
        for change in changes {
            let (text, dynamic) = resolve_text(&change.text.text, runtime);
            store.set_text_dynamic(change.network_id, dynamic);
            let placed: Vec<_> = text
                .split('\n')
                .filter(|line| !line.is_empty())
                .filter_map(|line| self.atlas.line(&Arc::from(line), font, layouts, pages))
                .collect();
            store.set_text_records(change.network_id, records(&change.text, &placed));
        }
        store.atlas = self.atlas.publish().0;
    }
}

/// Valid text objects resolve against the existing UI authority before escaped line splitting.
fn resolve_text(text: &str, runtime: &UiRuntime) -> (String, bool) {
    let (text, dynamic) = match protocol::parse_raw_text(text) {
        Ok(document) => (runtime.resolve_raw_text(&document).text, true),
        Err(_) => (text.to_owned(), false),
    };
    (text.replace("\\n", "\n"), dynamic)
}

/// Builds font-pixel quads; the shader applies shape color, facing and world scale.
fn records(text: &PrimitiveText, lines: &[AtlasLine]) -> Vec<PrimitiveTextRecord> {
    if lines.is_empty() {
        return Vec::new();
    }
    let flags = u32::from(text.depth_test)
        | (u32::from(text.show_backface) << 1)
        | (u32::from(text.show_text_backface) << 2)
        | (u32::from(text.use_rotation) << 3);
    let lift = EXTRA_LINE_LIFT * lines.len().saturating_sub(1) as f32;
    let meta = [0, 1, flags, lift.to_bits()];
    let half = lines
        .iter()
        .map(|line| line.width_px as u32 / 2)
        .max()
        .unwrap_or(0) as f32;
    let mut records = Vec::with_capacity(lines.len() + 1);
    if lines.len() > 1 || half > 0.0 {
        records.push(PrimitiveTextRecord {
            rect: [
                -(half + 1.0),
                -1.0,
                half + 1.0,
                LINE_PITCH_PX * lines.len() as f32 - 1.0,
            ],
            uv: [0.0, 0.0, -1.0, -1.0],
            color: text.background_color.unwrap_or(PLATE_COLOR),
            meta,
        });
    }
    let texels = FONT_DESIGN_PIXEL_TEXELS as f32;
    let side = NAMETAG_ATLAS_SIDE as f32;
    for (index, line) in lines.iter().enumerate() {
        let left = -((line.width_px as u32 / 2) as f32);
        let [x, y, width, height] = line.cell.map(|value| value as f32);
        let top = LINE_PITCH_PX * index as f32 + line.top_px;
        records.push(PrimitiveTextRecord {
            rect: [left, top, left + width / texels, top + height / texels],
            uv: [x / side, y / side, (x + width) / side, (y + height) / side],
            color: [1.0; 4],
            meta,
        });
    }
    records
}

#[cfg(test)]
mod tests;
