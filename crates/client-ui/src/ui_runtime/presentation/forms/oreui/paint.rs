//! The OreUI canvas: fills, one-texel edges, speculars and bevels, scaled text,
//! and original sprites from the installed bundle.

use std::collections::HashMap;
use std::sync::Arc;

#[cfg(test)]
mod cache_tests;
mod clip;
mod motion;
mod reveal;
mod sprites;
mod typing;
pub(super) use motion::Effects;
pub(super) use motion::apply_entrance;

use ui::{
    TextLayoutCache, TextLayoutRequest, TextShadow, UiNode, UiNodeId, UiRect, UiScale, UiVisual,
};

use super::super::super::menu_scroll::ScrollArea;
use super::super::super::{
    FONT_DESIGN_PIXEL_TEXELS, IconRef, TextMetrics, UiPresentationError, rect,
};
use super::super::menu_caret::TextSpot;
use super::theme::{
    Appearance, BODY, Bundle, EDGE, LETTER_SPACING, Rgba, Role, TEXT_DIMMEST, TEXT_SHADOW, Type,
};
use crate::menu::{
    MenuAction,
    view::{SettingsFocusLandmark, SettingsFocusTarget},
};

/// `style`'s text scale over the frame metrics: the open font's default line is the
/// 1.6rem body size.
pub(super) fn text_factor(style: Type) -> f32 {
    style.size / BODY.size
}

const MEASUREMENT_WIDTH_64: u32 = 65_536 * 64;

/// A logical-pixel rect `[left, top, right, bottom]`.
pub(super) type Bounds = [f32; 4];

/// The installed sprite catalog addresses immutable texture pages.
pub struct Originals {
    pub(super) page: u16,
    pub(super) images: crate::ui_runtime::oreui_assets::OreUiImages,
    pub(super) sprites: Arc<HashMap<String, crate::ui_runtime::oreui_assets::OreUiSprite>>,
    pub(super) masks: HashMap<String, crate::ui_runtime::oreui_assets::OreUiSprite>,
    pub(super) loading_frames: Arc<Vec<(String, u32)>>,
    pub(super) animations: Arc<HashMap<String, Vec<(String, u32)>>>,
}

pub(super) struct Canvas<'a> {
    pub(super) nodes: &'a mut Vec<UiNode>,
    pub(super) next: &'a mut u32,
    pub(super) layouts: &'a mut TextLayoutCache,
    pub(super) font: &'a assets::RuntimeFontCatalog,
    pub(super) metrics: TextMetrics,
    pub(super) solid_page: u16,
    pub(super) originals: Option<&'a Originals>,
    pub(super) artwork: Option<&'a HashMap<String, IconRef>>,
    pub(super) title_artwork: Option<IconRef>,
    pub(super) destination_icon: Option<IconRef>,
    /// Logical pixels per rem (five GUI pixels).
    pub(super) rem: f32,
    pub(super) hits: Vec<(MenuAction, UiRect)>,
    pub(super) focus_hits: Vec<(MenuAction, UiRect)>,
    pub(super) focus_targets: Vec<SettingsFocusTarget>,
    pub(super) focus_landmarks: Vec<SettingsFocusLandmark>,
    pub(super) focus_parent: Option<u16>,
    /// Opacity multiplier for everything drawn, for fading screens in.
    pub(super) alpha: f32,
    /// The menu clock used to select timed animation frames.
    pub(super) seconds: f64,
    pub(super) transitions: Option<&'a mut super::transitions::Transitions>,
    pub(super) surface: super::motion::Surface,
    entrance_active: bool,
    pub(super) slider_tracks: Vec<(u16, UiRect, Option<UiRect>)>,
    /// Scroll offsets by view key, as the last input left them.
    pub(super) offsets: HashMap<String, f32>,
    pub(super) scrolls: Vec<ScrollArea>,
    /// Where text fields drew their text, for placing a pressed caret.
    pub(super) spots: Vec<TextSpot>,
    pub(super) bundle: Bundle,
    pub(super) appearance: Appearance,
    pub(super) settings_scrollbars: bool,
    pub(super) capture_focus: bool,
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
            artwork: None,
            title_artwork: None,
            destination_icon: None,
            rem: gui_pixel * 5.0,
            hits: Vec::new(),
            focus_hits: Vec::new(),
            focus_targets: Vec::new(),
            focus_landmarks: Vec::new(),
            focus_parent: None,
            alpha: 1.0,
            seconds: 0.0,
            transitions: None,
            surface: super::motion::Surface::Loading,
            entrance_active: false,
            slider_tracks: Vec::new(),
            offsets: HashMap::new(),
            scrolls: Vec::new(),
            spots: Vec::new(),
            bundle: Bundle::Menus,
            appearance: Appearance::Default,
            settings_scrollbars: false,
            capture_focus: false,
            clip: None,
        }
    }

    /// Logical pixels for `value` rem.
    pub(super) fn r(&self, value: f32) -> f32 {
        value * self.rem
    }

    pub(super) fn role(&self, role: Role) -> Role {
        self.appearance.role(role)
    }

    pub(super) fn interaction(
        &mut self,
        view: &crate::menu::MenuView,
        action: Option<MenuAction>,
    ) -> super::widgets::Interaction {
        let mut state = super::widgets::Interaction::of(view, action);
        if let (Some(transitions), Some(action)) = (self.transitions.as_deref_mut(), action) {
            state.pressed |= transitions.pressed(action, self.seconds);
        }
        state
    }

    pub(super) fn switch_interaction(
        &mut self,
        view: &crate::menu::MenuView,
        action: Option<MenuAction>,
    ) -> super::widgets::Interaction {
        let Some(action) = action else {
            return super::widgets::Interaction::default();
        };
        let matching = |candidate| super::transitions::same_switch_action(candidate, action);
        let retained = self
            .transitions
            .as_deref_mut()
            .is_some_and(|transitions| transitions.pressed_switch(action, self.seconds));
        super::widgets::Interaction {
            action: Some(action),
            hovered: matching(view.hovered),
            pressed: matching(view.pressed) || retained,
            focused: view.navigation_focus_visible && matching(view.focused_action),
        }
    }

    /// Drag endpoints retain their full geometry; only the rendered thumb captures a pointer.
    pub(super) fn slider_track(
        &mut self,
        index: u16,
        track: Bounds,
        thumb: Bounds,
    ) -> Result<(), UiPresentationError> {
        let track = rect(track[0], track[1], track[2], track[3])?;
        let thumb = match self.clip {
            Some((_, clip)) => [
                thumb[0].max(clip[0]),
                thumb[1].max(clip[1]),
                thumb[2].min(clip[2]),
                thumb[3].min(clip[3]),
            ],
            None => thumb,
        };
        let thumb = if thumb[2] > thumb[0] && thumb[3] > thumb[1] {
            Some(rect(thumb[0], thumb[1], thumb[2], thumb[3])?)
        } else {
            None
        };
        self.slider_tracks.push((index, track, thumb));
        Ok(())
    }

    fn push(
        &mut self,
        bounds: Bounds,
        mut visual: UiVisual,
    ) -> Result<UiRect, UiPresentationError> {
        let area = self.local(bounds)?;
        match &mut visual {
            UiVisual::Solid { color, .. } => *color = self.appearance.surface(*color),
            UiVisual::Gradient { colors, .. } => {
                colors
                    .iter_mut()
                    .for_each(|color| *color = self.appearance.surface(*color));
            }
            UiVisual::Sprite {
                texture_page,
                color,
                ..
            }
            | UiVisual::RotatedSprite {
                texture_page,
                color,
                ..
            } => {
                *color = if *texture_page == self.solid_page {
                    self.appearance.surface(*color)
                } else {
                    self.appearance.ink(*color)
                };
            }
            _ => {}
        }
        if self.alpha < 1.0 {
            let scale = |color: &mut Rgba| {
                color[3] = (f32::from(color[3]) * self.alpha.max(0.0)).round() as u8;
            };
            match &mut visual {
                UiVisual::Solid { color, .. }
                | UiVisual::Sprite { color, .. }
                | UiVisual::RotatedSprite { color, .. }
                | UiVisual::Text { color, .. } => scale(color),
                UiVisual::Gradient { colors, .. } => colors.iter_mut().for_each(scale),
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

    pub(super) fn gradient(
        &mut self,
        bounds: Bounds,
        colors: [Rgba; 2],
    ) -> Result<(), UiPresentationError> {
        self.push(
            bounds,
            UiVisual::Gradient {
                texture_page: self.solid_page,
                colors,
                horizontal: false,
            },
        )?;
        Ok(())
    }

    /// Finishes a solid's extent after measuring its children, retaining its paint order.
    pub(super) fn resize_fill(
        &mut self,
        index: usize,
        bounds: Bounds,
    ) -> Result<(), UiPresentationError> {
        let area = self.local(bounds)?;
        let node = &mut self.nodes[index];
        debug_assert!(matches!(node.visual(), UiVisual::Solid { .. }));
        *node = UiNode::new(node.id(), node.parent(), area).with_visual(node.visual().clone());
        Ok(())
    }

    pub(super) fn rotated_fill(
        &mut self,
        bounds: Bounds,
        color: Rgba,
        angle_radians: f32,
    ) -> Result<(), UiPresentationError> {
        if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] || color[3] == 0 {
            return Ok(());
        }
        self.push(
            bounds,
            UiVisual::RotatedSprite {
                texture_page: self.solid_page,
                uv: [0, 0, 1, 1],
                color,
                angle_radians,
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

    /// One-texel inner edges: top and left in `top`, bottom and right in `bottom`. The
    /// top-right and bottom-left texels blend `top` over `bottom`, as vanilla's art does.
    pub(super) fn specular(
        &mut self,
        b: Bounds,
        top: Rgba,
        bottom: Rgba,
    ) -> Result<(), UiPresentationError> {
        let w = self.r(EDGE);
        self.fill([b[0], b[3] - w, b[2], b[3]], bottom)?;
        self.fill([b[2] - w, b[1], b[2], b[3] - w], bottom)?;
        self.fill([b[0], b[1], b[2], b[1] + w], top)?;
        self.fill([b[0], b[1] + w, b[0] + w, b[3]], top)
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

    /// Draws wrapped lines centered within the requested width, returning their height.
    pub(super) fn centered_wrapped_text(
        &mut self,
        value: &str,
        at: [f32; 2],
        width: f32,
        style: Type,
        color: Rgba,
    ) -> Result<f32, UiPresentationError> {
        if value.is_empty() {
            return Ok(0.0);
        }
        let mut request = self.text_request(value, (width.max(1.0) * 64.0) as u32, style)?;
        request.wrap.align = ui::TextLineAlign::Center;
        let layout = self
            .layouts
            .layout(request)
            .map_err(UiPresentationError::Text)?;
        self.place_text(layout, at, width, color, false)
    }

    fn layout(
        &mut self,
        value: &str,
        width_64: u32,
        style: Type,
    ) -> Result<std::sync::Arc<ui::TextLayout>, UiPresentationError> {
        #[cfg(feature = "tracy")]
        let _text_span = bevy::log::info_span!("ui.text").entered();
        let request = self.text_request(value, width_64, style)?;
        self.layouts
            .layout(request)
            .map_err(UiPresentationError::Text)
    }

    pub(super) fn text_request<'t>(
        &self,
        value: &'t str,
        width_64: u32,
        style: Type,
    ) -> Result<TextLayoutRequest<'t>, UiPresentationError>
    where
        'a: 't,
    {
        let font = self.font.font_named_at_size(
            style.face.name(),
            self.r(style.size) * self.metrics.dpi_scale.get(),
        );
        let mut request = self.metrics.request(value, width_64, font);
        if let Some(source) = font.line_metrics() {
            request.scale = UiScale::new_display(self.r(style.size) * 64.0 / source.em_64 as f32)
                .map_err(UiPresentationError::Geometry)?;
            let line = source.em_64 as f32 * style.line / style.size;
            let leading = (line - source.ascent_64 as f32 - source.descent_64 as f32) * 0.5;
            request.line_height_64 = line.round() as u32;
            request.baseline_64 =
                (source.ascent_64 as f32 + leading).clamp(0.0, line).round() as u32;
            request.wrap.letter_spacing_64 = (self.r(LETTER_SPACING) * 64.0).round() as i32;
        } else {
            request.scale = UiScale::new_display(self.metrics.scale.get() * text_factor(style))
                .map_err(UiPresentationError::Geometry)?;
        }
        Ok(request)
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
        match self.line_layout(value, width, style)? {
            Some(layout) => self.place_text(layout, at, width + 1.0, color, false),
            None => Ok(0.0),
        }
    }

    /// Keeps a field's visible glyphs centered while preserving horizontal ellipsis.
    pub(super) fn text_line_vertically_centred(
        &mut self,
        value: &str,
        bounds: Bounds,
        style: Type,
        color: Rgba,
    ) -> Result<(), UiPresentationError> {
        self.text_line_typing(value, bounds, style, color, None)
    }

    fn line_layout(
        &mut self,
        value: &str,
        width: f32,
        style: Type,
    ) -> Result<Option<Arc<ui::TextLayout>>, UiPresentationError> {
        #[cfg(feature = "tracy")]
        let _text_span = bevy::log::info_span!("ui.label").entered();
        let request = self.text_request(value, MEASUREMENT_WIDTH_64, style)?;
        let layout = self
            .layouts
            .single_line(request, (width.max(0.0) * 64.0) as u32)
            .map_err(UiPresentationError::Text)?;
        Ok((!layout.glyphs().is_empty()).then_some(layout))
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
        let layout = self.layout(value, MEASUREMENT_WIDTH_64, style)?;
        Ok((layout.size_64()[0] as f32 / 64.0, layout))
    }

    /// The height `value` wraps to within `width` in `style`.
    pub(super) fn measure_height(
        &mut self,
        value: &str,
        width: f32,
        style: Type,
    ) -> Result<f32, UiPresentationError> {
        #[cfg(feature = "tracy")]
        let _text_span = bevy::log::info_span!("ui.text").entered();
        let request = self.text_request(value, (width.max(1.0) * 64.0) as u32, style)?;
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
        let height = layout.size_64()[1] as f32 / 64.0;
        let at = [(b[0] + b[2] - width) * 0.5, (b[1] + b[3] - height) * 0.5];
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

    /// Centres the visible glyphs of a short label rather than its line box.
    pub(super) fn text_centred_visible(
        &mut self,
        value: &str,
        b: Bounds,
        style: Type,
        color: Rgba,
    ) -> Result<(), UiPresentationError> {
        let (width, layout) = self.measured(value, style)?;
        if width > b[2] - b[0] {
            return self.text_centred(value, b, style, color, false);
        }
        let mut ink = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for glyph in layout
            .glyphs()
            .iter()
            .filter(|glyph| !glyph.codepoint.is_whitespace())
        {
            for axis in 0..2 {
                ink[axis] = ink[axis].min(glyph.bounds_64[axis] as f32 / 64.0);
                ink[axis + 2] = ink[axis + 2].max(glyph.bounds_64[axis + 2] as f32 / 64.0);
            }
        }
        if !ink[0].is_finite() {
            return Ok(());
        }
        let at = [
            (b[0] + b[2] - ink[0] - ink[2]) * 0.5,
            (b[1] + b[3] - ink[1] - ink[3]) * 0.5,
        ];
        self.place_text(layout, at, width + 1.0, color, false)?;
        Ok(())
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

    pub(super) fn focus_target(
        &mut self,
        action: MenuAction,
        b: Bounds,
    ) -> Result<(), UiPresentationError> {
        if self.capture_focus && b[2] > b[0] && b[3] > b[1] {
            let bounds = rect(b[0], b[1], b[2], b[3])?;
            self.focus_hits.push((action, bounds));
            self.focus_targets.push(SettingsFocusTarget {
                action,
                bounds,
                landmark: self.focus_parent,
            });
        }
        Ok(())
    }

    pub(super) fn hit(&mut self, action: MenuAction, b: Bounds) -> Result<(), UiPresentationError> {
        self.focus_target(action, b)?;
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
            let width = self.r(if self.settings_scrollbars { 1.2 } else { 0.6 });
            let side = (height * height / content).max(self.r(2.0));
            let top = v[1] + offset / max * (height - side);
            let bar = [v[2] - width, top, v[2], top + side];
            if self.settings_scrollbars {
                let x = v[2] - width * 0.5;
                self.fill(
                    [x - self.r(EDGE), v[1], x + self.r(EDGE), v[3]],
                    [88, 88, 90, 255],
                )?;
                self.fill(bar, super::theme::BORDER)?;
                let e = self.r(EDGE);
                self.fill(
                    [bar[0] + e, bar[1] + e, bar[2] - e, bar[3] - e],
                    super::theme::NEUTRAL20.fill,
                )?;
                self.fill(
                    [bar[0] + e, bar[3] - self.r(0.4) - e, bar[2] - e, bar[3] - e],
                    [88, 88, 90, 255],
                )?;
            } else {
                self.fill(
                    bar,
                    [TEXT_DIMMEST[0], TEXT_DIMMEST[1], TEXT_DIMMEST[2], 160],
                )?;
            }
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
