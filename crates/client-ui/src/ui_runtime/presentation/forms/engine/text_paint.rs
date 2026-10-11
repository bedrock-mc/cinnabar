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

use super::super::super::{TextMetrics, UiPresentationError, rect};
use super::{Painter, tooltip::TEXT_PITCH};

/// Largest wrap width handed to the text layout (logical px), for "no wrap".
pub(in super::super) const UNWRAPPED_LOGICAL: f64 = 65_536.0;

/// Default bitmap labels use the font's native wrap height, independently of chat's pitch.
/// Attached named fonts keep their existing metrics instead of inheriting bitmap geometry.
fn label_metrics(
    mut metrics: TextMetrics,
    default: &RuntimeFontCatalog,
    selected: &RuntimeFontCatalog,
) -> TextMetrics {
    if std::ptr::eq(default, selected) {
        metrics.line_height_64 = TEXT_PITCH * FONT_DESIGN_PIXEL_TEXELS * 64;
    }
    metrics
}

/// Native bitmap UI text starts one GUI pixel below the authored label top. The inset is
/// independent of font size; painting and glyph interaction use this same origin.
pub(in super::super) fn label_origin(
    dest: [f32; 4],
    font: &RuntimeFontCatalog,
    options: &TextOptions,
    px: f32,
) -> [f32; 2] {
    let selected = font.font_named(options.font_type.as_deref().unwrap_or("default"));
    let inset = if std::ptr::eq(font, selected) {
        px
    } else {
        0.0
    };
    [dest[0], dest[1] + inset]
}

#[derive(Clone)]
pub(super) struct TextPaint {
    pub(super) color: [u8; 4],
    pub(super) edit: Option<super::host_edit::Feedback>,
    pub(super) shadow: TextShadow,
    pub(super) align: TextAlign,
    pub(super) scale: f32,
    pub(super) localize: bool,
    pub(super) options: TextOptions,
}

/// A label's text after vanilla localization; empty lines drop as the vanilla label drops them.
pub(super) fn localized<'a>(
    text: &'a str,
    translate: &dyn Fn(&str) -> Option<Arc<str>>,
) -> Cow<'a, str> {
    let text = json_ui::localize_text(text, translate);
    if text.contains("\n\n") || text.starts_with('\n') || text.ends_with('\n') {
        let lines: Vec<&str> = text.split('\n').filter(|line| !line.is_empty()).collect();
        return Cow::Owned(lines.join("\n"));
    }
    text
}

/// `request` at `factor` times the metrics' scale; a factor the display range
/// cannot hold keeps the base scale.
pub(super) fn scaled_request<'a>(
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

/// A vanilla label's request: hyphen chops, `line_padding` in logical px.
fn label_request<'a>(
    metrics: &TextMetrics,
    text: &'a str,
    width: f64,
    font: &'a RuntimeFontCatalog,
    shape: LabelShape,
    px: f32,
) -> TextLayoutRequest<'a> {
    let mut request = scaled_request(metrics, text, width_64(width), font, shape.scale as f32);
    request.wrap = TextWrap {
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

/// Rounded up, so text laid out at its own measured width does not wrap.
pub(in super::super) fn width_64(logical: f64) -> u32 {
    (logical.clamp(1.0, UNWRAPPED_LOGICAL) * 64.0).ceil() as u32
}

pub(super) struct Measure<'a, 'b> {
    pub(super) layouts: &'b RefCell<&'a mut TextLayoutCache>,
    pub(super) font: &'a RuntimeFontCatalog,
    pub(super) metrics: TextMetrics,
    pub(super) px: f32,
    pub(super) translate: &'a dyn Fn(&str) -> Option<Arc<str>>,
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
        let metrics = label_metrics(self.metrics, self.font, font);
        let request = label_request(&metrics, text, width, font, shape, self.px);
        match self.layouts.borrow_mut().layout(request) {
            Ok(layout) => layout.size_64().map(|size| f64::from(size) / 64.0 / px),
            Err(_) => [0.0, 0.0],
        }
    }
}

impl TextMeasure for Measure<'_, '_> {
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
        width: Option<f64>,
        shape: LabelShape,
    ) -> [f64; 2] {
        self.font_label(text, width, shape, self.font.font_named(font))
    }

    fn localize<'t>(&self, text: &'t str) -> Cow<'t, str> {
        localized(text, self.translate)
    }
}

/// Builds the same wrapping, alignment and line limit used by label painting.
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn painted_label_request<'a>(
    metrics: TextMetrics,
    text: &'a str,
    dest: [f32; 4],
    font: &'a RuntimeFontCatalog,
    scale: f32,
    options: &TextOptions,
    align: TextAlign,
    px: f32,
) -> TextLayoutRequest<'a> {
    let shape = LabelShape {
        scale: f64::from(scale),
        line_padding: f64::from(options.line_padding),
        hide_hyphen: options.hide_hyphen,
    };
    let selected = font.font_named(options.font_type.as_deref().unwrap_or("default"));
    let label_metrics = label_metrics(metrics, font, selected);
    let mut request = label_request(
        &label_metrics,
        text,
        f64::from(dest[2] - dest[0]),
        selected,
        shape,
        px,
    );
    let pitch = (request.line_height_64 as f32 * request.scale.get()
        + request.wrap.line_padding_64 as f32)
        / 64.0;
    let room = ((dest[3] - dest[1]) / pitch.max(1e-3) + 0.01)
        .floor()
        .max(1.0);
    request.wrap.max_lines = Some(room.min(f32::from(u16::MAX)) as u16);
    request.wrap.align = match align {
        TextAlign::Left => TextLineAlign::Left,
        TextAlign::Center => TextLineAlign::Center,
        TextAlign::Right => TextLineAlign::Right,
    };
    request.wrap.align_grid_65536 =
        super::pixel_snap::align_grid_65536(scale, metrics.gui_scale, px);
    request
}

impl Painter<'_> {
    /// A label's text as one layout: lines past its height drop and the last
    /// kept one ends in `...`; each line aligns within the label's width.
    pub(super) fn text(
        &mut self,
        text: &str,
        dest: [f32; 4],
        clip: [f32; 4],
        style: TextPaint,
    ) -> Result<(), UiPresentationError> {
        let text = if style.localize {
            localized(text, self.translate)
        } else {
            Cow::Borrowed(text)
        };
        if text.is_empty() {
            return Ok(());
        }
        let request = painted_label_request(
            self.metrics,
            &text,
            dest,
            self.font,
            style.scale,
            &style.options,
            style.align,
            self.px,
        );
        let Ok(layout) = self.layouts.layout(request) else {
            return Ok(());
        };
        let [width, height] = layout.size_64().map(|size| size as f32 / 64.0);
        let [left, top] = label_origin(dest, self.font, &style.options, self.px);
        let parent = self.group(clip)?;
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
                shadow: style.shadow,
            }),
        );
        if let Some(selection) = style.edit.and_then(|edit| edit.selection) {
            let mut edges = [0.0; 2];
            for (edge, byte) in edges.iter_mut().zip(selection) {
                let byte = super::super::menu_caret::caret_byte(&text, byte);
                let prefix = &text[..byte];
                if !prefix.is_empty() {
                    *edge = self
                        .layouts
                        .layout(TextLayoutRequest {
                            text: prefix,
                            ..request
                        })
                        .map_err(UiPresentationError::Text)?
                        .size_64()[0] as f32
                        / 64.0;
                }
            }
            self.push(
                UiVisual::InvertedSprite {
                    texture_page: self.solid_page,
                    uv: [0, 0, 1, 1],
                },
                [
                    dest[0] + edges[0],
                    dest[1],
                    dest[0] + edges[1],
                    dest[1] + height,
                ],
            )?;
        }
        Ok(())
    }
}

/// The format codes in force at the end of `text`, to open the next line with.
pub(in super::super) fn active_codes(text: &str) -> String {
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
mod tests;
