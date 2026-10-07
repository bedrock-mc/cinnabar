//! Initial font, art and retained presentation storage.

use super::*;

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

    /// Installs the font and optional HUD/icon pages in one texture array.
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
            fallback_font: None,
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
            assembly_nodes: Vec::new(),
            #[cfg(test)]
            tree_builds: 0,
            scoreboard: PresentedScoreboardCache::default(),
            scoreboard_owner_names: ScoreboardOwnerNameAuthority::default(),
            debug_lines: None,
            debug_overlay: debug_overlay::OverlayCache::default(),
            gui_scale_preference: None,
            safe_area: SafeArea::ZERO,
            hud_frame: HudFrame::default(),
            last_hud_diagnostics: Default::default(),
            nametag_anchors: Vec::new(),
            nametag_atlas: nametag_atlas::NametagAtlas::default(),
            primitive_text: primitive_shapes::PrimitiveTextRasterizer::default(),
            paper_doll: Default::default(),
            player_preview_page: None,
            player_preview_source_hash: None,
            player_preview_pose: None,
            player_preview_view: player_preview::PreviewView::default(),
            menu_preview_model: player_preview::model::MenuPreviewModel::default(),
            menu_preview: player_preview::controller::MenuPreview::default(),
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
            menu_skin_thumbnail_indices: Vec::new(),
            menu_cape_thumbnail_indices: Vec::new(),
            settings_slider_drag_targets: Vec::new(),
            menu_scrolls: Default::default(),
            form_presentation: forms::FormPresentation::default(),
            loading_stage: None,
            startup: StartupPresentationState::default(),
        })
    }

    /// Creates a presentation without an icon catalog.
    fn with_optional_hud(
        font: Arc<RuntimeFontCatalog>,
        hud: Option<Arc<RuntimeHudCatalog>>,
    ) -> Result<Self, UiPresentationError> {
        Self::with_optional_assets(font, hud, None)
    }
}
