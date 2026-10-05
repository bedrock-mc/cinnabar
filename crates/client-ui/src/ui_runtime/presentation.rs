use std::{fmt, sync::Arc};

use assets::{RuntimeFontCatalog, RuntimeHudCatalog, RuntimeIconCatalog};
use bevy::prelude::Resource;
use render_model::{UiRenderInput, UiRenderTextureArray};
use sha2::{Digest, Sha256};

use ui::{
    DpiScale, ObfuscationGlyphs, SafeArea, TextEffects, TextLayoutCache, UiNode, UiNodeId, UiPoint,
    UiRect, UiScale, UiTree, UiVisual,
};

use super::scene_stack::{Scene, SceneHost};
use super::{UiRuntime, render_adapter::UiRenderViewport};
use crate::ui_runtime::{item_facts, render_adapter::adapt_ui_draw_list};

pub mod debug_overlay;
pub mod dynamic_textures;
pub mod forms;
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
pub use menu_artwork::BUILT_IN_TITLE;
pub mod nametag_atlas;
pub mod nametags;
pub mod paper_doll;
pub mod player_preview;
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
pub use forms::{BedHit, ChatHit, LoadingStage};
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
    last_frame: Option<BuiltFrame>,
    #[cfg(test)]
    tree_builds: usize,
    scoreboard: PresentedScoreboardCache,
    scoreboard_owner_names: ScoreboardOwnerNameAuthority,
    debug_lines: Option<DebugLines>,
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
    /// Stable reserved logical page for the optional preview raster.
    paper_doll: paper_doll::PaperDoll,
    player_preview_page: Option<u16>,
    player_preview_source_hash: Option<[u8; 32]>,
    player_preview_pose: Option<player_preview::PlayerPreviewPose>,
    /// How the UI last asked to show the model, and the idle sway it was drawn at.
    player_preview_view: player_preview::PreviewView,
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
    menu_view: Option<MenuView>,
    menu_hit_targets: Vec<(MenuAction, UiRect)>,
    /// Current full GUI slider geometry, including steps clipped from view.
    /// Captured drags keep following it while scale changes move the row.
    gui_scale_drag_targets: Vec<(MenuAction, UiRect)>,
    menu_scrolls: menu_scroll::MenuScrolls,
    form_presentation: forms::FormPresentation,
    /// Window-space rect of the sign editor's Done button in the last build.
    loading_stage: Option<LoadingStage>,
    startup: StartupPresentationState,
}

impl UiPresentationRuntime {
    pub fn new(font: Arc<RuntimeFontCatalog>) -> Result<Self, UiPresentationError> {
        Self::with_optional_hud(font, None)
    }

    pub fn with_hud(
        font: Arc<RuntimeFontCatalog>,
        hud: Arc<RuntimeHudCatalog>,
    ) -> Result<Self, UiPresentationError> {
        Self::with_optional_assets(font, Some(hud), None)
    }

    pub fn with_hud_and_icons(
        font: Arc<RuntimeFontCatalog>,
        hud: Arc<RuntimeHudCatalog>,
        icons: Arc<RuntimeIconCatalog>,
    ) -> Result<Self, UiPresentationError> {
        Self::with_optional_assets(font, Some(hud), Some(icons))
    }

    fn with_optional_assets(
        font: Arc<RuntimeFontCatalog>,
        hud: Option<Arc<RuntimeHudCatalog>>,
        icons: Option<Arc<RuntimeIconCatalog>>,
    ) -> Result<Self, UiPresentationError> {
        let (textures, solid_texture_page, hud_textures, icon_refs) =
            match (hud.as_deref(), icons.as_deref()) {
                (Some(hud), None) => {
                    let (textures, solid_texture_page, hud_textures) =
                        font_texture_array_with_optional_hud(&font, Some(hud))?;
                    (textures, solid_texture_page, hud_textures, None)
                }
                (None, None) => {
                    let (textures, solid_texture_page) = font_texture_array(&font)?;
                    (textures, solid_texture_page, None, None)
                }
                (hud, icons) => font_texture_array_with_hud_and_icons(&font, hud, icons)?,
            };
        let textures = Arc::new(textures);
        Ok(Self {
            obfuscation: ObfuscationGlyphs::from_catalog(&font),
            base_font: Arc::clone(&font),
            mod_panel_font: None,
            font,
            blank_dynamic_page: textures.pages()[textures.dynamic_start()].clone(),
            textures,
            texture_session: None,
            solid_texture_page,
            hud_textures,
            icon_catalog: icons,
            icon_refs,
            layouts: TextLayoutCache::new(TEXT_CACHE_ENTRIES, TEXT_CACHE_BYTES),
            revision: 0,
            last_input: None,
            last_frame: None,
            #[cfg(test)]
            tree_builds: 0,
            scoreboard: PresentedScoreboardCache::default(),
            scoreboard_owner_names: ScoreboardOwnerNameAuthority::default(),
            debug_lines: None,
            gui_scale_preference: None,
            safe_area: SafeArea::ZERO,
            hud_frame: HudFrame::default(),
            last_hud_diagnostics: Default::default(),
            nametag_anchors: Vec::new(),
            nametag_atlas: nametag_atlas::NametagAtlas::default(),
            paper_doll: Default::default(),
            player_preview_page: None,
            player_preview_source_hash: None,
            player_preview_pose: None,
            player_preview_view: player_preview::PreviewView::default(),
            player_preview_drawn: None,
            player_preview_bob: 0.0,
            player_preview_gear: player_preview::PreviewEquipment::default(),
            equipment_catalog: None,
            gui_models: Default::default(),
            player_preview_pixels: None,
            preview_dirty: false,
            player_preview_icon: None,
            left_hand_icon: None,
            right_hand_icon: None,
            held_viewmodel_source: None,
            offhand_viewmodel_source: None,
            held_viewmodel_icon: None,
            offhand_viewmodel_icon: None,
            menu_artwork_set: Default::default(),
            menu_artwork_loader: Default::default(),
            menu_seconds: 0.0,
            scene_clocks: Default::default(),
            scene_clock: Default::default(),
            menu_artwork: menu_artwork::MenuArtworkAtlas::default(),
            // The title logo loads before any service art arrives.
            menu_artwork_dirty: true,
            session_icons: session_icons::SessionIconPage::default(),
            session_glyphs: session_glyphs::SessionGlyphPages::default(),
            missing_icons: Default::default(),
            logged_hotbar: Default::default(),
            menu_view: None,
            menu_hit_targets: Vec::new(),
            gui_scale_drag_targets: Vec::new(),
            menu_scrolls: Default::default(),
            form_presentation: forms::FormPresentation::default(),
            loading_stage: None,
            startup: StartupPresentationState::default(),
        })
    }

    pub fn set_loading_stage(&mut self, stage: Option<LoadingStage>) {
        self.loading_stage = stage;
    }

    /// Retains original UI skin pixels and compatibility hand rasters. Live model view/bob
    /// changes update geometry without regenerating a thumbnail or reuploading texture pixels.
    pub fn set_player_preview_skin(
        &mut self,
        skin: Option<&[u8]>,
        pose: player_preview::PlayerPreviewPose,
    ) {
        let default_skin = render_model::default_actor_skin_rgba8();
        let skin = skin
            .filter(|pixels| {
                let side = (pixels.len() / 4).isqrt();
                side != 0 && side * side * 4 == pixels.len()
            })
            .unwrap_or(default_skin.as_ref());
        let source_hash = self
            .set_gui_skin(skin)
            .unwrap_or_else(|| Sha256::digest(skin).into());
        let drawn = if self.gui_models.enabled {
            // Model pose and sway now only change geometry. Keep the software hand carriers
            // cached by skin/hand pose; they must not force a small model render/upload each frame.
            (
                player_preview::PreviewView::default(),
                0.0,
                Default::default(),
            )
        } else {
            (
                self.player_preview_view,
                self.player_preview_bob,
                self.player_preview_gear.clone(),
            )
        };
        if self.player_preview_source_hash == Some(source_hash)
            && self.player_preview_pose == Some(pose)
            && self.player_preview_drawn.as_ref() == Some(&drawn)
        {
            return;
        }
        self.player_preview_pixels = Some(player_preview::PlayerPreviewRasters {
            preview: if self.gui_models.enabled {
                // The retained reference supplies the JSON-UI model's virtual coordinate basis.
                // No thumbnail is drawn: apply_gui_models replaces it with destination geometry.
                vec![
                    0;
                    (player_preview::PREVIEW_WIDTH * player_preview::PREVIEW_HEIGHT * 4) as usize
                ]
            } else {
                player_preview::render(skin, pose, drawn.0, drawn.1, &drawn.2)
            },
            left_hand: player_preview::render_hand(skin, pose, true),
            right_hand: player_preview::render_hand(skin, pose, false),
        });
        self.player_preview_drawn = Some(drawn);
        self.player_preview_source_hash = Some(source_hash);
        self.player_preview_pose = Some(pose);
        self.preview_dirty = true;
        self.rebuild_dynamic_textures();
    }

    pub const fn player_preview_icon(&self) -> Option<IconRef> {
        self.player_preview_icon
    }

    pub const fn player_hand_icons(&self) -> (Option<IconRef>, Option<IconRef>) {
        (self.left_hand_icon, self.right_hand_icon)
    }

    pub(super) fn rebuild_dynamic_textures(&mut self) {
        dynamic_textures::rebuild(self);
    }

    /// Service art at `paths`, plus the engine's oversized textures, on the art
    /// pages once the worker has packed them; the last atlas draws meanwhile.
    pub fn sync_menu_artwork(&mut self, paths: Vec<(String, u32)>) {
        let set = menu_artwork::ArtworkSet {
            paths,
            oversized: self.oversized_ui_textures(),
        };
        if !set.same(&self.menu_artwork_set) {
            self.menu_artwork_set = set.clone();
            self.menu_artwork_loader.request(set);
        }
        if self.menu_artwork_loader.poll() {
            self.rebuild_dynamic_textures();
        }
    }

    /// Installs the latest requested art set's complete atlas.
    #[cfg(any(test, feature = "test-support"))]
    pub fn finish_menu_artwork(&mut self) {
        self.menu_artwork_loader.wait();
        self.rebuild_dynamic_textures();
    }

    pub fn menu_artwork_icon(&self, path: &str) -> Option<IconRef> {
        self.menu_artwork.refs.get(path).copied()
    }

    pub fn set_menu_view(&mut self, view: Option<MenuView>) {
        self.menu_view = view;
    }

    pub fn hit_test_menu(&self, position: UiPoint) -> Option<MenuAction> {
        self.menu_hit_targets
            .iter()
            .rev()
            .find_map(|(action, bounds)| bounds.contains(position).then_some(*action))
    }

    fn with_optional_hud(
        font: Arc<RuntimeFontCatalog>,
        hud: Option<Arc<RuntimeHudCatalog>>,
    ) -> Result<Self, UiPresentationError> {
        Self::with_optional_assets(font, hud, None)
    }

    /// Selects a fixed desktop GUI scale; `None` or 0 restores auto.
    pub fn set_gui_scale_preference(&mut self, preference: Option<u8>) {
        self.gui_scale_preference = preference.filter(|value| *value > 0);
    }

    /// Binds the platform's reported safe-area insets (logical px). Every
    /// subsequent frame lays out inside the inset viewport and clips renders
    /// to it; viewports too inset for the fixed HUD fail closed to no HUD.
    pub fn set_safe_area(&mut self, safe_area: SafeArea) {
        self.safe_area = safe_area;
    }

    /// Borrows the captured HUD values for the app's publication adapters.
    pub fn hud_frame(&self) -> &HudFrame {
        &self.hud_frame
    }

    pub fn hud_frame_mut(&mut self) -> &mut HudFrame {
        &mut self.hud_frame
    }

    /// Supplies the world-query adapter's projected nametag anchors for this frame.
    pub fn set_nametag_anchors(&mut self, anchors: Vec<nametags::NametagAnchor>) {
        self.nametag_anchors = anchors;
    }

    /// The world-space tag quads for this frame's anchors.
    pub fn nametag_scene(&mut self) -> render_model::NametagScene {
        let palette = self.formatting_palette().copied().unwrap_or_default();
        self.nametag_atlas.set_palette(palette);
        let (font, glyphs) = (&self.font, &self.session_glyphs);
        let dynamic_start = self.textures.dynamic_start();
        nametags::build_nametag_scene(
            &self.nametag_anchors,
            font,
            &mut self.layouts,
            &mut self.nametag_atlas,
            &|page| {
                nametag_atlas::font_page(font, page).or_else(|| glyphs.page(dynamic_start, page))
            },
        )
    }

    /// Retained text-layout cache entries, exposed for the bounded-memory
    /// steady-state witnesses.
    #[cfg(test)]
    pub fn layout_cache_len(&self) -> usize {
        self.layouts.len()
    }

    /// The Java-look surfaces outside the engine HUD, over the safe HUD geometry.
    #[allow(
        clippy::too_many_arguments,
        reason = "Player authority is borrowed separately from UI state."
    )]
    fn append_java_hud(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next_id: &mut u32,
        geometry: Option<HudGeometry>,
        now_millis: u64,
        container: bool,
    ) -> Result<(), UiPresentationError> {
        let (Some(hud_textures), Some(geometry)) = (self.hud_textures.as_ref(), geometry) else {
            return Ok(());
        };
        let mut frame = self.hud_frame.clone();
        frame.now_millis = now_millis;
        HudLayout::new(
            nodes,
            next_id,
            hud_textures,
            &mut self.layouts,
            &self.font,
            self.solid_texture_page,
            geometry,
        )?
        .append(player_runtime, runtime, &frame, container)
    }

    /// Builds the frame from its retained UI authority.
    pub fn build(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        now_millis: u64,
        physical_size: [u32; 2],
        dpi_scale: DpiScale,
    ) -> Result<UiRenderInput, UiPresentationError> {
        dynamic_textures::observe_session(self, runtime.session_id());
        session_icons::observe(self, runtime.session_icons());
        self.observe_server_ui(runtime.server_ui());
        session_glyphs::observe(self, runtime.session_glyphs());
        // Install artwork before any screen resolves its pixel UVs.
        if self.menu_artwork_loader.poll() {
            self.rebuild_dynamic_textures();
        }
        let logical_width = physical_size[0] as f32 / dpi_scale.get();
        let logical_height = physical_size[1] as f32 / dpi_scale.get();
        let metrics =
            TextMetrics::for_viewport(physical_size, dpi_scale, self.gui_scale_preference);
        // The gameplay HUD lays out in Java GUI pixels; it fails closed to no
        // HUD when the safe viewport cannot contain the fixed-width hotbar.
        let safe_area = self.safe_area;
        let hud_geometry = self.hud_textures.as_ref().and_then(|_| {
            HudGeometry::new(
                physical_size,
                dpi_scale.get(),
                safe_area,
                self.gui_scale_preference,
            )
        });
        let viewport = rect(0.0, 0.0, logical_width, logical_height)?;
        // Root nodes lay out relative to the safe content rect; the retained
        // tree translates them by the safe-area origin.
        let content_width = (logical_width - safe_area.left() - safe_area.right()).max(0.0);
        let content_height = (logical_height - safe_area.top() - safe_area.bottom()).max(0.0);
        let mut nodes = Vec::new();
        let mut next_id = 1u32;
        let content = [content_width, content_height];
        let host = SceneHost {
            menu: self.menu_view.as_ref().map(|view| view.screen),
            over_world: self.menu_view.as_ref().is_none_or(|view| view.over_world),
            loading: self.loading_stage.is_some(),
        };
        let stack = runtime.scenes_in(player_runtime, host, &self.screen_settings());
        let scenes = stack.visible(false);
        self.begin_form_frame();
        self.menu_seconds = now_millis as f64 / 1_000.0;
        let open: Vec<Scene> = stack.scenes().iter().map(|scene| scene.key).collect();
        self.scene_clocks.observe(&open, self.menu_seconds);
        let mut menu_hit_targets = Vec::new();
        for scene in &scenes {
            self.scene_clock = self.scene_clocks.clocks(*scene);
            let (nodes, next) = (&mut nodes, &mut next_id);
            match scene {
                Scene::Gameplay => {
                    self.append_java_hud(
                        player_runtime,
                        runtime,
                        nodes,
                        next,
                        hud_geometry,
                        now_millis,
                        false,
                    )?;
                }
                Scene::Crosshair | Scene::Hud => {
                    let crosshair = *scene == Scene::Crosshair;
                    self.append_engine_hud(
                        player_runtime,
                        runtime,
                        nodes,
                        next,
                        metrics,
                        content,
                        now_millis,
                        crosshair,
                    )?;
                    if !crosshair {
                        self.append_mod_hud(player_runtime, runtime, nodes, next, metrics, content);
                        self.append_player_list(
                            player_runtime,
                            runtime,
                            nodes,
                            next,
                            metrics,
                            content,
                        )?;
                    }
                }
                Scene::Bed => {
                    self.append_bed_screen(runtime, nodes, next, metrics, content, now_millis)?;
                }
                Scene::Container => {
                    self.append_java_hud(
                        player_runtime,
                        runtime,
                        nodes,
                        next,
                        hud_geometry,
                        now_millis,
                        true,
                    )?;
                    self.append_container_scene(
                        player_runtime,
                        runtime,
                        nodes,
                        next,
                        metrics,
                        content_width,
                        content_height,
                    )?;
                }
                Scene::Chat => {
                    self.append_chat_screen(runtime, nodes, next, metrics, content, now_millis)?;
                }
                Scene::Emote => {
                    self.append_emote_screen(runtime, nodes, next, metrics, content, now_millis)?;
                }
                Scene::Loading => {
                    if let Some(stage) = self.loading_stage {
                        // An opaque cover under the loading screen: no partial terrain or
                        // HUD leaks through while the world settles.
                        nodes.push(
                            UiNode::new(UiNodeId::new(*next), None, viewport).with_visual(
                                UiVisual::Solid {
                                    texture_page: self.solid_texture_page,
                                    color: [8, 10, 14, 255],
                                },
                            ),
                        );
                        *next = next.saturating_add(1);
                        self.append_loading_screen(runtime, stage, nodes, next, metrics, content)?;
                    }
                }
                Scene::SignEditor => {
                    self.append_sign_editor(
                        runtime,
                        nodes,
                        next,
                        metrics,
                        content_width,
                        content_height,
                        now_millis,
                    )?;
                }
                Scene::ServerForm | Scene::ServerSettingsForm => {
                    self.append_server_form(
                        runtime,
                        nodes,
                        next,
                        metrics,
                        content_width,
                        content_height,
                    )?;
                }
                Scene::Menu(_) => {
                    menu_hit_targets = self.append_menu(
                        runtime,
                        nodes,
                        next,
                        metrics,
                        content_width,
                        content_height,
                    )?;
                }
            }
        }
        if !scenes.contains(&Scene::Chat) {
            self.close_chat_screen();
        }
        if !scenes.contains(&Scene::Emote) {
            self.close_emote_screen();
        }
        if !scenes.contains(&Scene::SignEditor) {
            self.hide_sign_editor();
        }
        // Toasts live on their own stack, drawn last over every scene.
        self.scene_clock.clear();
        self.append_toast_screen(
            runtime,
            &mut nodes,
            &mut next_id,
            metrics,
            content,
            now_millis,
        )?;
        self.sync_server_ui_pages();
        self.append_experience_chrome(
            runtime,
            &mut nodes,
            &mut next_id,
            metrics,
            [content_width, content_height],
        );
        self.append_mod_panel(runtime, &mut nodes, &mut next_id, metrics, content);
        if scenes.contains(&Scene::Gameplay)
            && stack
                .scenes()
                .iter()
                .all(|scene| matches!(scene.key, Scene::Gameplay | Scene::Crosshair | Scene::Hud))
        {
            self.append_debug_overlay(&mut nodes, &mut next_id, metrics, content)?;
        }
        // Every screen has painted: retire animation state nothing touched.
        self.end_animation_frame();
        self.apply_gui_models(&mut nodes);
        // Unchanged nodes build the same frame unless §k text re-rolls its glyphs, so tree,
        // layout and draw-list construction are skipped.
        let frame = (physical_size, dpi_scale.get(), safe_area);
        if let (Some(last), Some(input)) = (&self.last_frame, &self.last_input)
            && last.same(frame, &self.textures, &nodes)
        {
            self.menu_hit_targets = menu_hit_targets;
            return Ok(input.clone());
        }
        self.last_frame = (!obfuscated(&nodes)).then(|| BuiltFrame {
            nodes: nodes.clone(),
            frame,
            textures: Arc::clone(&self.textures),
        });
        #[cfg(test)]
        {
            self.tree_builds += 1;
        }
        let mut tree = UiTree::new(nodes).map_err(UiPresentationError::Tree)?;
        tree.layout(viewport, UiScale::default(), safe_area)
            .map_err(UiPresentationError::Tree)?;
        let draw_list = tree
            .build_draw_list_with(TextEffects {
                palette: self.formatting_palette(),
                obfuscation_seed: now_millis,
                obfuscation: Some(&self.obfuscation),
            })
            .map_err(UiPresentationError::Tree)?;
        let input = adapt_ui_draw_list(
            &draw_list,
            Arc::clone(&self.textures),
            UiRenderViewport {
                physical_size,
                dpi_scale,
                safe_area,
            },
        )
        .map_err(UiPresentationError::Adapter)?;
        let input = self.stabilize_revision(input);
        self.menu_hit_targets = menu_hit_targets;
        Ok(input)
    }
}

/// A frame's inputs: its nodes, viewport and texture array.
struct BuiltFrame {
    nodes: Vec<UiNode>,
    frame: ([u32; 2], f32, SafeArea),
    textures: Arc<UiRenderTextureArray>,
}

impl BuiltFrame {
    fn same(
        &self,
        frame: ([u32; 2], f32, SafeArea),
        textures: &Arc<UiRenderTextureArray>,
        nodes: &[UiNode],
    ) -> bool {
        self.frame == frame && Arc::ptr_eq(&self.textures, textures) && self.nodes == nodes
    }
}

/// Whether any text carries `§k`, whose glyphs change every frame.
fn obfuscated(nodes: &[UiNode]) -> bool {
    nodes.iter().any(|node| match node.visual() {
        UiVisual::Text { layout, .. } | UiVisual::RotatedText { layout, .. } => {
            layout.glyphs().iter().any(|glyph| glyph.style.obfuscated)
        }
        _ => false,
    })
}

#[cfg(test)]
pub mod tests;

impl UiPresentationRuntime {
    /// Borrows loading readiness for the app's existing ordered preparation stage.
    pub fn startup_mut(&mut self) -> &mut startup::StartupPresentationState {
        &mut self.startup
    }
    /// Borrows loading readiness without advancing it.
    pub fn startup(&self) -> &startup::StartupPresentationState {
        &self.startup
    }
    /// Reports the currently presented loading stage to the app's hand adapter.
    pub fn loading_stage(&self) -> Option<LoadingStage> {
        self.loading_stage
    }
    /// Advances paper-doll presentation from the app's synchronous movement observation.
    pub fn observe_paper_doll(&mut self, now: u64, state: Option<paper_doll::State>) {
        self.hud_frame.paper_doll_visible = self.paper_doll.update(now, state);
    }
}

impl UiPresentationRuntime {
    /// The effective GUI preference consumed by the app scale adapter.
    pub fn gui_scale_preference(&self) -> Option<u8> {
        self.gui_scale_preference
    }
}
