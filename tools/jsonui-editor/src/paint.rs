//! Engine draw nodes to the client's retained [`UiNode`]s, then through `ui`'s
//! own draw-list builder (glyph runs, `§` colours, shadows, clip batches) and
//! the CPU rasterizer. The node conversion mirrors the client's form painter:
//! a clip group opens whenever the clip changes, labels draw one node per
//! source line, and lines past the label's height drop. `custom` renderers
//! are not drawn.

use std::borrow::Cow;
use std::collections::BTreeMap;

use json_ui::{Draw, DrawNode, RectOut, TextAlign};
use ui::{
    SafeArea, TextShadow, UiDrawList, UiNode, UiNodeId, UiPoint, UiRect, UiScale, UiTree, UiVisual,
};

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
    pub px: f32,
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
    let mut painter = Painter {
        px: input.px,
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
        rect(0.0, 0.0, input.size[0] as f32, input.size[1] as f32).ok_or("empty viewport")?;
    tree.layout(viewport, UiScale::default(), SafeArea::ZERO)
        .map_err(error)?;
    tree.build_draw_list().map_err(error)
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Option<UiRect> {
    UiRect::new(UiPoint::new(x0, y0).ok()?, UiPoint::new(x1, y1).ok()?).ok()
}

struct Painter<'a, 't> {
    px: f32,
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
        let bounds = rect(clip[0], clip[1], clip[2], clip[3])?;
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
        let Some(local) = rect(
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
        let dest = self.logical(&drawn.dest);
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
                ..
            } => {
                let style = TextPaint {
                    color: alpha(*color),
                    shadow: *shadow,
                    align: *align,
                    scale: *scale,
                    localize: *localize,
                };
                self.text(text, dest, clip, style);
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

    /// One node per source line so each aligns on its own; only whole lines
    /// that fit the label's height draw (the first always does).
    fn text(&mut self, text: &str, dest: [f32; 4], clip: [f32; 4], style: TextPaint) {
        let text = if style.localize {
            self.fonts.localized(text)
        } else {
            Cow::Borrowed(text)
        };
        if self.fonts.font().is_none() {
            return self.text_bars(&text, dest, clip, style);
        }
        let mut top = dest[1];
        let mut carry = String::new();
        for (index, line) in text.split('\n').enumerate() {
            let source = format!("{carry}{line}");
            carry = active_codes(&source);
            if line.is_empty() {
                continue;
            }
            let width = f64::from(dest[2] - dest[0]);
            let Some(layout) = self.fonts.layout(&source, width, self.px, style.scale) else {
                continue;
            };
            let [width, height] = layout.size_64().map(|size| size as f32 / 64.0);
            let pitch = height / f32::from(layout.line_count().max(1));
            let room = ((dest[3] - top) / pitch + 0.01).floor().max(0.0);
            if index > 0 && room < 1.0 {
                break;
            }
            let shown = room.clamp(1.0, f32::from(layout.line_count().max(1)));
            let bottom = (top + shown * pitch).min(clip[3]);
            let line_clip = [clip[0], clip[1], clip[2], bottom];
            if line_clip[3] <= line_clip[1] {
                break;
            }
            let slack = (dest[2] - dest[0] - width).max(0.0);
            let shift = match style.align {
                TextAlign::Left => 0.0,
                TextAlign::Center => slack * 0.5,
                TextAlign::Right => slack,
            };
            if self.group(line_clip).is_none() {
                break;
            }
            let x = dest[0] + shift;
            let shadow = if style.shadow {
                self.fonts.shadow()
            } else {
                TextShadow::None
            };
            let visual = UiVisual::Text {
                layout,
                color: style.color,
                shadow,
            };
            self.push(visual, [x, top, x + width.max(1.0), top + height]);
            top += height;
        }
    }

    /// Without a font carrier, each line paints as a bar of its measured width.
    fn text_bars(&mut self, text: &str, dest: [f32; 4], clip: [f32; 4], style: TextPaint) {
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

#[derive(Clone, Copy)]
struct TextPaint {
    color: [u8; 4],
    shadow: bool,
    align: TextAlign,
    scale: f32,
    localize: bool,
}

/// The format codes in force at the end of `text`, to open the next line with.
fn active_codes(text: &str) -> String {
    let mut codes = String::new();
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '§' {
            continue;
        }
        match characters.next() {
            Some('r') => codes.clear(),
            Some(code @ ('0'..='9' | 'a'..='w')) => {
                codes.push('§');
                codes.push(code);
            }
            _ => {}
        }
    }
    codes
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
            px: 2.0,
            size: [200, 200],
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
