//! Label painting after vanilla's label text: one layout per label with
//! per-line alignment, line padding, hyphen chops and `...` at the lines its
//! height holds. Native hover geometry lives in the sibling tooltip module.

use std::{borrow::Cow, cell::RefCell, sync::Arc};

use assets::RuntimeFontCatalog;
use json_ui::{LabelShape, TextAlign, TextMeasure, TextOptions};
use ui::{
    FONT_DESIGN_PIXEL_TEXELS, TextLayoutCache, TextLayoutRequest, TextLineAlign, TextShadow,
    TextWrap, UiNode, UiScale, UiVisual, WordChop,
};

use super::{Canvas, rect, screen::PaintError, tooltip::TEXT_PITCH};
use crate::text_metrics::TextMetrics;

/// Largest wrap width handed to the text layout (logical px), for "no wrap".
pub const UNWRAPPED_LOGICAL: f64 = 65_536.0;

/// Normal outline labels occupy twelve GUI pixels of ascent and descent.
const OUTLINE_LABEL_HEIGHT_GUI: f32 = 12.0;

/// Selects one installed face for measurement, painting, positioning and edit metadata.
pub(super) fn label_font<'a>(
    font: &'a RuntimeFontCatalog,
    options: &TextOptions,
) -> &'a RuntimeFontCatalog {
    let name = font.resolve_font_name(
        options.font_type.as_deref().unwrap_or("default"),
        options.backup_font_type.as_deref(),
    );
    font.font_named(name)
}

/// Resolved face geometry shared by label measurement, painting and edit metadata.
#[derive(Clone, Copy)]
pub(super) struct LabelMetrics<'a> {
    metrics: TextMetrics,
    font: &'a RuntimeFontCatalog,
    frame_scale: UiScale,
    authored_factor: f32,
    shadow_factor: f32,
}

/// Keeps bitmap spacing and resolves native sizes and effects in the selected face's units.
pub(super) fn label_metrics<'a>(
    mut metrics: TextMetrics,
    default: &RuntimeFontCatalog,
    selected: &'a RuntimeFontCatalog,
    factor: f32,
) -> LabelMetrics<'a> {
    let frame_scale = metrics.scale;
    if std::ptr::eq(default, selected) {
        metrics.line_height_64 = TEXT_PITCH * FONT_DESIGN_PIXEL_TEXELS * 64;
    } else if let Some(source) = selected.line_metrics() {
        let height = source.ascent_64 + source.descent_64;
        let size = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32 * OUTLINE_LABEL_HEIGHT_GUI;
        if let Ok(scale) = UiScale::new_display(size * 64.0 / height as f32) {
            metrics.scale = scale;
            metrics.line_height_64 = height;
            metrics.baseline_64 = source.ascent_64;
        }
    }
    let shadow_factor = frame_scale.get() / metrics.scale.get();
    if factor != 1.0
        && let Ok(scale) = UiScale::new_display(metrics.scale.get() * factor)
    {
        metrics.scale = scale;
    }
    metrics.scale = metrics.scale.quantized();
    LabelMetrics {
        metrics,
        font: selected,
        frame_scale,
        authored_factor: factor,
        shadow_factor,
    }
}

/// UI labels start one GUI pixel below the authored top, independently of
/// font size. Painting and glyph interaction share this origin.
pub fn label_origin(
    dest: [f32; 4],
    _font: &RuntimeFontCatalog,
    _options: &TextOptions,
    px: f32,
) -> [f32; 2] {
    [dest[0], dest[1] + px]
}

/// Filtered outline labels retain fractional placement; pixel faces use the physical grid.
pub(super) fn label_positioned(
    dest: [f64; 4],
    font: &RuntimeFontCatalog,
    options: &TextOptions,
    gui_scale: f32,
    px: f32,
) -> [f32; 4] {
    let selected = label_font(font, options);
    if selected.linear_sampling() {
        let [x, y, w, h] = dest.map(|value| value as f32 * px);
        [x, y, x + w, y + h]
    } else {
        super::pixel_snap::positioned(dest, gui_scale, px)
    }
}

/// How one label paints; the options are borrowed from the draw node for the paint.
#[derive(Clone)]
pub struct TextPaint<'a> {
    pub color: [u8; 4],
    pub edit: Option<super::host_edit::Feedback>,
    pub shadow: TextShadow,
    pub align: TextAlign,
    pub scale: f32,
    pub localize: bool,
    pub options: &'a TextOptions,
}

/// A label's text after vanilla localization. Every newline starts a line, so `\n\n` leaves a
/// blank line as vanilla's text component does, as in a server form body spaced with them.
pub fn localized<'a>(text: &'a str, translate: &dyn Fn(&str) -> Option<Arc<str>>) -> Cow<'a, str> {
    json_ui::localize_text(text, translate)
}

/// `request` at `factor` times the metrics' scale; a factor the display range
/// cannot hold keeps the base scale.
pub fn scaled_request<'a>(
    metrics: &TextMetrics,
    text: &'a str,
    width_64: u32,
    font: &'a RuntimeFontCatalog,
    factor: f32,
) -> TextLayoutRequest<'a> {
    let mut request = metrics.request(text, width_64, font);
    if factor != 1.0
        && let Ok(scale) = UiScale::new_display(metrics.scale.get() * factor)
    {
        request.scale = scale;
    }
    request
}

impl<'a> LabelMetrics<'a> {
    /// Builds wrapping from the resolved face, with padding in logical pixels.
    fn request(
        self,
        text: &'a str,
        width: f64,
        shape: LabelShape,
        px: f32,
    ) -> TextLayoutRequest<'a> {
        let mut request = self.metrics.request(text, width_64(width), self.font);
        request.wrap = TextWrap {
            effect_scale_1024: self.effect_scale_1024(),
            line_padding_64: (shape.line_padding * f64::from(px) * 64.0).round() as i32,
            chop: if shape.hide_hyphen {
                WordChop::Bare
            } else {
                WordChop::Hyphen
            },
            ..request.wrap
        };
        request
    }

    /// Exports the effective factor for callers measuring against the frame's base scale.
    pub(super) fn factor(self) -> f32 {
        self.metrics.scale.get() / self.frame_scale.get()
    }

    /// Keeps bitmap-unit styling in display units after normalizing a native glyph raster.
    pub(super) fn effect_scale_1024(self) -> u32 {
        if self.shadow_factor == 1.0 {
            return 0;
        }
        (self.metrics.scale.get() * self.shadow_factor * UiScale::SCALE_DENOMINATOR as f32).round()
            as u32
    }

    /// Converts bitmap-unit effects to the selected face's units without resizing them.
    fn shadow(self, shadow: TextShadow) -> TextShadow {
        match shadow {
            TextShadow::None => TextShadow::None,
            TextShadow::Offset64(offset) => {
                TextShadow::Offset64((offset as f32 * self.shadow_factor).round() as u32)
            }
        }
    }

    /// Adds paint alignment and capacity using layout's quantized line geometry.
    fn painted_request(
        self,
        text: &'a str,
        dest: [f32; 4],
        options: &TextOptions,
        align: TextAlign,
        px: f32,
    ) -> TextLayoutRequest<'a> {
        let shape = LabelShape {
            scale: f64::from(self.authored_factor),
            line_padding: f64::from(options.line_padding),
            hide_hyphen: options.hide_hyphen,
        };
        let mut request = self.request(text, f64::from(dest[2] - dest[0]), shape, px);
        let height_64 = ((dest[3] - dest[1]).max(0.0) * 64.0).round() as u32;
        request.wrap.max_lines = Some(request.max_lines_in_height(height_64).unwrap_or(1).max(1));
        request.wrap.align = match align {
            TextAlign::Left => TextLineAlign::Left,
            TextAlign::Center => TextLineAlign::Center,
            TextAlign::Right => TextLineAlign::Right,
        };
        request.wrap.align_grid_65536 = if self.font.linear_sampling() {
            0
        } else {
            super::pixel_snap::align_grid_65536(self.authored_factor, self.metrics.gui_scale, px)
        };
        request
    }
}

/// Rounded up, so text laid out at its own measured width does not wrap.
pub fn width_64(logical: f64) -> u32 {
    (logical.clamp(1.0, UNWRAPPED_LOGICAL) * 64.0).ceil() as u32
}

/// Vanilla label measurement through the font's shared layout cache; `px` is logical px per GUI px.
pub struct Measure<'a, 'b> {
    pub layouts: &'b RefCell<&'a mut TextLayoutCache>,
    pub font: &'a RuntimeFontCatalog,
    pub metrics: TextMetrics,
    pub px: f32,
    pub translate: &'a dyn Fn(&str) -> Option<Arc<str>>,
}

impl Measure<'_, '_> {
    /// Measures with the selected face while keeping the default bitmap's label metrics
    /// separate from an attached named font and the shared chat metrics.
    fn font_label(
        &self,
        text: &str,
        max_width: Option<f64>,
        shape: LabelShape,
        font: &RuntimeFontCatalog,
    ) -> [f64; 2] {
        if text.is_empty() {
            return [0.0, 0.0];
        }
        let px = f64::from(self.px);
        let width = max_width
            .filter(|width| *width > 0.0)
            .map_or(UNWRAPPED_LOGICAL, |width| width * px);
        let metrics = label_metrics(self.metrics, self.font, font, shape.scale as f32);
        let request = metrics.request(text, width, shape, self.px);
        match self.layouts.borrow_mut().layout(request) {
            Ok(layout) => layout.size_64().map(|size| f64::from(size) / 64.0 / px),
            Err(_) => [0.0, 0.0],
        }
    }
}

impl TextMeasure for Measure<'_, '_> {
    /// Converts the renderer's physical coverage pixel into the layout's virtual units.
    fn physical_pixel_size(&self) -> f64 {
        1.0 / f64::from(self.px * self.metrics.dpi_scale.get())
    }

    /// Uses the same destination snapping as the canvas, then returns virtual coordinates.
    fn paint_rect(&self, rect: json_ui::Rect) -> json_ui::Rect {
        let [x0, y0, x1, y1] = super::pixel_snap::snapped(
            [rect.x, rect.y, rect.w, rect.h],
            self.metrics.gui_scale,
            self.px,
        )
        .map(|edge| f64::from(edge / self.px));
        json_ui::Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    fn extent(&self, text: &str) -> [f64; 2] {
        self.wrapped(text, UNWRAPPED_LOGICAL / f64::from(self.px))
    }

    fn wrapped(&self, text: &str, max_width: f64) -> [f64; 2] {
        self.label(
            text,
            Some(max_width),
            LabelShape {
                scale: 1.0,
                line_padding: 0.0,
                hide_hyphen: false,
            },
        )
    }

    fn label(&self, text: &str, max_width: Option<f64>, shape: LabelShape) -> [f64; 2] {
        self.font_label(text, max_width, shape, self.font)
    }

    /// Keeps label shaping while selecting the pack's named font.
    fn named_label(
        &self,
        text: &str,
        font: &str,
        backup: Option<&str>,
        width: Option<f64>,
        shape: LabelShape,
    ) -> [f64; 2] {
        let name = self.font.resolve_font_name(font, backup);
        self.font_label(text, width, shape, self.font.font_named(name))
    }

    fn localize<'t>(&self, text: &'t str) -> Cow<'t, str> {
        localized(text, self.translate)
    }
}

/// Builds the same wrapping, alignment and line limit used by label painting.
#[allow(clippy::too_many_arguments)]
pub fn painted_label_request<'a>(
    metrics: TextMetrics,
    text: &'a str,
    dest: [f32; 4],
    font: &'a RuntimeFontCatalog,
    scale: f32,
    options: &TextOptions,
    align: TextAlign,
    px: f32,
) -> TextLayoutRequest<'a> {
    let selected = label_font(font, options);
    label_metrics(metrics, font, selected, scale).painted_request(text, dest, options, align, px)
}

impl Canvas<'_> {
    /// A label's text as one layout: lines past its height drop and the last
    /// kept one ends in `...`; each line aligns within the label's width.
    pub fn text(
        &mut self,
        text: &str,
        dest: [f32; 4],
        clip: [f32; 4],
        style: TextPaint<'_>,
    ) -> Result<(), PaintError> {
        let text = if style.localize {
            localized(text, self.translate)
        } else {
            Cow::Borrowed(text)
        };
        if text.is_empty() {
            return Ok(());
        }
        let selected = label_font(self.font, style.options);
        let metrics = label_metrics(self.metrics, self.font, selected, style.scale);
        let request = metrics.painted_request(&text, dest, style.options, style.align, self.px);
        let Ok(layout) = self.layouts.layout(request) else {
            return Ok(());
        };
        let [width, height] = layout.size_64().map(|size| size as f32 / 64.0);
        let [left, top] = label_origin(dest, self.font, style.options, self.px);
        let parent = self.group(clip)?;
        let shadow = metrics.shadow(style.shadow);
        if self.paint_bounds.is_some()
            && let Some(ink) = glyph_ink(layout.glyphs())
        {
            let padding = match shadow {
                TextShadow::None => 0.,
                TextShadow::Offset64(offset) => {
                    layout.key().scale_1024 as f32 / 1024. * offset as f32 / 64.
                }
            };
            self.observe_paint_bounds(
                [
                    left + ink[0],
                    top + ink[1],
                    left + ink[2] + padding,
                    top + ink[3] + padding,
                ],
                clip,
            );
        }
        let id = self.id();
        self.nodes.push(
            UiNode::new(
                id,
                Some(parent),
                rect(
                    left - clip[0],
                    top - clip[1],
                    left + width.max(1.0) - clip[0],
                    top + height - clip[1],
                )?,
            )
            .with_visual(UiVisual::Text {
                layout,
                color: style.color,
                shadow,
            }),
        );
        if let Some(selection) = style.edit.and_then(|edit| edit.selection) {
            let mut edges = [0.0; 2];
            for (edge, byte) in edges.iter_mut().zip(selection) {
                let byte = crate::caret::caret_byte(&text, byte);
                let prefix = &text[..byte];
                if !prefix.is_empty() {
                    *edge = self
                        .layouts
                        .layout(TextLayoutRequest {
                            text: prefix,
                            ..request
                        })
                        .map_err(PaintError::Text)?
                        .size_64()[0] as f32
                        / 64.0;
                }
            }
            self.push(
                UiVisual::InvertedSprite {
                    texture_page: self.solid_page,
                    uv: [0, 0, 1, 1],
                },
                [dest[0] + edges[0], top, dest[0] + edges[1], top + height],
            )?;
        }
        Ok(())
    }
}

/// Name tag backing is black at alpha 0.25.
const NAME_TAG_BACKGROUND: [u8; 4] = [0, 0, 0, 64];

impl Canvas<'_> {
    /// `#playername` centred on the control over a backing one GUI px wider
    /// than the text, plus `#x_padding`/`#y_padding`.
    pub(super) fn name_tag(
        &mut self,
        data: &std::collections::BTreeMap<String, serde_json::Value>,
        dest: [f32; 4],
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Option<(UiVisual, [f32; 4])> {
        let name = data
            .get("#playername")?
            .as_str()
            .filter(|name| !name.is_empty())?;
        let px = self.px;
        let padding = |key: &str| {
            data.get(key)
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0) as f32
                * px
        };
        let (pad_x, pad_y) = (padding("#x_padding"), padding("#y_padding"));
        let request = self
            .metrics
            .request(name, width_64(UNWRAPPED_LOGICAL), self.font);
        let layout = self.layouts.layout(request).ok()?;
        let [w, h] = layout.size_64().map(|size| size as f32 / 64.0);
        let centre = [(dest[0] + dest[2]) * 0.5, (dest[1] + dest[3]) * 0.5];
        let left = centre[0] - w * 0.5;
        let top = centre[1] - h * 0.5;
        self.group(self.screen).ok()?;
        self.solid(
            [
                left - px - pad_x,
                top - px - pad_y,
                left + w + pad_x,
                top + h + pad_y,
            ],
            alpha(NAME_TAG_BACKGROUND),
        )
        .ok()?;
        Some((
            UiVisual::Text {
                layout,
                color: alpha([255; 4]),
                shadow: TextShadow::None,
            },
            [left, top, left + w, top + h],
        ))
    }
}

/// The format codes in force at the end of `text`, to open the next line with.
pub fn active_codes(text: &str) -> String {
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

/// The union of positioned glyph quads relative to the label origin, excluding alignment space;
/// none when no glyph paints.
fn glyph_ink(glyphs: &[ui::GlyphQuad]) -> Option<[f32; 4]> {
    glyphs
        .iter()
        .map(|glyph| glyph.bounds_64.map(|edge| edge as f32 / 64.0))
        .reduce(|a, b| {
            [
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]
        })
}

#[cfg(test)]
mod tests;
