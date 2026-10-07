//! A `label`'s text component, as vanilla reads it: glyph
//! scale from `font_size` and `font_scale_factor`, `line_padding`, the locked
//! (disabled) colour and alpha, hyphen and font options.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tree::ResolvedControl;
use crate::widgets;

/// 1.26.50's `ui::FontSize` scales: small, normal, large, extra_large.
const FONT_SIZE_SCALES: [f64; 4] = [0.5, 1.0, 2.0, 4.0];

/// Label options the text painter needs besides colour and alignment.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextOptions {
    /// Extra virtual pixels between lines.
    pub line_padding: f32,
    /// `hide_hyphen`: chop an overlong word without drawing its `-`.
    pub hide_hyphen: bool,
    /// `font_type`/`backup_font_type` as authored; the host picks the font.
    pub font_type: Option<String>,
    pub backup_font_type: Option<String>,
    /// `enable_profanity_filter`.
    pub profanity_filter: bool,
}

/// The measured shape of a label's text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabelShape {
    pub scale: f64,
    pub line_padding: f64,
    pub hide_hyphen: bool,
}

impl LabelShape {
    pub(crate) fn of(control: &ResolvedControl) -> Self {
        Self {
            scale: font_scale(control),
            line_padding: widgets::bound_number(control, "line_padding").unwrap_or(0.0),
            hide_hyphen: widgets::bound_bool(control, "hide_hyphen") == Some(true),
        }
    }
}

/// A label's glyph scale: its `font_size` step times `font_scale_factor`
/// (1 when absent or non-positive).
pub(crate) fn font_scale(control: &ResolvedControl) -> f64 {
    let factor = widgets::bound_number(control, "font_scale_factor")
        .filter(|scale| *scale > 0.0)
        .unwrap_or(1.0);
    let size = match control.properties.get("font_size").and_then(Value::as_str) {
        Some("small") => 0,
        Some("large") => 2,
        Some("extra_large") => 3,
        _ => 1,
    };
    factor * FONT_SIZE_SCALES[size]
}

/// A label localizes its text unless `localize` is `false`.
pub(crate) fn localizes(control: &ResolvedControl) -> bool {
    control.properties.get("localize") != Some(&Value::Bool(false))
}

/// The label's text as it draws; binding already emptied an unbound `#name`,
/// so a bound payload starting with `#` is literal text.
pub(crate) fn text(control: &ResolvedControl) -> String {
    let texts = |key: &str| control.properties.get(key).and_then(Value::as_array);
    // A `label_cycler` shows its first `text_labels` entry until it cycles.
    if control.control_type.as_deref() == Some("label_cycler") {
        return texts("text_labels")
            .and_then(|labels| labels.first())
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
    }
    control
        .properties
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The text colour: a disabled label (or one under a disabled control) uses
/// `locked_color` (default its colour) with its alpha scaled by `locked_alpha`.
pub(crate) fn color(control: &ResolvedControl, enabled: bool) -> [u8; 4] {
    let white = [255, 255, 255, 255];
    let base = match control.properties.get("#color") {
        Some(value) => crate::emit::color_value(value).unwrap_or(white),
        None => control
            .properties
            .get("color")
            .and_then(crate::emit::color_value)
            .unwrap_or(white),
    };
    if enabled {
        return base;
    }
    let mut locked = control
        .properties
        .get("locked_color")
        .and_then(crate::emit::color_value)
        .unwrap_or(base);
    let alpha = widgets::bound_number(control, "locked_alpha").unwrap_or(1.0);
    locked[3] = (f64::from(locked[3]) * alpha.clamp(0.0, 1.0)).round() as u8;
    locked
}

pub(crate) fn options(control: &ResolvedControl) -> TextOptions {
    let string = |key: &str| {
        control
            .properties
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    TextOptions {
        line_padding: widgets::bound_number(control, "line_padding").unwrap_or(0.0) as f32,
        hide_hyphen: widgets::bound_bool(control, "hide_hyphen") == Some(true),
        font_type: string("font_type"),
        backup_font_type: string("backup_font_type"),
        profanity_filter: widgets::bound_bool(control, "enable_profanity_filter") == Some(true),
    }
}

/// The label's text extent, wrapped at `width` when known.
pub(crate) fn natural(
    control: &ResolvedControl,
    env: &crate::layout::LayoutEnv,
    width: Option<f64>,
) -> [f64; 2] {
    let text = text(control);
    let text = if text.is_empty() && is_editable(control) {
        "_"
    } else {
        &text
    };
    let text = if localizes(control) {
        env.text.localize(text)
    } else {
        std::borrow::Cow::Borrowed(text)
    };
    env.text.named_label(
        &text,
        control
            .properties
            .get("font_type")
            .and_then(Value::as_str)
            .unwrap_or("default"),
        width,
        LabelShape::of(control),
    )
}

/// The property field identifies the label written by vanilla's text edit component.
pub(crate) fn is_editable(control: &ResolvedControl) -> bool {
    is_label(control)
        && control
            .properties
            .get("property_bag")
            .and_then(|bag| bag.get("#property_field"))
            .and_then(Value::as_str)
            == Some("#item_name")
}

/// Whether `control` draws text: a `label` or a `label_cycler`.
pub(crate) fn is_label(control: &ResolvedControl) -> bool {
    matches!(
        control.control_type.as_deref(),
        Some("label" | "label_cycler")
    )
}

#[cfg(test)]
mod tests;
