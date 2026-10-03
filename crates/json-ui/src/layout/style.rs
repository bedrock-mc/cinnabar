//! Typed placement properties shared by repeated layouts of one bound control.

use serde_json::Value;

use crate::{tree::ResolvedControl, widgets};

#[derive(Clone, Copy)]
pub(super) struct Style {
    pub(super) visible: bool,
    pub(super) clips: bool,
    pub(super) clip_offset: [f64; 2],
    pub(super) allows: Option<bool>,
    /// Any descendant that can deliberately escape an ancestor clip.
    pub(super) unclipped_descendant: bool,
    pub(super) enabled: bool,
    pub(super) alpha: f32,
    pub(super) layer: i32,
}

impl Style {
    /// Read only bound properties; ancestor, interaction and scroll state stay live.
    pub(super) fn read(control: &ResolvedControl) -> Self {
        Self {
            visible: own_visible(control),
            clips: clip_children(control),
            clip_offset: clip_offset(control),
            allows: widgets::bound_bool(control, "allow_clipping"),
            unclipped_descendant: control.children.iter().any(|child| {
                let style = super::measure::style(child);
                style.allows == Some(false) || style.unclipped_descendant
            }),
            enabled: widgets::enabled(control),
            alpha: alpha(control),
            layer: layer(control),
        }
    }
}

/// `clip_offset`, `[0, 0]` unless a numeric pair.
fn clip_offset(control: &ResolvedControl) -> [f64; 2] {
    let Some(Value::Array(pair)) = control.properties.get("clip_offset") else {
        return [0.0; 2];
    };
    let number = |index: usize| pair.get(index).and_then(Value::as_f64).unwrap_or(0.0);
    [number(0), number(1)]
}

/// Whether either authored clipping flag enables child clipping.
fn clip_children(control: &ResolvedControl) -> bool {
    ["clips_children", "clip_children"]
        .iter()
        .any(|key| matches!(control.properties.get(*key), Some(Value::Bool(true))))
}

/// The relative layer, with the same integer cast as uncached placement.
fn layer(control: &ResolvedControl) -> i32 {
    control
        .properties
        .get("layer")
        .and_then(Value::as_i64)
        .map(|value| value as i32)
        .unwrap_or(0)
}

/// The static alpha, before inherited and animated contributors.
fn alpha(control: &ResolvedControl) -> f32 {
    control
        .properties
        .get("alpha")
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .unwrap_or(1.0)
}

/// `visible` honours a literal bool or `"true"`/`"false"`; an undecidable binding
/// stays visible, matching the lenient-remote-data rule.
fn own_visible(control: &ResolvedControl) -> bool {
    match control.properties.get("visible") {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => text != "false",
        _ => true,
    }
}
