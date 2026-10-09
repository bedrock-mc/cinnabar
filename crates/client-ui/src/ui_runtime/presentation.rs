use std::{fmt, sync::Arc};

use assets::{RuntimeFontCatalog, RuntimeHudCatalog, RuntimeIconCatalog};
use bevy::prelude::Resource;
use render_model::{UiRenderInput, UiRenderTextureArray};
use sha2::{Digest, Sha256};

use ui::{
    DpiScale, ObfuscationGlyphs, SafeArea, TextEffects, TextLayoutCache, UiNode, UiNodeId, UiPoint,
    UiRect, UiScale, UiTree, UiVisual,
};

use super::scene_stack::Scene;
use super::{UiRuntime, render_adapter::UiRenderViewport};
use crate::ui_runtime::{item_facts, render_adapter::adapt_ui_draw_list};

mod accessors;
mod construction;
pub mod debug_overlay;
pub mod dynamic_textures;
mod font_fallback;
pub mod forms;
mod frame;
pub mod gui_models;
pub mod gui_scale_settings;
pub mod hud_layout;
pub mod inventory_pointer;
pub mod inventory_tooltip;
pub mod item_gui;
pub mod item_sprite;
pub mod item_viewmodel;
pub mod menu;
pub mod menu_artwork;
pub mod menu_scroll;
mod mod_panel_font;
mod runtime_assets;
pub use menu_artwork::BUILT_IN_TITLE;
pub mod nametag_atlas;
pub mod nametags;
pub mod paper_doll;
pub mod player_preview;
pub mod primitive_shapes;
pub mod primitives;
pub mod publish;
pub mod retained_hud;
pub mod screens;
pub mod session_glyphs;
pub mod session_icons;
pub use forms::{MAX_PACK_TEXTURE_BYTES, ServerUiPack};
pub use session_glyphs::SessionGlyphSheets;
pub use session_icons::{MAX_SESSION_ICON_SIDE, SessionIcon, SessionIcons};
pub mod startup;
pub mod text_metrics;
pub mod texture_atlas;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "dormant until presentation owns an exact walk-distance query cadence"
    )
)]
pub mod viewmodel_bob;

use crate::menu::{MenuAction, MenuView};
pub use debug_overlay::DebugLines;
pub use forms::{BedHit, ChatHit, ExperienceModal, LoadingStage, ModScreensInput, ModalEdits};
pub use hud_layout::HudFrame;
use hud_layout::{HudGeometry, HudLayout, gui_scale};
use primitives::{bounded_visible_text, rect, resolve_chat_line};
#[cfg(any(test, feature = "test-support"))]
pub use publish::commit::refresh_hud_frame;
pub use publish::{
    capture_hud_frame,
    commit::{PendingUiPublication, PreparedUiPublication, PreviewCapture, render_prepared_ui},
    item_icons::ItemIconFrames,
};
use retained_hud::{PresentedScoreboardCache, ScoreboardOwnerNameAuthority};
use startup::StartupPresentationState;
use text_metrics::{
    FONT_DESIGN_PIXEL_TEXELS, TEXT_BASELINE_64, TEXT_LINE_HEIGHT_64, TEXT_SHADOW_OFFSET_64,
    TextMetrics,
};
pub use texture_atlas::IconRef;
use texture_atlas::{
    HudTexturePages, font_texture_array, font_texture_array_with_hud_and_icons,
    font_texture_array_with_optional_hud,
};

const TEXT_CACHE_ENTRIES: usize = 1_024;
const TEXT_CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_PRESENTED_TEXT_BYTES: usize = 512;
#[derive(Debug)]
pub enum UiPresentationError {
    InvalidFontTexture,
    Geometry(ui::GeometryError),
    Text(ui::TextError),
    Tree(ui::UiError),
    Adapter(super::render_adapter::UiRenderAdapterError),
    Render(render_model::UiRenderReject),
}

impl fmt::Display for UiPresentationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "UI presentation failed: {self:?}")
    }
}

impl std::error::Error for UiPresentationError {}

#[derive(Resource)]
pub struct UiPresentationRuntime {
    font: Arc<RuntimeFontCatalog>,
    /// The startup font without the session's glyph sheets.
    base_font: Arc<RuntimeFontCatalog>,
    mod_panel_font: Option<mod_panel_font::InstalledFont>,
    fallback_font: Option<Arc<RuntimeFontCatalog>>,
    textures: Arc<UiRenderTextureArray>,
    texture_session: Option<u64>,
    blank_dynamic_page: render_model::UiTexturePage,
    solid_texture_page: u16,
    hud_textures: Option<HudTexturePages>,
    icon_catalog: Option<Arc<RuntimeIconCatalog>>,
    icon_refs: Option<Box<[IconRef]>>,
    layouts: TextLayoutCache,
    obfuscation: ObfuscationGlyphs, // same-width pools for the per-frame §k swap
    revision: u64,
    last_input: Option<UiRenderInput>, // last built frame; see `stabilize_revision`
    /// What the last frame was built from, while time cannot change its output.
    last_frame: Option<frame::BuiltFrame>,
    /// Scene assembly keeps its capacity across unchanged frames and failed builds.
    assembly_nodes: Vec<UiNode>,
    retained_menu: Option<forms::RetainedMenu>,
    #[cfg(test)]
    tree_builds: usize,
    #[cfg(test)]
    oreui_paints: usize,
    scoreboard: PresentedScoreboardCache,
    scoreboard_owner_names: ScoreboardOwnerNameAuthority,
    debug_lines: Option<DebugLines>,
    debug_overlay: debug_overlay::OverlayCache,
    /// Bedrock desktop GUI-scale preference: `None`/0 selects the auto rule.
    gui_scale_preference: Option<u8>,
    /// Platform safe-area insets in logical px, applied to the HUD geometry,
    /// the retained tree layout, and the render viewport alike.
    safe_area: SafeArea,
    /// Item facts and camera state refreshed immediately before each build.
    hud_frame: HudFrame,
    /// Last logged skip/odd-data counters, so changes surface exactly once.
    last_hud_diagnostics: crate::ui_runtime::gameplay_hud::GameplayHudDiagnostics,
    /// This frame's world-space tags, including scores, and their retained glyph atlas.
    nametag_anchors: Vec<nametags::NametagAnchor>,
    nametag_atlas: nametag_atlas::NametagAtlas,
    primitive_text: primitive_shapes::PrimitiveTextRasterizer,
    /// Stable reserved logical page for the optional preview raster.
    paper_doll: paper_doll::PaperDoll,
    player_preview_page: Option<u16>,
    player_preview_source_hash: Option<[u8; 32]>,
    player_preview_pose: Option<player_preview::PlayerPreviewPose>,
    /// How the UI last asked to show the model, and the idle sway it was drawn at.
    player_preview_view: player_preview::PreviewView,
    menu_preview_model: player_preview::model::MenuPreviewModel,
    menu_preview: player_preview::controller::MenuPreview,
    player_preview_drawn: Option<(
        player_preview::PreviewView,
        f32,
        player_preview::PreviewEquipment,
    )>,
    player_preview_bob: f32,
    /// Worn armor and the held item the model shows, and where armor art comes from.
    player_preview_gear: player_preview::PreviewEquipment,
    equipment_catalog: Option<Arc<assets::RuntimeEquipmentCatalog>>,
    gui_models: gui_models::GuiModels,
    player_preview_pixels: Option<player_preview::PlayerPreviewRasters>,
    preview_dirty: bool,
    player_preview_icon: Option<IconRef>,
    left_hand_icon: Option<IconRef>,
    right_hand_icon: Option<IconRef>,
    held_viewmodel_source: Option<IconRef>,
    offhand_viewmodel_source: Option<IconRef>,
    held_viewmodel_icon: Option<IconRef>,
    offhand_viewmodel_icon: Option<IconRef>,
    /// The art set last requested: service art plus engine textures too big for a server page.
    menu_artwork_set: menu_artwork::ArtworkSet,
    menu_artwork_loader: menu_artwork::ArtworkLoader,
    /// This frame's clock in seconds, which engine screen animations paint at.
    menu_seconds: f64,
    scene_clocks: super::scene_stack::SceneClocks,
    /// The drawing scene's transition event clocks.
    scene_clock: std::collections::BTreeMap<String, f64>,
    menu_artwork: menu_artwork::MenuArtworkAtlas,
    /// The installed refs must be rebased onto moved art pages.
    menu_artwork_dirty: bool,
    session_icons: session_icons::SessionIconPage,
    session_glyphs: session_glyphs::SessionGlyphPages,
    /// Identifiers already logged as iconless.
    missing_icons: std::sync::Mutex<std::collections::HashSet<String>>,
    /// The hotbar last logged: each slot's identifier and whether it had an icon.
    logged_hotbar: [Option<(Arc<str>, bool)>; 9],
    menu_view: Option<Arc<MenuView>>,
    menu_hit_targets: Vec<(MenuAction, UiRect)>,
    menu_skin_thumbnail_indices: Vec<usize>,
    menu_cape_thumbnail_indices: Vec<usize>,
    /// Full settings slider geometry, including steps clipped from view.
    settings_slider_drag_targets: Vec<(MenuAction, UiRect)>,
    menu_scrolls: menu_scroll::MenuScrolls,
    form_presentation: forms::FormPresentation,
    /// Window-space rect of the sign editor's Done button in the last build.
    loading_stage: Option<LoadingStage>,
    startup: StartupPresentationState,
}

#[cfg(test)]
pub mod tests;
