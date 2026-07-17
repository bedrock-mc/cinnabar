//! Vendor-independent user-interface primitives.

mod action;
mod geometry;
mod settings;
mod text;

pub use action::{PointerPhase, UiAction, UiLimits};
pub use geometry::{DpiScale, GeometryError, SafeArea, UiPoint, UiRect, UiScale};
pub use settings::{CURRENT_SETTINGS_SCHEMA, GameplaySettings, UserSettings, VideoSettings};
pub use text::{
    BedrockColor, GlyphQuad, MAX_GLYPHS_PER_LAYOUT, MAX_TEXT_SPANS, MAX_WRAP_LINES, TextError,
    TextLayout, TextLayoutCache, TextLayoutKey, TextLayoutRequest, TextSpan, TextSpans, TextStyle,
    parse_bedrock_text,
};
