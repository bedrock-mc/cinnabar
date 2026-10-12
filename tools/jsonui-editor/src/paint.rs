//! Engine draw nodes to the client's retained [`UiNode`]s, then through `ui`'s
//! own draw-list builder (glyph runs, `§` colours, shadows, clip batches) and
//! the CPU rasterizer. Placement follows the client's form painter: a clip
//! group opens whenever the clip changes, destinations snap to whole physical
//! pixels, and each label is one layout from `ui-engine`'s label request.
//! `custom` renderers are not drawn; without a font carrier, text paints as bars.

use std::borrow::Cow;
use std::collections::BTreeMap;

use json_ui::{Draw, DrawNode, RectOut, TextAlign};
use ui::{SafeArea, TextShadow, UiDrawList, UiNode, UiNodeId, UiScale, UiTree, UiVisual};
use ui_engine::engine::text_paint::{TextPaint, label_origin, localized, painted_label_request};
use ui_engine::engine::{pixel_snap, rect};
use ui_engine::text_metrics::TextMetrics;

use crate::raster::{Canvas, Page};
use crate::text::Fonts;
use crate::textures::Textures;

/// Page numbering: the font's pages first (glyph runs name them directly),
/// then one white page for solids, then decoded textures.
pub struct PageMap {
    pub font_pages: usize,
}

impl PageMap {
    pub fn solid(&self) -> u16 {
        self.font_pages as u16
    }

    pub fn texture(&self, index: usize) -> u16 {
        (self.font_pages + 1 + index) as u16
    }
}

pub struct PaintInput<'a> {
    pub nodes: &'a [DrawNode],
    /// Physical pixels per GUI pixel.
    pub px: u32,
    pub size: [u32; 2],
    pub now: f64,
    pub clocks: &'a BTreeMap<String, f64>,
}

/// Rasterize `input` to straight-alpha RGBA8.
pub fn paint(input: &PaintInput, fonts: &Fonts, textures: &Textures) -> Result<Vec<u8>, String> {
    let pages = PageMap {
        font_pages: fonts.font().map_or(0, |font| font.pages().len()),
    };
    let list = draw_list(input, fonts, textures, &pages)?;
    let white = Page::white();
    let font_pages: Vec<Page> = fonts
        .font()
        .map(|font| {
            font.pages()
                .iter()
                .map(|page| Page {
                    width: page.width,
                    height: page.height,
                    rgba: (0..page.width as usize * page.height as usize)
                        .flat_map(|texel| page.pixels.texel(texel).unwrap_or_default())
                        .collect(),
                })
                .collect()
        })
        .unwrap_or_default();
    let texture_pages: Vec<Page> = (0..)
        .map_while(|index| textures.with_page(index, Page::clone))
        .collect();
    let mut canvas = Canvas::new(input.size[0], input.size[1]);
    canvas.draw(&list, |page| {
        let page = usize::from(page);
        if page < pages.font_pages {
            font_pages.get(page)
        } else if page == pages.font_pages {
            Some(&white)
        } else {
            texture_pages.get(page - pages.font_pages - 1)
        }
    });
    Ok(canvas.into_rgba())
}

/// The client draw list for `input`.
pub fn draw_list(
    input: &PaintInput,
    fonts: &Fonts,
    textures: &Textures,
    pages: &PageMap,
) -> Result<UiDrawList, String> {
    let metrics = crate::text::metrics(input.size, input.px);
    let mut painter = Painter {
        px: input.px as f32,
        metrics,
        fonts,
        textures,
        pages,
        nodes: Vec::new(),
        next: 1,
        clip: None,
    };
    // Each frame samples its moment afresh, every control created at zero.
    let mut animator = json_ui::Animator::starting_at(0.0);
    for node in input.nodes {
        painter.paint(node, &mut animator, input.now, input.clocks);
    }
    let error = |e: ui::UiError| e.to_string();
    let mut tree = UiTree::new(painter.nodes).map_err(error)?;
    let viewport =
        rect(0.0, 0.0, input.size[0] as f32, input.size[1] as f32).map_err(|_| "empty viewport")?;
    tree.layout(viewport, UiScale::default(), SafeArea::ZERO)
        .map_err(error)?;
    tree.build_draw_list().map_err(error)
}

struct Painter<'a, 't> {
    /// Logical (here physical) px per GUI px.
    px: f32,
    metrics: TextMetrics,
    fonts: &'a Fonts,
    textures: &'a Textures<'t>,
    pages: &'a PageMap,
    nodes: Vec<UiNode>,
    next: u32,
    clip: Option<([f32; 4], UiNodeId)>,
}

impl Painter<'_, '_> {
    fn logical(&self, rect: &RectOut) -> [f32; 4] {
        let px = self.px;
        [
            rect.x as f32 * px,
            rect.y as f32 * px,
            (rect.x + rect.w) as f32 * px,
            (rect.y + rect.h) as f32 * px,
        ]
    }

    fn id(&mut self) -> UiNodeId {
        let id = UiNodeId::new(self.next);
        self.next += 1;
        id
    }

    fn group(&mut self, clip: [f32; 4]) -> Option<UiNodeId> {
        if let Some((current, id)) = self.clip
            && current == clip
        {
            return Some(id);
        }
        let bounds = rect(clip[0], clip[1], clip[2], clip[3]).ok()?;
        let id = self.id();
        self.nodes
            .push(UiNode::new(id, None, bounds).with_clip_children(true));
        self.clip = Some((clip, id));
        Some(id)
    }

    fn push(&mut self, visual: UiVisual, bounds: [f32; 4]) {
        let Some((clip, parent)) = self.clip else {
            return;
        };
        let Ok(local) = rect(
            bounds[0] - clip[0],
            bounds[1] - clip[1],
            bounds[2] - clip[0],
            bounds[3] - clip[1],
        ) else {
            return;
        };
        let id = self.id();
        self.nodes
            .push(UiNode::new(id, Some(parent), local).with_visual(visual));
    }

    fn paint(
        &mut self,
        node: &DrawNode,
        animator: &mut json_ui::Animator,
        now: f64,
        clocks: &BTreeMap<String, f64>,
    ) {
        let drawn = node.animate(animator, now, Some(clocks), None);
        let clip = self.logical(&drawn.clip);
        let dest = [drawn.dest.x, drawn.dest.y, drawn.dest.w, drawn.dest.h];
        // As the client places them: text moves to a whole physical pixel, images snap to one.
        let dest = if matches!(node.draw, Draw::Text { .. }) {
            pixel_snap::positioned(dest, self.metrics.gui_scale, self.px)
        } else {
            pixel_snap::snapped(dest, self.metrics.gui_scale, self.px)
        };
        let opacity = drawn.opacity;
        if drawn.hidden
            || clip[2] <= clip[0]
            || clip[3] <= clip[1]
            || dest[2] <= dest[0]
            || dest[3] <= dest[1]
            || opacity <= 0.0
        {
            return;
        }
        let alpha = |color: [u8; 4]| {
            let a = (f32::from(color[3]) * opacity.clamp(0.0, 1.0)).round() as u8;
            [color[0], color[1], color[2], a]
        };
        match &node.draw {
            Draw::Text {
                text,
                color,
                shadow,
                align,
                scale,
                localize,
                options,
            } => {
                let style = TextPaint {
                    color: alpha(*color),
                    edit: None,
                    shadow: if *shadow {
                        self.metrics.shadow()
                    } else {
                        TextShadow::None
                    },
                    align: *align,
                    scale: *scale,
                    localize: *localize,
                    options: options.clone(),
                };
                self.text(text, dest, clip, &style);
            }
            Draw::Solid { color } => {
                if self.group(clip).is_some() {
                    let visual = UiVisual::Solid {
                        texture_page: self.pages.solid(),
                        color: alpha(*color),
                    };
                    self.push(visual, dest);
                }
            }
            Draw::Sprite {
                texture, uv, color, ..
            } => {
                let Some((page, [w, h])) = self.textures.sprite(texture) else {
                    return;
                };
                let uv = drawn.uv.unwrap_or(*uv);
                let color = &drawn.color.unwrap_or(*color);
                let pixel =
                    |span: f64, t: f32| (span * f64::from(t)).round().clamp(0.0, 65_535.0) as u16;
                let visual = UiVisual::Sprite {
                    texture_page: self.pages.texture(page),
                    uv: [
                        pixel(w, uv.u0),
                        pixel(h, uv.v0),
                        pixel(w, uv.u1),
                        pixel(h, uv.v1),
                    ],
                    color: alpha(*color),
                };
                if self.group(clip).is_some() {
                    self.push(visual, dest);
                }
            }
            // Native renderers (item icons, player previews, HUD bars) are the
            // client's own code; the editor outlines their boxes instead.
            Draw::Custom { .. } => {}
        }
    }

    /// One layout per label, as the client paints it: lines past the label's
    /// height drop and the last kept one ends in `...`.
    fn text(&mut self, text: &str, dest: [f32; 4], clip: [f32; 4], style: &TextPaint) {
        let fonts = self.fonts;
        let text = if style.localize {
            localized(text, &|key: &str| fonts.translate(key))
        } else {
            Cow::Borrowed(text)
        };
        let Some(font) = fonts.font() else {
            return self.text_bars(&text, dest, clip, style);
        };
        if text.is_empty() {
            return;
        }
        let request = painted_label_request(
            self.metrics,
            &text,
            dest,
            font,
            style.scale,
            &style.options,
            style.align,
            self.px,
        );
        let Some(layout) = fonts.layout(request) else {
            return;
        };
        let [width, height] = layout.size_64().map(|size| size as f32 / 64.0);
        let [left, top] = label_origin(dest, font, &style.options, self.px);
        if self.group(clip).is_none() {
            return;
        }
        let visual = UiVisual::Text {
            layout,
            color: style.color,
            shadow: style.shadow,
        };
        self.push(visual, [left, top, left + width.max(1.0), top + height]);
    }

    /// Without a font carrier, each line paints as a bar of its measured width.
    fn text_bars(&mut self, text: &str, dest: [f32; 4], clip: [f32; 4], style: &TextPaint) {
        if self.group(clip).is_none() {
            return;
        }
        let mut top = dest[1];
        for line in text.split('\n').filter(|line| !line.is_empty()) {
            let [w, h] = crate::text::fallback_extent(
                line,
                f64::from(dest[2] - dest[0]) / f64::from(self.px),
            );
            let (w, h) = (w as f32 * self.px, h as f32 * self.px);
            let x = match style.align {
                TextAlign::Left => dest[0],
                TextAlign::Center => dest[0] + (dest[2] - dest[0] - w).max(0.0) * 0.5,
                TextAlign::Right => dest[2] - w,
            };
            let [r, g, b, a] = style.color;
            let visual = UiVisual::Solid {
                texture_page: self.pages.solid(),
                color: [r, g, b, a / 2],
            };
            self.push(visual, [x, top + h * 0.2, x + w, top + h * 0.8]);
            top += h;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textures::TextureCache;
    use crate::workspace::Workspace;
    use json_ui::UvRect;

    fn node(name: &str, dest: [f64; 4], clip: [f64; 4], draw: Draw) -> DrawNode {
        let rect = |r: [f64; 4]| RectOut {
            x: r[0],
            y: r[1],
            w: r[2],
            h: r[3],
        };
        DrawNode {
            name: name.into(),
            key: format!("/{name}"),
            dest: rect(dest),
            clip: rect(clip),
            layer: 0,
            alpha: 0.5,
            anim: None,
            draw,
            gates: Vec::new(),
        }
    }

    // Solids scale by the GUI scale, take node alpha, and batch per clip; a
    // missing sprite and a custom renderer draw nothing.
    #[test]
    fn draw_nodes_become_scaled_clipped_quads() {
        let screen = [0.0, 0.0, 100.0, 100.0];
        let nodes = [
            node(
                "a",
                [1.0, 2.0, 3.0, 4.0],
                screen,
                Draw::Solid {
                    color: [255, 0, 0, 255],
                },
            ),
            node(
                "b",
                [5.0, 5.0, 10.0, 10.0],
                [0.0, 0.0, 8.0, 8.0],
                Draw::Solid {
                    color: [0, 255, 0, 255],
                },
            ),
            node(
                "c",
                [0.0, 0.0, 4.0, 4.0],
                screen,
                Draw::Sprite {
                    texture: "textures/ui/none".into(),
                    uv: UvRect::full(),
                    color: [255; 4],
                    filter: Default::default(),
                },
            ),
            node(
                "d",
                [0.0, 0.0, 4.0, 4.0],
                screen,
                Draw::Custom {
                    renderer: "x".into(),
                    data: Default::default(),
                },
            ),
        ];
        let mut workspace = Workspace::default();
        let mut cache = TextureCache::default();
        let textures = Textures::new(&mut workspace, &mut cache);
        let fonts = Fonts::default();
        let clocks = BTreeMap::new();
        let input = PaintInput {
            nodes: &nodes,
            px: 2,
            size: [800, 600],
            now: 0.0,
            clocks: &clocks,
        };
        let list = draw_list(&input, &fonts, &textures, &PageMap { font_pages: 0 }).unwrap();
        assert_eq!(list.vertices.len(), 8);
        assert_eq!(list.vertices[0].position, [2.0, 4.0]);
        assert_eq!(list.vertices[2].position, [8.0, 12.0]);
        assert_eq!(list.vertices[0].color, [255, 0, 0, 128]);
        assert_eq!(list.batches.len(), 2);
        assert_eq!(list.batches[1].clip.max().x(), 16.0);
        assert!(textures.missing.borrow().contains("textures/ui/none"));
    }
}
