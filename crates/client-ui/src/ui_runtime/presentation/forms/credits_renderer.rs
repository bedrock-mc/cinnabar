//! The End credits' scrolling text, drawn by `credits_renderer` controls.

use std::{
    cell::Cell,
    sync::{Arc, Mutex},
};

use json_ui::{TextAlign, TextureSource};

use super::engine::{
    Canvas,
    screen::{CustomDraw, CustomRenderer, PaintError},
    text_paint::{TextPaint, scaled_request, width_64},
};

const RENDERER: &str = "credits_renderer";
const TITLE_TEXTURE: &str = super::super::menu_artwork::TITLE_KEY;

pub(super) struct MeasuredCredits {
    content: Arc<super::credits_content::Content>,
    player_name: String,
    font: assets::FontCatalogIdentity,
    width: u32,
    px: u32,
    rows: Vec<MeasuredRow>,
    height: f32,
}

struct MeasuredRow {
    text: Arc<str>,
    centered: bool,
    top: f32,
    height: f32,
}

/// What one credits paint scrolls through, and where it records that the text has ended.
pub(super) struct CreditsRenderer<'a> {
    pub(super) content: Arc<super::credits_content::Content>,
    pub(super) player_name: &'a str,
    pub(super) scroll_pixels: f64,
    pub(super) finished: &'a Cell<bool>,
    pub(super) layout: &'a Mutex<Option<Arc<MeasuredCredits>>>,
}

impl CustomRenderer for CreditsRenderer<'_> {
    fn paint(
        &mut self,
        node: &CustomDraw<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<bool, PaintError> {
        if node.renderer != RENDERER {
            return Ok(false);
        }
        self.credits(canvas, node.dest, &|color| node.alpha(color));
        Ok(true)
    }

    fn textures<'s>(&'s self, drawn: &dyn Fn(&str) -> bool, texture: &mut dyn FnMut(&'s str)) {
        if drawn(RENDERER) {
            texture(TITLE_TEXTURE);
        }
    }
}

impl CreditsRenderer<'_> {
    fn credits(&self, canvas: &mut Canvas<'_>, dest: [f32; 4], alpha: &dyn Fn([u8; 4]) -> [u8; 4]) {
        let (px, metrics, font) = (canvas.px(), canvas.metrics(), canvas.font());
        let width = dest[2] - dest[0];
        let mut y = dest[3] - self.scroll_pixels as f32 * px;
        let logo = TITLE_TEXTURE;
        if let Some(metadata) = canvas.textures().texture(logo) {
            let [w, h] = metadata.pixels.map(|side| side as f32);
            let logo_width = width * 0.9;
            let height = ((width / px) * h / w.max(1.0) * 0.9).trunc() * px;
            let left = dest[0] + (width - logo_width) * 0.5;
            if let Some(visual) = canvas.sprite(
                logo,
                json_ui::UvRect {
                    u0: 0.0,
                    v0: 0.0,
                    u1: 1.0,
                    v1: 1.0,
                },
                alpha([255; 4]),
                Default::default(),
            ) {
                let _ = canvas.push(visual, [left, y, left + logo_width, y + height]);
            }
            y += height;
        }
        let measured = {
            let mut cache = self
                .layout
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let fresh = cache.as_ref().is_some_and(|old| {
                Arc::ptr_eq(&old.content, &self.content)
                    && old.player_name == self.player_name
                    && old.font == font.identity()
                    && old.width == width.to_bits()
                    && old.px == px.to_bits()
            });
            if !fresh {
                let mut top = 0.0;
                let mut rows = Vec::with_capacity(self.content.rows.len());
                for row in self.content.rows.iter() {
                    let text: Arc<str> = row.text.replace("PLAYERNAME", self.player_name).into();
                    let height = if row.blank_line {
                        metrics.line_height_64 as f32 / 64.0 * metrics.scale.get()
                    } else if text.is_empty() {
                        0.0
                    } else {
                        let request =
                            scaled_request(&metrics, &text, width_64(f64::from(width)), font, 1.0);
                        canvas
                            .layouts()
                            .layout(request)
                            .map(|layout| layout.size_64()[1] as f32 / 64.0)
                            .unwrap_or(0.0)
                    };
                    rows.push(MeasuredRow {
                        text,
                        centered: row.centered,
                        top,
                        height,
                    });
                    top += height + row.gap * px;
                }
                *cache = Some(Arc::new(MeasuredCredits {
                    content: Arc::clone(&self.content),
                    player_name: self.player_name.to_owned(),
                    font: font.identity(),
                    width: width.to_bits(),
                    px: px.to_bits(),
                    rows,
                    height: top,
                }));
            }
            Arc::clone(cache.as_ref().expect("credits layout just measured"))
        };
        let start = measured
            .rows
            .partition_point(|row| y + row.top + row.height < dest[1]);
        for row in measured.rows[start..]
            .iter()
            .take_while(|row| y + row.top <= dest[3])
        {
            let top = y + row.top;
            if !row.text.is_empty() {
                let clip = canvas.clip().unwrap_or(dest);
                let _ = canvas.text(
                    &row.text,
                    [dest[0], top, dest[2], top + row.height],
                    clip,
                    TextPaint {
                        color: alpha([255; 4]),
                        edit: None,
                        shadow: metrics.shadow(),
                        align: if row.centered {
                            TextAlign::Center
                        } else {
                            TextAlign::Left
                        },
                        scale: 1.0,
                        localize: false,
                        options: Default::default(),
                    },
                );
            }
        }
        self.finished.set(y + measured.height < dest[1]);
    }
}
