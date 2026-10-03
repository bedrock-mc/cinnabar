//! The OreUI canvas: fills, one-texel edges, speculars and bevels, scaled text,
//! and (in the local-originals mode) sprites from the install's atlases.

use std::collections::HashMap;

use ui::{TextLayoutCache, TextShadow, UiNode, UiNodeId, UiRect, UiScale, UiVisual};

use super::super::super::menu_scroll::ScrollArea;
use super::super::super::{
    FONT_DESIGN_PIXEL_TEXELS, IconRef, TextMetrics, UiPresentationError, rect,
};
use super::super::menu_caret::TextSpot;
use super::theme::{EDGE, Rgba, TEXT_DIMMEST, TEXT_SHADOW, Type};
use crate::menu::MenuAction;

/// `style`'s text scale over the frame metrics: the open font's default line is the
/// 1.6rem body size.
pub(super) fn text_factor(style: Type) -> f32 {
    style.size / 1.6
}

/// A logical-pixel rect `[left, top, right, bottom]`.
pub(super) type Bounds = [f32; 4];

/// The install's atlas sprites on one texture page (local-originals mode).
pub(crate) struct Originals {
    pub(super) page: u16,
    pub(super) sprites: HashMap<String, [u16; 4]>,
}

pub(super) struct Canvas<'a> {
    pub(super) nodes: &'a mut Vec<UiNode>,
    pub(super) next: &'a mut u32,
    pub(super) layouts: &'a mut TextLayoutCache,
    pub(super) font: &'a assets::RuntimeFontCatalog,
    pub(super) metrics: TextMetrics,
    pub(super) solid_page: u16,
    pub(super) originals: Option<&'a Originals>,
    /// Logical pixels per rem (five GUI pixels).
    pub(super) rem: f32,
    pub(super) hits: Vec<(MenuAction, UiRect)>,
    /// Opacity multiplier for everything drawn, for fading screens in.
    pub(super) alpha: f32,
    /// Scroll offsets by view key, as the last input left them.
    pub(super) offsets: HashMap<String, f32>,
    pub(super) scrolls: Vec<ScrollArea>,
    /// Where text fields drew their text, for placing a pressed caret.
    pub(super) spots: Vec<TextSpot>,
    /// The clipping node drawing attaches to, and its bounds.
    clip: Option<(UiNodeId, Bounds)>,
}

/// A scroll view being drawn: restore `outer` when it ends.
pub(super) struct Scroll {
    id: UiNodeId,
    first_node: usize,
    key: String,
    viewport: Bounds,
    outer: Option<(UiNodeId, Bounds)>,
    pub(super) offset: f32,
}

impl<'a> Canvas<'a> {
    pub(super) fn new(
        nodes: &'a mut Vec<UiNode>,
        next: &'a mut u32,
        layouts: &'a mut TextLayoutCache,
        font: &'a assets::RuntimeFontCatalog,
        metrics: TextMetrics,
        solid_page: u16,
        originals: Option<&'a Originals>,
    ) -> Self {
        let gui_pixel = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        Self {
            nodes,
            next,
            layouts,
            font,
            metrics,
            solid_page,
            originals,
            rem: gui_pixel * 5.0,
            hits: Vec::new(),
            alpha: 1.0,
            offsets: HashMap::new(),
            scrolls: Vec::new(),
            spots: Vec::new(),
            clip: None,
        }
    }

    /// Logical pixels for `value` rem.
    pub(super) fn r(&self, value: f32) -> f32 {
        value * self.rem
    }

    fn push(
        &mut self,
        bounds: Bounds,
        mut visual: UiVisual,
    ) -> Result<UiRect, UiPresentationError> {
        let area = self.local(bounds)?;
        if self.alpha < 1.0 {
            let scale = |color: &mut Rgba| {
                color[3] = (f32::from(color[3]) * self.alpha.max(0.0)).round() as u8;
            };
            match &mut visual {
                UiVisual::Solid { color, .. }
                | UiVisual::Sprite { color, .. }
                | UiVisual::Text { color, .. } => scale(color),
                _ => {}
            }
        }
        let parent = self.clip.map(|(id, _)| id);
        self.nodes
            .push(UiNode::new(UiNodeId::new(*self.next), parent, area).with_visual(visual));
        *self.next = self.next.saturating_add(1);
        Ok(area)
    }

    /// `b` relative to the clipping node it attaches to, as the UI tree lays out.
    fn local(&self, b: Bounds) -> Result<UiRect, UiPresentationError> {
        let [x, y] = self.clip.map_or([0.0; 2], |(_, c)| [c[0], c[1]]);
        rect(b[0] - x, b[1] - y, b[2] - x, b[3] - y)
    }

    pub(super) fn fill(&mut self, bounds: Bounds, color: Rgba) -> Result<(), UiPresentationError> {
        if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] || color[3] == 0 {
            return Ok(());
        }
        let texture_page = self.solid_page;
        self.push(
            bounds,
            UiVisual::Solid {
                texture_page,
                color,
            },
        )?;
        Ok(())
    }

    /// A `width`-rem border drawn inside `bounds`.
    pub(super) fn frame(
        &mut self,
        b: Bounds,
        width: f32,
        color: Rgba,
    ) -> Result<(), UiPresentationError> {
        let w = self.r(width);
        self.fill([b[0], b[1], b[2], b[1] + w], color)?;
        self.fill([b[0], b[3] - w, b[2], b[3]], color)?;
        self.fill([b[0], b[1] + w, b[0] + w, b[3] - w], color)?;
        self.fill([b[2] - w, b[1] + w, b[2], b[3] - w], color)
    }

    /// One-texel inner edges: top and left in `top`, bottom and right in `bottom`.
    pub(super) fn specular(
        &mut self,
        b: Bounds,
        top: Rgba,
        bottom: Rgba,
    ) -> Result<(), UiPresentationError> {
        let w = self.r(EDGE);
        self.fill([b[0], b[1], b[2], b[1] + w], top)?;
        self.fill([b[0], b[1] + w, b[0] + w, b[3] - w], top)?;
        self.fill([b[0], b[3] - w, b[2], b[3]], bottom)?;
        self.fill([b[2] - w, b[1] + w, b[2], b[3] - w], bottom)
    }

    /// One-texel top and bottom edges only.
    pub(super) fn bevel(
        &mut self,
        b: Bounds,
        top: Rgba,
        bottom: Rgba,
    ) -> Result<(), UiPresentationError> {
        let w = self.r(EDGE);
        self.fill([b[0], b[1], b[2], b[1] + w], top)?;
        self.fill([b[0], b[3] - w, b[2], b[3]], bottom)
    }

    /// Draws `value` from `at` within `width`; returns the laid-out height.
    pub(super) fn text(
        &mut self,
        value: &str,
        at: [f32; 2],
        width: f32,
        style: Type,
        color: Rgba,
        shadow: bool,
    ) -> Result<f32, UiPresentationError> {
        if value.is_empty() {
            return Ok(0.0);
        }
        let layout = self.layout(value, (width.max(1.0) * 64.0) as u32, style)?;
        self.place_text(layout, at, width, color, shadow)
    }

    fn layout(
        &mut self,
        value: &str,
        width_64: u32,
        style: Type,
    ) -> Result<std::sync::Arc<ui::TextLayout>, UiPresentationError> {
        let mut request = self.metrics.request(value, width_64, self.font);
        if let Ok(scale) = UiScale::new_display(self.metrics.scale.get() * text_factor(style)) {
            request.scale = scale;
        }
        self.layouts
            .layout(request)
            .map_err(UiPresentationError::Text)
    }

    /// Draws a laid-out `layout` from `at` within `width`; returns its height.
    fn place_text(
        &mut self,
        layout: std::sync::Arc<ui::TextLayout>,
        at: [f32; 2],
        width: f32,
        color: Rgba,
        shadow: bool,
    ) -> Result<f32, UiPresentationError> {
        let height = layout.size_64()[1] as f32 / 64.0;
        if shadow {
            let offset = self.r(EDGE);
            self.push(
                [
                    at[0] + offset,
                    at[1] + offset,
                    at[0] + offset + width.max(1.0),
                    at[1] + offset + height.max(1.0),
                ],
                UiVisual::Text {
                    layout: layout.clone(),
                    color: TEXT_SHADOW,
                    shadow: TextShadow::None,
                },
            )?;
        }
        self.push(
            [
                at[0],
                at[1],
                at[0] + width.max(1.0),
                at[1] + height.max(1.0),
            ],
            UiVisual::Text {
                layout,
                color,
                shadow: TextShadow::None,
            },
        )?;
        Ok(height)
    }

    /// `value` on one line from `at`, cut with an ellipsis to fit `width`.
    pub(super) fn text_line(
        &mut self,
        value: &str,
        at: [f32; 2],
        width: f32,
        style: Type,
        color: Rgba,
    ) -> Result<f32, UiPresentationError> {
        let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
        let (fits, layout) = self.measured(&value, style)?;
        if fits <= width {
            return self.place_text(layout, at, width + 1.0, color, false);
        }
        let ends: Vec<usize> = value.char_indices().map(|(at, _)| at).collect();
        let (mut low, mut high) = (0, ends.len());
        let mut best = None;
        while low < high {
            let mid = low + (high - low) / 2;
            let shown = format!("{}…", value[..ends[mid]].trim_end());
            let (fits, layout) = self.measured(&shown, style)?;
            if fits <= width {
                best = Some(layout);
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        match best {
            Some(layout) => self.place_text(layout, at, width + 1.0, color, false),
            None => Ok(0.0),
        }
    }

    /// The width `value` lays out to in `style`.
    pub(super) fn measure(&mut self, value: &str, style: Type) -> Result<f32, UiPresentationError> {
        Ok(self.measured(value, style)?.0)
    }

    /// `value`'s unwrapped width and layout in `style`.
    fn measured(
        &mut self,
        value: &str,
        style: Type,
    ) -> Result<(f32, std::sync::Arc<ui::TextLayout>), UiPresentationError> {
        let layout = self.layout(value, 65_536 * 64, style)?;
        Ok((layout.size_64()[0] as f32 / 64.0, layout))
    }

    /// The height `value` wraps to within `width` in `style`.
    pub(super) fn measure_height(
        &mut self,
        value: &str,
        width: f32,
        style: Type,
    ) -> Result<f32, UiPresentationError> {
        let mut request = self
            .metrics
            .request(value, (width.max(1.0) * 64.0) as u32, self.font);
        if let Ok(scale) = UiScale::new_display(self.metrics.scale.get() * text_factor(style)) {
            request.scale = scale;
        }
        let layout = self
            .layouts
            .layout(request)
            .map_err(UiPresentationError::Text)?;
        Ok(layout.size_64()[1] as f32 / 64.0)
    }

    /// `value` centred in `bounds` on one line.
    pub(super) fn text_centred(
        &mut self,
        value: &str,
        b: Bounds,
        style: Type,
        color: Rgba,
        shadow: bool,
    ) -> Result<(), UiPresentationError> {
        let (measured, layout) = self.measured(value, style)?;
        let width = measured.min(b[2] - b[0]);
        let height = self.r(style.line);
        let at = [
            (b[0] + b[2] - width) * 0.5,
            (b[1] + b[3] - height) * 0.5 + self.r((style.line - style.size) * 0.5),
        ];
        if value.is_empty() {
            return Ok(());
        }
        if measured <= width {
            self.place_text(layout, at, width + 1.0, color, shadow)?;
        } else {
            self.text(value, at, width + 1.0, style, color, shadow)?;
        }
        Ok(())
    }

    /// Draws an install sprite (local-originals mode); `false` when unavailable.
    pub(super) fn sprite(
        &mut self,
        key: &str,
        b: Bounds,
        color: Rgba,
    ) -> Result<bool, UiPresentationError> {
        let Some(originals) = self.originals else {
            return Ok(false);
        };
        let Some(&uv) = originals.sprites.get(key) else {
            return Ok(false);
        };
        let texture_page = originals.page;
        self.push(
            b,
            UiVisual::Sprite {
                texture_page,
                uv,
                color,
            },
        )?;
        Ok(true)
    }

    /// A caller-supplied image (artwork, gamerpic) stretched over `b`.
    pub(super) fn icon_ref(&mut self, icon: IconRef, b: Bounds) -> Result<(), UiPresentationError> {
        self.push(
            b,
            UiVisual::Sprite {
                texture_page: icon.page,
                uv: icon.uv,
                color: [255; 4],
            },
        )?;
        Ok(())
    }

    pub(super) fn hit(&mut self, action: MenuAction, b: Bounds) -> Result<(), UiPresentationError> {
        let b = match self.clip {
            Some((_, c)) => [
                b[0].max(c[0]),
                b[1].max(c[1]),
                b[2].min(c[2]),
                b[3].min(c[3]),
            ],
            None => b,
        };
        if b[2] <= b[0] || b[3] <= b[1] {
            return Ok(());
        }
        let area = rect(b[0], b[1], b[2], b[3])?;
        self.hits.push((action, area));
        Ok(())
    }

    /// Starts clipping to `viewport` for the scroll view `key`; draw its
    /// content shifted up by the returned offset, then call [`Self::end_scroll`].
    pub(super) fn begin_scroll(
        &mut self,
        key: &str,
        viewport: Bounds,
    ) -> Result<Scroll, UiPresentationError> {
        let area = self.local(viewport)?;
        let id = UiNodeId::new(*self.next);
        let parent = self.clip.map(|(id, _)| id);
        self.nodes
            .push(UiNode::new(id, parent, area).with_clip_children(true));
        *self.next = self.next.saturating_add(1);
        let outer = self.clip.replace((id, viewport));
        Ok(Scroll {
            id,
            first_node: self.nodes.len(),
            key: key.to_owned(),
            viewport,
            outer,
            offset: self.offsets.get(key).copied().unwrap_or(0.0),
        })
    }

    /// Ends a viewport using the lowest directly attached content node as its extent.
    pub(super) fn end_scroll_to_fit(&mut self, scroll: Scroll) -> Result<(), UiPresentationError> {
        let content = self.nodes[scroll.first_node..]
            .iter()
            .filter(|node| node.parent() == Some(scroll.id))
            .map(|node| node.bounds().max().y() + scroll.offset)
            .fold(0.0, f32::max);
        self.end_scroll(scroll, content)
    }

    /// Ends `scroll` with `content` logical px drawn: a thumb shows when it
    /// overflows, and the view takes wheel and drag input next frame.
    pub(super) fn end_scroll(
        &mut self,
        scroll: Scroll,
        content: f32,
    ) -> Result<(), UiPresentationError> {
        self.clip = scroll.outer;
        let v = scroll.viewport;
        let height = v[3] - v[1];
        let max = (content - height).max(0.0);
        let offset = scroll.offset.min(max);
        let (mut track, mut thumb) = (None, None);
        if max > 0.0 {
            let width = self.r(0.6);
            let side = (height * height / content).max(self.r(2.0));
            let top = v[1] + offset / max * (height - side);
            let bar = [v[2] - width, top, v[2], top + side];
            self.fill(
                bar,
                [TEXT_DIMMEST[0], TEXT_DIMMEST[1], TEXT_DIMMEST[2], 160],
            )?;
            track = Some(rect(v[2] - width, v[1], v[2], v[3])?);
            thumb = Some(rect(bar[0], bar[1], bar[2], bar[3])?);
        }
        self.scrolls.push(ScrollArea {
            key: scroll.key,
            viewport: rect(v[0], v[1], v[2], v[3])?,
            scale: 1.0,
            offset,
            max,
            speed: self.r(6.0),
            track,
            thumb,
            engine: None,
            draggable: true,
        });
        Ok(())
    }
}
