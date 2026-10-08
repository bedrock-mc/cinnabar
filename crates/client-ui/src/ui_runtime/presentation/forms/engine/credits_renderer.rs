use std::sync::Arc;

use json_ui::{TextAlign, TextureSource};

use super::{FormEngine, Painter, TextPaint};

pub(super) const RENDERER: &str = "credits_renderer";
pub(super) const TITLE_TEXTURE: &str = super::super::super::menu_artwork::TITLE_KEY;

pub(in super::super) struct MeasuredCredits {
    content: Arc<super::super::credits_content::Content>,
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

impl FormEngine {
    pub(in super::super) fn credits_content(&self) -> Arc<super::super::credits_content::Content> {
        Arc::clone(self.credits.get_or_init(|| {
            let content = super::super::credits_content::Content::read(|path| {
                self.server_source.as_ref().and_then(|pack| pack.view.as_ref())
                    .and_then(|view| view.read_capped(path, assets::MAX_UI_FILE_BYTES as u64))
                    .map(Vec::from)
                    .or_else(|| self.assets.ui_file(path).map(<[u8]>::to_vec))
            });
            if content.missing {
                bevy::log::warn!("credits runtime files are missing from the UI carrier or pack; rebuild UI assets from a pack containing credits/end.txt, credits/credits.json and credits/quote.txt");
            }
            Arc::new(content)
        }))
    }
}

impl Painter<'_> {
    pub(super) fn credits(&mut self, dest: [f32; 4], alpha: &dyn Fn([u8; 4]) -> [u8; 4]) {
        let Some(paint) = self.art.credits else {
            return;
        };
        let width = dest[2] - dest[0];
        let mut y = dest[3] - paint.scroll_pixels as f32 * self.px;
        let logo = TITLE_TEXTURE;
        if let Some(metadata) = self.textures.texture(logo) {
            let [w, h] = metadata.pixels.map(|side| side as f32);
            let logo_width = width * 0.9;
            let height = ((width / self.px) * h / w.max(1.0) * 0.9).trunc() * self.px;
            let left = dest[0] + (width - logo_width) * 0.5;
            if let Some(visual) = self.sprite(
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
                let _ = self.push(visual, [left, y, left + logo_width, y + height]);
            }
            y += height;
        }
        let measured = {
            let mut cache = paint
                .layout
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let fresh = cache.as_ref().is_some_and(|old| {
                Arc::ptr_eq(&old.content, &paint.content)
                    && old.player_name == paint.player_name
                    && old.font == self.font.identity()
                    && old.width == width.to_bits()
                    && old.px == self.px.to_bits()
            });
            if !fresh {
                let mut top = 0.0;
                let mut rows = Vec::with_capacity(paint.content.rows.len());
                for row in paint.content.rows.iter() {
                    let text: Arc<str> = row.text.replace("PLAYERNAME", &paint.player_name).into();
                    let height = if row.blank_line {
                        self.metrics.line_height_64 as f32 / 64.0 * self.metrics.scale.get()
                    } else if text.is_empty() {
                        0.0
                    } else {
                        let request = super::text_paint::scaled_request(
                            &self.metrics,
                            &text,
                            super::text_paint::width_64(f64::from(width)),
                            self.font,
                            1.0,
                        );
                        self.layouts
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
                    top += height + row.gap * self.px;
                }
                *cache = Some(Arc::new(MeasuredCredits {
                    content: Arc::clone(&paint.content),
                    player_name: paint.player_name.clone(),
                    font: self.font.identity(),
                    width: width.to_bits(),
                    px: self.px.to_bits(),
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
                let clip = self.clip.map_or(dest, |(clip, _)| clip);
                let _ = self.text(
                    &row.text,
                    [dest[0], top, dest[2], top + row.height],
                    clip,
                    TextPaint {
                        color: alpha([255; 4]),
                        edit: None,
                        shadow: self.metrics.shadow(),
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
        paint.finished.set(y + measured.height < dest[1]);
    }
}
