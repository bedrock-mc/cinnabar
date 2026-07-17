//! Vendor-independent user-interface primitives.

mod action;
mod chat;
mod geometry;
mod hud;
mod model;
mod settings;
mod text;

pub use action::{PointerPhase, UiAction, UiLimits};
pub use chat::{
    ChatApplyResult, ChatAutocompleteAction, ChatAutocompleteApply, ChatAutocompleteDelta,
    ChatAutocompleteError, ChatAutocompleteRequest, ChatAutocompleteState, ChatEditor,
    ChatEditorError, ChatHistory, ChatMessage, ChatMessageKind, ChatRateLimit, ChatSendError,
    ChatSendQueue, ChatSendRequest, ChatStore, ChatViewNode, MAX_CHAT_AUTOCOMPLETE,
    MAX_CHAT_AUTOCOMPLETE_BYTES, MAX_CHAT_HISTORY, MAX_CHAT_INPUT_BYTES, MAX_CHAT_MESSAGES,
    MAX_CHAT_RETAINED_BYTES, MAX_PENDING_CHAT_SENDS,
};
pub use geometry::{DpiScale, GeometryError, SafeArea, UiPoint, UiRect, UiScale};
pub use hud::{
    BoundedStat, HudPlayerStatus, HudStore, HudViewNode, HudViewRole, MAX_TOAST_RETAINED_BYTES,
    MAX_TOASTS, TimedText, TitleDurations, Toast,
};
pub use model::{
    FocusState, FocusTransition, UiDrawBatch, UiDrawList, UiError, UiFrame, UiNode, UiNodeId,
    UiTree, UiVertex, UiVisual,
};
pub use settings::{CURRENT_SETTINGS_SCHEMA, GameplaySettings, UserSettings, VideoSettings};
pub use text::{
    BedrockColor, GlyphQuad, MAX_GLYPHS_PER_LAYOUT, MAX_TEXT_SPANS, MAX_WRAP_LINES, TextError,
    TextLayout, TextLayoutCache, TextLayoutKey, TextLayoutRequest, TextSpan, TextSpans, TextStyle,
    parse_bedrock_text,
};
