//! Vendor-independent user-interface primitives.

mod action;
mod chat;
mod geometry;
mod hud;
mod icon;
pub mod mod_hud;
pub mod mod_panel;
mod model;
mod scoreboard;
mod settings;
mod text;

pub use action::{PointerPhase, UiAction, UiLimits};
pub use chat::{
    ChatApplyResult, ChatAutocompleteAction, ChatAutocompleteApply, ChatAutocompleteDelta,
    ChatAutocompleteError, ChatAutocompleteRequest, ChatAutocompleteResponse,
    ChatAutocompleteState, ChatClipboard, ChatEditor, ChatEditorError, ChatHistory, ChatMessage,
    ChatMessageKind, ChatPasteError, ChatRateLimit, ChatSendError, ChatSendQueue, ChatSendRequest,
    ChatStore, ChatViewNode, MAX_CHAT_AUTOCOMPLETE, MAX_CHAT_AUTOCOMPLETE_BYTES, MAX_CHAT_HISTORY,
    MAX_CHAT_INPUT_BYTES, MAX_CHAT_MESSAGES, MAX_CHAT_RETAINED_BYTES, MAX_PENDING_CHAT_SENDS,
};
pub use geometry::{
    DesktopGuiScale, DesktopGuiScaleChoice, DpiScale, GeometryError, SafeArea, UiPoint, UiRect,
    UiScale, gui_scale,
};
pub use hud::{
    BoundedStat, HudExperience, HudPlayerStatus, HudStore, HudViewNode, HudViewRole,
    MAX_TOAST_RETAINED_BYTES, MAX_TOASTS, ShownToast, StandingToast, TOAST_DISPLAY_MILLIS,
    TOAST_SLIDE_IN_MILLIS, TOAST_SLIDE_OUT_MILLIS, TimedText, TitleDurations, Toast, ToastPress,
};
pub use icon::IconRef;
pub use model::{
    DrawUpdate, FocusState, FocusTransition, RetainedDraw, TextEffects, TextShadow,
    UI_STYLE_BILINEAR, UI_STYLE_GLINT, UI_STYLE_GRAYSCALE, UiBlendMode, UiDrawBatch, UiDrawList,
    UiError, UiFrame, UiMesh, UiMeshBatch, UiMeshError, UiMeshVertex, UiNode, UiNodeId, UiTree,
    UiVertex, UiVisual, UiWorldProjection,
};
pub use render_api::EnhancedQuality;
pub use scoreboard::{
    BossAction, BossBarDiagnostics, BossBarEvent, BossBarStore, BossBarView, BossColor,
    BossOverlay, BossStyle, DisplaySlot, MAX_BOSS_BARS, MAX_BOSS_RETAINED_TEXT_BYTES,
    MAX_OBJECTIVES, MAX_RETAINED_UI_TEXT_FIELD_BYTES, MAX_SCOREBOARD_RETAINED_TEXT_BYTES,
    MAX_SCORES, RetainedUiApply, RetainedUiSequenceError, ScoreAction, ScoreEntry, ScoreIdentity,
    ScoreOwner, ScoreRenderType, ScoreRow, ScoreSortOrder, ScoreboardDiagnostics, ScoreboardEvent,
    ScoreboardProjection, ScoreboardStore,
};
pub use settings::{
    ANTI_ALIASING_SAMPLE_COUNTS, AntiAliasingSupport, CURRENT_SETTINGS_SCHEMA,
    DEFAULT_ANTI_ALIASING_SAMPLES, DEFAULT_FOV_DEGREES, DEFAULT_OUTLINE_SELECTION,
    DEFAULT_RENDER_DISTANCE_CHUNKS, DEFAULT_SMAA_MODE, GameplaySettings, MAX_FOV_DEGREES,
    MIN_FOV_DEGREES, MOTION_BLUR_REFERENCE_FPS, MotionBlurQuality, RenderMode, SmaaMode,
    UserSettings, VideoSettings,
};
pub use text::{
    BedrockColor, FONT_ASCENT_TEXELS, FONT_DESIGN_PIXEL_TEXELS, FONT_INK_TEXELS, FormattingPalette,
    GlyphQuad, MAX_GLYPHS_PER_LAYOUT, MAX_TEXT_SPANS, MAX_WRAP_LINES, ObfuscationGlyphs,
    TEXT_BASELINE_64, TEXT_BOLD_OFFSET_64, TEXT_LINE_HEIGHT_64, TEXT_SHADOW_OFFSET_64, TextError,
    TextLayout, TextLayoutCache, TextLayoutKey, TextLayoutRequest, TextLineAlign, TextSpan,
    TextSpans, TextStyle, TextWrap, WordChop, parse_bedrock_text,
};
