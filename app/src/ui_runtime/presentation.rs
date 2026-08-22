use std::{fmt, sync::Arc};

use assets::{RuntimeFontCatalog, RuntimeHudCatalog, RuntimeIconCatalog};
use bevy::{
    camera::Camera,
    math::Vec3,
    prelude::{Camera3d, GlobalTransform, Query, Res, ResMut, Resource, Time, With},
    time::Real,
    window::{PrimaryWindow, Window},
};
use render::{
    ActorSkinPixels, ChunkRenderQueue, ChunkUploadAcknowledgements, VisibilityDiagnostics,
    VisibilityDiagnosticsInput, normalize_actor_skin,
};
use render::{UiRenderInput, UiRenderScene, UiRenderStats, UiRenderTextureArray};
use sha2::{Digest, Sha256};

use ui::{
    DpiScale, HudViewRole, SafeArea, TextLayoutCache, TextLayoutRequest, TextShadow, TextStyle,
    UiNode, UiNodeId, UiPoint, UiRect, UiScale, UiTree, UiVisual,
};

use super::{UiRuntime, render_adapter::UiRenderViewport};
use crate::{
    camera::CameraSettingsAuthority,
    runtime::{
        shutdown::record_fatal_error,
        visibility::CaveVisibilityCache,
        world::{ClientWorld, WorldStreamFramePoll},
    },
    ui_runtime::{item_facts, render_adapter::adapt_ui_draw_list},
};

mod chat;
mod dynamic_textures;
mod hud_layout;
pub(crate) mod inventory_pointer;
mod item_viewmodel;
mod menu;
mod menu_artwork;
mod player_preview;
mod primitives;
mod publish;
mod retained_hud;
mod startup;
mod texture_atlas;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "dormant until presentation owns an exact walk-distance query cadence"
    )
)]
mod viewmodel_bob;

use crate::menu::{MenuAction, MenuView};
use chat::visible_suggestion_range;
pub(crate) use hud_layout::HudFrame;
use hud_layout::{HudGeometry, HudLayout, java_gui_scale};
use primitives::{bounded_visible_text, hud_position, rect};
#[cfg(test)]
pub(crate) use publish::refresh_hud_frame;
pub(crate) use publish::{observe_mount_jump_input, platform_safe_area_insets, publish_ui_runtime};
use retained_hud::{
    BelowNameAnchor, PresentedScoreboardCache, ScoreboardOpacityAuthority,
    ScoreboardOwnerNameAuthority,
};
use startup::{StartupPresentationState, StartupReadinessInput};
pub(crate) use texture_atlas::IconRef;
use texture_atlas::{
    HudSprite, HudTexturePages, font_texture_array, font_texture_array_with_hud_and_icons,
    font_texture_array_with_optional_hud,
};

const TEXT_CACHE_ENTRIES: usize = 1_024;
const TEXT_CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_PRESENTED_CHAT_ROWS: usize = 8;
const MAX_PRESENTED_CHAT_SUGGESTIONS: usize = 8;
const MAX_PRESENTED_TOAST_ROWS: usize = 8;
const MAX_PRESENTED_TEXT_BYTES: usize = 512;
// Java-style chat presentation (Hybrid HUD): unfocused chat lines get an always-on translucent
// black backdrop, matching Java Edition's per-line chat background (drawn at textBackgroundOpacity,
// default 0.5 -> byte alpha 128). Recorded as a Hybrid deviation in plan.md.
const CHAT_LINE_BACKDROP_COLOR: [u8; 4] = [0, 0, 0, 128];
const CHAT_LINE_BACKDROP_PAD: f32 = 2.0;
// Java's default chat text begins four GUI pixels from the safe content edge.
// Keep the text anchor independent of the bottom HUD width so chat remains a
// true left-edge surface on ultrawide and resized windows.
const CHAT_LEFT_INSET: f32 = 4.0;
const CHAT_PANEL_PAD: f32 = 4.0;
// Java chat fade: rows show for 200 ticks then fade over the final 20
// (10 s + 1 s), pinned here in milliseconds.
const CHAT_VISIBLE_MILLIS: u64 = 10_000;
const CHAT_FADE_MILLIS: u64 = 1_000;
// The compiled Monocraft atlas is rasterized at 18 px/em (see
// `assets/ui-font-source.json`). Monocraft draws on a 60-font-unit grid against
// a 1080-unit em, so one design pixel is two texels: ASCII ink is 16 texels
// tall, 14 of them above the baseline, and the widest advance is 12. That makes
// `UiScale` 1 already equal to Mojang's GUI scale 2, and only whole numbers of
// physical pixels per texel keep every design pixel on a pixel boundary.
const FONT_DESIGN_PIXEL_TEXELS: u32 = 2;
const FONT_ASCENT_TEXELS: u32 = 14;
const FONT_INK_TEXELS: u32 = 16;
/// Mojang pitches chat one design pixel below the font's ink height -- 9 px for
/// an 8 px font. The same ratio against Monocraft's 16 texels gives 18.
const TEXT_LINE_HEIGHT_64: u32 = (FONT_INK_TEXELS + FONT_DESIGN_PIXEL_TEXELS) * 64;
/// Distance from the top of a line box down to the baseline, so glyphs sit
/// inside the box instead of hanging above its origin.
const TEXT_BASELINE_64: u32 = FONT_ASCENT_TEXELS * 64;
/// Mojang offsets the shadow by exactly one design pixel on both axes.
const TEXT_SHADOW_OFFSET_64: u32 = FONT_DESIGN_PIXEL_TEXELS * 64;
/// Per-frame text metrics shared by every HUD, chat, and scoreboard run so a
/// single frame cannot mix scales or line pitches. Font atlas texels are two
/// texels per Java GUI design pixel, while sprite geometry uses one GUI pixel.
#[derive(Clone, Copy)]
pub(super) struct TextMetrics {
    scale: UiScale,
    line_height_64: u32,
    baseline_64: u32,
    shadow: TextShadow,
}

impl TextMetrics {
    /// Uses the same Java GUI-scale choice as sprite geometry. The font atlas
    /// is authored at two texels per GUI design pixel, so its logical scale is
    /// half the sprite scale before the platform DPI is removed.
    fn for_viewport(physical_size: [u32; 2], dpi_scale: DpiScale, preference: Option<u8>) -> Self {
        let dpi = dpi_scale.get();
        let gui_scale = java_gui_scale(physical_size, preference) as f32;
        let scale =
            (gui_scale / (FONT_DESIGN_PIXEL_TEXELS as f32 * dpi)).clamp(UiScale::MIN, UiScale::MAX);
        Self {
            scale: UiScale::new(scale).expect("the clamped scale is inside the UiScale range"),
            line_height_64: TEXT_LINE_HEIGHT_64,
            baseline_64: TEXT_BASELINE_64,
            shadow: TextShadow::Offset64(TEXT_SHADOW_OFFSET_64),
        }
    }

    fn request<'a>(
        &self,
        text: &'a str,
        width_64: u32,
        font: &'a RuntimeFontCatalog,
    ) -> TextLayoutRequest<'a> {
        TextLayoutRequest {
            text,
            style: TextStyle::default(),
            width_64,
            line_height_64: self.line_height_64,
            baseline_64: self.baseline_64,
            scale: self.scale,
            font,
        }
    }

    pub(super) const fn shadow(&self) -> TextShadow {
        self.shadow
    }
}

#[derive(Debug)]
pub enum UiPresentationError {
    InvalidFontTexture,
    Geometry(ui::GeometryError),
    Text(ui::TextError),
    Tree(ui::UiError),
    Adapter(super::render_adapter::UiRenderAdapterError),
    Render(render::UiRenderReject),
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
    base_textures: Arc<UiRenderTextureArray>,
    textures: Arc<UiRenderTextureArray>,
    solid_texture_page: u16,
    hud_textures: Option<HudTexturePages>,
    icon_catalog: Option<Arc<RuntimeIconCatalog>>,
    icon_refs: Option<Box<[IconRef]>>,
    layouts: TextLayoutCache,
    revision: u64,
    scoreboard: PresentedScoreboardCache,
    scoreboard_owner_names: ScoreboardOwnerNameAuthority,
    scoreboard_opacity: Option<ScoreboardOpacityAuthority>,
    chat_hit_logical_size: Option<[f32; 2]>,
    chat_suggestion_hits: Vec<(usize, UiRect)>,
    /// Java GUI-scale preference: `None`/0 selects the auto rule.
    gui_scale_preference: Option<u8>,
    /// Platform safe-area insets in logical px, applied to the HUD geometry,
    /// the retained tree layout, and the render viewport alike.
    safe_area: SafeArea,
    /// Item facts and camera state refreshed immediately before each build.
    hud_frame: HudFrame,
    /// Last logged skip/odd-data counters, so changes surface exactly once.
    last_hud_diagnostics: crate::ui_runtime::gameplay_hud::GameplayHudDiagnostics,
    /// World-projected below-name score anchors for the current frame.
    below_name_anchors: Vec<BelowNameAnchor>,
    /// Identity of the static font/HUD/item carrier before the optional
    /// cached player-preview layer is appended.
    base_texture_identity: [u8; 32],
    player_preview_page: Option<u16>,
    player_preview_source_hash: Option<[u8; 32]>,
    player_preview_pose: Option<player_preview::PlayerPreviewPose>,
    player_preview_pixels: Option<player_preview::PlayerPreviewRasters>,
    player_preview_icon: Option<IconRef>,
    left_hand_icon: Option<IconRef>,
    right_hand_icon: Option<IconRef>,
    held_viewmodel_source: Option<IconRef>,
    offhand_viewmodel_source: Option<IconRef>,
    held_viewmodel_icon: Option<IconRef>,
    offhand_viewmodel_icon: Option<IconRef>,
    menu_artwork_paths: Vec<String>,
    menu_artwork: menu_artwork::MenuArtworkAtlas,
    menu_view: Option<MenuView>,
    menu_hit_targets: Vec<(MenuAction, UiRect)>,
    loading_message: Option<&'static str>,
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
        let base_texture_identity = textures.identity;
        let textures = Arc::new(textures);
        Ok(Self {
            font,
            base_textures: Arc::clone(&textures),
            textures,
            solid_texture_page,
            hud_textures,
            icon_catalog: icons,
            icon_refs,
            layouts: TextLayoutCache::new(TEXT_CACHE_ENTRIES, TEXT_CACHE_BYTES),
            revision: 0,
            scoreboard: PresentedScoreboardCache::default(),
            scoreboard_owner_names: ScoreboardOwnerNameAuthority::default(),
            scoreboard_opacity: None,
            chat_hit_logical_size: None,
            chat_suggestion_hits: Vec::with_capacity(MAX_PRESENTED_CHAT_SUGGESTIONS),
            gui_scale_preference: None,
            safe_area: SafeArea::ZERO,
            hud_frame: HudFrame::default(),
            last_hud_diagnostics: Default::default(),
            below_name_anchors: Vec::new(),
            base_texture_identity,
            player_preview_page: None,
            player_preview_source_hash: None,
            player_preview_pose: None,
            player_preview_pixels: None,
            player_preview_icon: None,
            left_hand_icon: None,
            right_hand_icon: None,
            held_viewmodel_source: None,
            offhand_viewmodel_source: None,
            held_viewmodel_icon: None,
            offhand_viewmodel_icon: None,
            menu_artwork_paths: Vec::new(),
            menu_artwork: menu_artwork::MenuArtworkAtlas::default(),
            menu_view: None,
            menu_hit_targets: Vec::new(),
            loading_message: None,
            startup: StartupPresentationState::default(),
        })
    }

    pub(crate) fn set_loading_message(&mut self, message: Option<&'static str>) {
        self.loading_message = message;
    }

    /// Resolves an authoritative item identity to the packed icon atlas.
    /// Unknown/custom items fail closed and keep the hotbar frame and server
    /// state intact.
    pub(crate) fn item_icon(&self, identifier: &str, metadata: u32) -> Option<IconRef> {
        let sprite = self
            .icon_catalog
            .as_ref()?
            .lookup_index(identifier, metadata)?;
        self.icon_refs.as_deref()?.get(sprite).copied()
    }

    /// Updates the cached corner avatar. The raster is regenerated and the UI
    /// texture array is replaced only when the authoritative skin or pose
    /// changes; normal camera/HUD frames reuse the same GPU texture.
    pub(crate) fn set_player_preview_skin(
        &mut self,
        skin: Option<&[u8]>,
        pose: player_preview::PlayerPreviewPose,
    ) {
        let default_skin = render::default_actor_skin_rgba8();
        let skin = skin
            .filter(|pixels| pixels.len() == render::STANDARD_SKIN_BYTES)
            .unwrap_or(default_skin.as_ref());
        let source_hash: [u8; 32] = Sha256::digest(skin).into();
        if self.player_preview_source_hash == Some(source_hash)
            && self.player_preview_pose == Some(pose)
        {
            return;
        }
        self.player_preview_pixels = Some(player_preview::PlayerPreviewRasters {
            preview: player_preview::render(skin, pose),
            left_hand: player_preview::render_hand(skin, pose, true),
            right_hand: player_preview::render_hand(skin, pose, false),
        });
        self.player_preview_source_hash = Some(source_hash);
        self.player_preview_pose = Some(pose);
        self.rebuild_dynamic_textures();
    }

    pub(crate) const fn player_preview_icon(&self) -> Option<IconRef> {
        self.player_preview_icon
    }

    pub(crate) const fn player_hand_icons(&self) -> (Option<IconRef>, Option<IconRef>) {
        (self.left_hand_icon, self.right_hand_icon)
    }

    pub(super) fn rebuild_dynamic_textures(&mut self) {
        dynamic_textures::rebuild(self);
    }

    pub(crate) fn sync_menu_artwork(&mut self, paths: Vec<String>) {
        if self.menu_artwork_paths == paths {
            return;
        }
        self.menu_artwork_paths = paths;
        self.rebuild_dynamic_textures();
    }

    pub(crate) fn menu_artwork_icon(&self, path: &str) -> Option<IconRef> {
        self.menu_artwork.refs.get(path).copied()
    }

    pub(crate) fn set_menu_view(&mut self, view: Option<MenuView>) {
        self.menu_view = view;
    }

    pub(crate) fn hit_test_menu(&self, position: UiPoint) -> Option<MenuAction> {
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

    /// Selects a fixed Java GUI scale (1..=4); `None` or 0 restores auto.
    pub fn set_gui_scale_preference(&mut self, preference: Option<u8>) {
        self.gui_scale_preference = preference.filter(|value| *value > 0);
    }

    /// Binds the platform's reported safe-area insets (logical px). Every
    /// subsequent frame lays out inside the inset viewport and clips renders
    /// to it; viewports too inset for the fixed HUD fail closed to no HUD.
    pub fn set_safe_area(&mut self, safe_area: SafeArea) {
        self.safe_area = safe_area;
    }

    #[cfg(test)]
    pub(crate) fn hud_frame(&self) -> &HudFrame {
        &self.hud_frame
    }

    pub(crate) fn hud_frame_mut(&mut self) -> &mut HudFrame {
        &mut self.hud_frame
    }

    fn set_below_name_anchors(&mut self, anchors: impl IntoIterator<Item = BelowNameAnchor>) {
        self.below_name_anchors.clear();
        self.below_name_anchors.extend(
            anchors
                .into_iter()
                .take(retained_hud::MAX_PRESENTED_BELOW_NAME_ROWS),
        );
    }

    /// Retained text-layout cache entries, exposed for the bounded-memory
    /// steady-state witnesses.
    #[cfg(test)]
    pub(crate) fn layout_cache_len(&self) -> usize {
        self.layouts.len()
    }

    pub fn build(
        &mut self,
        runtime: &UiRuntime,
        now_millis: u64,
        physical_size: [u32; 2],
        dpi_scale: DpiScale,
    ) -> Result<UiRenderInput, UiPresentationError> {
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
        let wrap_width = ((content_width * 0.45).clamp(1.0, 640.0) * 64.0) as u32;
        let chat_content_width = wrap_width as f32 / 64.0;
        let chat_left = CHAT_LEFT_INSET.min(content_width);
        let chat_right = (chat_left + chat_content_width)
            .min(content_width)
            .max(chat_left);
        let mut nodes = Vec::new();
        let mut next_id = 1u32;
        let menu_visible = self.menu_view.is_some();

        if !menu_visible
            && let Some(hud_textures) = self.hud_textures.as_ref()
            && let Some(geometry) = hud_geometry
        {
            let mut frame = self.hud_frame.clone();
            frame.now_millis = now_millis;
            let mut layout = HudLayout::new(
                &mut nodes,
                &mut next_id,
                hud_textures,
                &mut self.layouts,
                &self.font,
                self.solid_texture_page,
                geometry,
            )?;
            layout.append(runtime, &frame)?;
        }

        let inventory_open = runtime.inventory_open();
        let hud_nodes = runtime.hud().view_nodes(now_millis);
        let mut toast_rows = 0usize;
        for node in hud_nodes.iter() {
            if inventory_open || menu_visible {
                break;
            }
            if matches!(
                node.role,
                HudViewRole::Health | HudViewRole::Hunger | HudViewRole::Armor | HudViewRole::Air
            ) {
                continue;
            }
            if matches!(
                node.role,
                HudViewRole::ToastTitle | HudViewRole::ToastMessage
            ) {
                if toast_rows >= MAX_PRESENTED_TOAST_ROWS {
                    continue;
                }
                toast_rows += 1;
            }
            let text = bounded_visible_text(&node.text);
            let layout = self
                .layouts
                .layout(metrics.request(text, wrap_width, &self.font))
                .map_err(UiPresentationError::Text)?;
            let [x, y] = hud_position(node.role, nodes.len(), content_width, content_height);
            nodes.push(
                UiNode::new(
                    UiNodeId::new(next_id),
                    None,
                    rect(
                        x,
                        y,
                        (x + content_width * 0.45).min(content_width),
                        content_height,
                    )?,
                )
                .with_visual(UiVisual::Text {
                    layout,
                    color: [255; 4],
                    shadow: metrics.shadow(),
                }),
            );
            next_id = next_id.saturating_add(1);
        }

        if !inventory_open
            && !menu_visible
            && let Some(opacity) = self.scoreboard_opacity
            && let Some(scoreboard) = self
                .scoreboard
                .refresh(runtime.scoreboards(), &self.scoreboard_owner_names)
        {
            retained_hud::append_scoreboard_nodes(
                &mut nodes,
                &mut next_id,
                &mut self.layouts,
                &self.font,
                metrics,
                self.solid_texture_page,
                content_width,
                content_height,
                scoreboard,
                opacity,
            )?;
        }

        // The tab player-list overlay presents every known player with the
        // list-objective score while the player-list action is held.
        if !inventory_open && !menu_visible && self.hud_frame.tab_list_open {
            let players = runtime.player_list_overlay_rows();
            retained_hud::append_player_list_nodes(
                &mut nodes,
                &mut next_id,
                &mut self.layouts,
                &self.font,
                metrics,
                self.solid_texture_page,
                content_width,
                content_height,
                &players,
            )?;
        }

        if !inventory_open && !menu_visible {
            retained_hud::append_below_name_nodes(
                &mut nodes,
                &mut next_id,
                &mut self.layouts,
                &self.font,
                metrics,
                self.solid_texture_page,
                content_width,
                content_height,
                &self.below_name_anchors,
            )?;
        }

        let chat_focused = !menu_visible && !inventory_open && runtime.chat_focused();
        let visible_suggestions = if chat_focused {
            visible_suggestion_range(
                runtime.chat_suggestions().len(),
                runtime.chat_selected_suggestion(),
            )
        } else {
            0..0
        };
        let mut editor_layout = None;
        let mut suggestion_layouts = Vec::new();
        if chat_focused {
            let editor = runtime.chat_editor();
            let mut visible = String::with_capacity(editor.len_bytes().saturating_add(1));
            visible.push_str(&editor.as_str()[..editor.cursor_byte()]);
            visible.push('|');
            visible.push_str(&editor.as_str()[editor.cursor_byte()..]);
            editor_layout = Some(
                self.layouts
                    .layout(metrics.request(bounded_visible_text(&visible), wrap_width, &self.font))
                    .map_err(UiPresentationError::Text)?,
            );

            for (index, suggestion) in runtime
                .chat_suggestions()
                .iter()
                .enumerate()
                .skip(visible_suggestions.start)
                .take(visible_suggestions.len())
            {
                let selected = runtime.chat_selected_suggestion() == Some(index);
                let layout = self
                    .layouts
                    .layout(metrics.request(
                        bounded_visible_text(suggestion),
                        wrap_width,
                        &self.font,
                    ))
                    .map_err(UiPresentationError::Text)?;
                suggestion_layouts.push((index, layout, [220, 220, 220, 255], selected));
            }
        }

        let suggestion_reserved_height = suggestion_layouts
            .iter()
            .map(|(_, layout, _, _)| layout.size_64()[1] as f32 / 64.0 + 2.0)
            .sum::<f32>();
        let chat_region_top = (content_height - 220.0 - suggestion_reserved_height).max(0.0);
        let bottom_hud_top = hud_geometry.map_or_else(
            || (content_height - 42.0).max(chat_region_top),
            |geometry| geometry.bottom_row_top_logical().max(chat_region_top),
        );
        let editor_bottom = (bottom_hud_top - 2.0).max(chat_region_top);
        let editor_y = editor_layout.as_ref().map_or(editor_bottom, |layout| {
            (editor_bottom - layout.size_64()[1] as f32 / 64.0).max(chat_region_top)
        });
        let mut suggestion_cursor = (editor_y - 4.0).max(chat_region_top);
        let mut positioned_suggestions = Vec::new();
        for (index, layout, color, selected) in suggestion_layouts {
            let layout_height = layout.size_64()[1] as f32 / 64.0;
            if layout_height > suggestion_cursor - chat_region_top {
                break;
            }
            let y = suggestion_cursor - layout_height;
            positioned_suggestions.push((index, layout, y, suggestion_cursor, color, selected));
            suggestion_cursor = (y - 2.0).max(chat_region_top);
        }
        let chat = runtime.chat().messages();
        let first = if inventory_open || menu_visible {
            chat.len()
        } else {
            chat.len().saturating_sub(MAX_PRESENTED_CHAT_ROWS)
        };
        let chat_bottom = if chat_focused {
            suggestion_cursor
        } else {
            (content_height - 72.0).max(chat_region_top)
        };
        let mut chat_cursor = chat_bottom;
        let mut visible_chat = Vec::new();
        for node in chat.iter().skip(first).rev() {
            // Java chat fade: an unfocused row shows for ten seconds, then
            // fades over one second (200 + 20 ticks in the reference). Rows
            // stamped ahead of the local clock stay fresh rather than hiding.
            let alpha = if chat_focused {
                255u8
            } else {
                let age = now_millis.saturating_sub(node.received_millis);
                if age <= CHAT_VISIBLE_MILLIS {
                    255
                } else if age >= CHAT_VISIBLE_MILLIS + CHAT_FADE_MILLIS {
                    continue;
                } else {
                    let remaining = (CHAT_VISIBLE_MILLIS + CHAT_FADE_MILLIS - age) as f32;
                    (255.0 * remaining / CHAT_FADE_MILLIS as f32) as u8
                }
            };
            if alpha == 0 {
                continue;
            }
            let text = bounded_visible_text(&node.message);
            let layout = self
                .layouts
                .layout(metrics.request(text, wrap_width, &self.font))
                .map_err(UiPresentationError::Text)?;
            let layout_height = layout.size_64()[1] as f32 / 64.0;
            if layout_height > chat_cursor - chat_region_top {
                if visible_chat.is_empty() {
                    let available_height = chat_cursor - chat_region_top;
                    let boundaries = text
                        .char_indices()
                        .map(|(index, _)| index)
                        .skip(1)
                        .chain(std::iter::once(text.len()))
                        .collect::<Vec<_>>();
                    let mut low = 0usize;
                    let mut high = boundaries.len();
                    let mut best = None;
                    while low < high {
                        let middle = low + (high - low) / 2;
                        let candidate = self
                            .layouts
                            .layout(metrics.request(
                                &text[..boundaries[middle]],
                                wrap_width,
                                &self.font,
                            ))
                            .map_err(UiPresentationError::Text)?;
                        let candidate_height = candidate.size_64()[1] as f32 / 64.0;
                        if candidate_height <= available_height {
                            best = Some((candidate, candidate_height));
                            low = middle.saturating_add(1);
                        } else {
                            high = middle;
                        }
                    }
                    if let Some((layout, height)) = best {
                        visible_chat.push((layout, chat_cursor - height, chat_cursor, alpha));
                    }
                }
                break;
            }
            let y = chat_cursor - layout_height;
            visible_chat.push((layout, y, chat_cursor, alpha));
            // No extra gap: the line pitch already carries the one design pixel
            // Mojang leaves between chat rows, so adding more double-spaces them.
            chat_cursor = y.max(chat_region_top);
        }
        if chat_focused {
            let panel_left = (chat_left - CHAT_PANEL_PAD).max(0.0).min(logical_width);
            let panel_right = (chat_right + CHAT_PANEL_PAD)
                .min(logical_width)
                .max(panel_left);
            let panel_top = (editor_y - 2.0).max(chat_region_top);
            let panel_bottom = (editor_bottom + 2.0).min(bottom_hud_top);
            nodes.push(
                UiNode::new(
                    UiNodeId::new(next_id),
                    None,
                    rect(panel_left, panel_top, panel_right, panel_bottom)?,
                )
                .with_visual(UiVisual::Solid {
                    texture_page: self.solid_texture_page,
                    color: [0, 0, 0, 176],
                }),
            );
            next_id = next_id.saturating_add(1);
        }
        // Java-style unfocused chat: each line carries its own translucent
        // backdrop so the row's fade dims the background with the text. The
        // rects extend across the inter-line spacing to the row above, keeping
        // the block visually contiguous like the reference. When focused, the
        // unified chat panel above already provides the background. Backdrops
        // precede the text nodes so they render underneath.
        if !chat_focused && !visible_chat.is_empty() {
            let backdrop_left = (chat_left - CHAT_LINE_BACKDROP_PAD).max(0.0);
            for (index, (_, top, bottom, alpha)) in visible_chat.iter().enumerate() {
                // The next entry (pushed after this one) sits above; stretch
                // this row's backdrop up to it so no stripe shows through.
                let covered_top = visible_chat
                    .get(index + 1)
                    .map_or(*top, |(_, _, above_bottom, _)| top.min(*above_bottom));
                let backdrop_alpha =
                    (u16::from(CHAT_LINE_BACKDROP_COLOR[3]) * u16::from(*alpha) / 255) as u8;
                nodes.push(
                    UiNode::new(
                        UiNodeId::new(next_id),
                        None,
                        rect(backdrop_left, covered_top, chat_right, *bottom)?,
                    )
                    .with_visual(UiVisual::Solid {
                        texture_page: self.solid_texture_page,
                        color: [
                            CHAT_LINE_BACKDROP_COLOR[0],
                            CHAT_LINE_BACKDROP_COLOR[1],
                            CHAT_LINE_BACKDROP_COLOR[2],
                            backdrop_alpha,
                        ],
                    }),
                );
                next_id = next_id.saturating_add(1);
            }
        }
        for (layout, y, bottom, alpha) in visible_chat.into_iter().rev() {
            nodes.push(
                UiNode::new(
                    UiNodeId::new(next_id),
                    None,
                    rect(chat_left, y, chat_right, bottom)?,
                )
                .with_visual(UiVisual::Text {
                    layout,
                    color: [255, 255, 255, alpha],
                    shadow: metrics.shadow(),
                }),
            );
            next_id = next_id.saturating_add(1);
        }

        if chat_focused {
            let layout = editor_layout.expect("focused chat prepared an editor layout");
            nodes.push(
                UiNode::new(
                    UiNodeId::new(next_id),
                    None,
                    rect(chat_left, editor_y, chat_right, editor_bottom)?,
                )
                .with_visual(UiVisual::Text {
                    layout,
                    color: [255; 4],
                    shadow: metrics.shadow(),
                }),
            );
            next_id = next_id.saturating_add(1);

            for (_, layout, y, bottom, color, selected) in &positioned_suggestions {
                nodes.push(
                    UiNode::new(
                        UiNodeId::new(next_id),
                        None,
                        rect(chat_left - 2.0, *y, chat_right, *bottom)?,
                    )
                    .with_visual(UiVisual::Solid {
                        texture_page: self.solid_texture_page,
                        color: if *selected {
                            [96, 96, 96, 224]
                        } else {
                            [0, 0, 0, 192]
                        },
                    }),
                );
                next_id = next_id.saturating_add(1);
                nodes.push(
                    UiNode::new(
                        UiNodeId::new(next_id),
                        None,
                        rect(chat_left, *y, chat_right, *bottom)?,
                    )
                    .with_visual(UiVisual::Text {
                        layout: Arc::clone(layout),
                        color: *color,
                        shadow: metrics.shadow(),
                    }),
                );
                next_id = next_id.saturating_add(1);
            }
        }

        let menu_hit_targets = if let Some(view) = self.menu_view.as_ref() {
            menu::append_menu_nodes(
                view,
                &mut nodes,
                &mut next_id,
                &mut self.layouts,
                &self.font,
                metrics,
                self.solid_texture_page,
                content_width,
                content_height,
                safe_area,
            )?
        } else {
            Vec::new()
        };

        // Hit rects are compared against window-logical pointer positions, so
        // translate the content-relative rows by the safe-area origin.
        if !menu_visible && let Some(message) = self.loading_message {
            // Keep the pre-world frame intentional: the sky clear color is a
            // renderer fallback, not a user-facing loading screen. The opaque
            // cover hides partial terrain and HUD state while the world cohort
            // is still settling.
            // Append it after the normal HUD/chat nodes so no partial terrain
            // or UI leaks through while the world cohort is still settling.
            nodes.push(
                UiNode::new(
                    UiNodeId::new(next_id),
                    None,
                    rect(0.0, 0.0, logical_width, logical_height)?,
                )
                .with_visual(UiVisual::Solid {
                    texture_page: self.solid_texture_page,
                    color: [8, 10, 14, 255],
                }),
            );
            next_id = next_id.saturating_add(1);
            let layout = self
                .layouts
                .layout(metrics.request(message, logical_width.max(1.0) as u32 * 64, &self.font))
                .map_err(UiPresentationError::Text)?;
            let width = layout.size_64()[0] as f32 / 64.0;
            let height = layout.size_64()[1] as f32 / 64.0;
            nodes.push(
                UiNode::new(
                    UiNodeId::new(next_id),
                    None,
                    rect(
                        ((logical_width - width) * 0.5).max(0.0),
                        (logical_height * 0.5 - height * 0.5).max(0.0),
                        (logical_width + width) * 0.5,
                        (logical_height * 0.5 + height * 0.5).max(height),
                    )?,
                )
                .with_visual(UiVisual::Text {
                    layout,
                    color: [235, 238, 245, 255],
                    shadow: metrics.shadow(),
                }),
            );
        }

        let chat_suggestion_hits = positioned_suggestions
            .iter()
            .map(|(index, _, top, bottom, _, _)| {
                rect(
                    chat_left + safe_area.left(),
                    *top + safe_area.top(),
                    chat_right + safe_area.left(),
                    *bottom + safe_area.top(),
                )
                .map(|bounds| (*index, bounds))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut tree = UiTree::new(nodes).map_err(UiPresentationError::Tree)?;
        tree.layout(viewport, UiScale::default(), safe_area)
            .map_err(UiPresentationError::Tree)?;
        let mut draw_list = tree.build_draw_list().map_err(UiPresentationError::Tree)?;
        self.revision = self.revision.saturating_add(1);
        draw_list.revision = self.revision;
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
        self.chat_hit_logical_size = Some([logical_width, logical_height]);
        self.chat_suggestion_hits = chat_suggestion_hits;
        self.menu_hit_targets = menu_hit_targets;
        Ok(input)
    }
}

#[cfg(test)]
pub(crate) mod tests;
