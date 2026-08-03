use std::{fmt, sync::Arc};

use assets::{RuntimeFontCatalog, RuntimeHudCatalog, RuntimeIconCatalog};
use bevy::{
    camera::Camera,
    math::Vec3,
    prelude::{Camera3d, GlobalTransform, Query, Res, ResMut, Resource, Time, With},
    time::Real,
    window::{PrimaryWindow, Window},
};
use render::{ActorSkinPixels, normalize_actor_skin};
use render::{
    MAX_UI_TEXTURE_LAYERS, UiRenderInput, UiRenderScene, UiRenderStats, UiRenderTextureArray,
};
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
mod hud_layout;
mod player_preview;
mod retained_hud;
mod texture_atlas;

use chat::visible_suggestion_range;
pub(crate) use hud_layout::HudFrame;
use hud_layout::{HudGeometry, HudLayout, java_gui_scale};
use retained_hud::{
    BelowNameAnchor, PresentedScoreboardCache, ScoreboardOpacityAuthority,
    ScoreboardOwnerNameAuthority,
};
use texture_atlas::{
    HudSprite, HudTexturePages, IconRef, font_texture_array, font_texture_array_with_hud_and_icons,
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
// Keep the opaque loading cover up until an initial playable neighborhood of
// visible terrain has arrived. A worker-completed mesh can still be waiting in the
// render queue, so counting mesh jobs alone exposes a brief void/partial-world
// flash while the initial stream is still being admitted.
const MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION: usize = 1_024;

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
    player_preview_icon: Option<IconRef>,
    left_hand_icon: Option<IconRef>,
    right_hand_icon: Option<IconRef>,
    loading_message: Option<&'static str>,
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
        Ok(Self {
            font,
            textures: Arc::new(textures),
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
            player_preview_icon: None,
            left_hand_icon: None,
            right_hand_icon: None,
            loading_message: None,
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
        let Some(page) = self.player_preview_page.or_else(|| {
            (self.textures.layers < MAX_UI_TEXTURE_LAYERS).then_some(self.textures.layers as u16)
        }) else {
            self.player_preview_icon = None;
            self.left_hand_icon = None;
            self.right_hand_icon = None;
            return;
        };
        let layer_bytes = usize::try_from(self.textures.width)
            .ok()
            .and_then(|width| width.checked_mul(self.textures.height as usize))
            .and_then(|pixels| pixels.checked_mul(4));
        let Some(layer_bytes) = layer_bytes else {
            self.player_preview_icon = None;
            self.left_hand_icon = None;
            self.right_hand_icon = None;
            return;
        };
        if self.textures.width < player_preview::PREVIEW_WIDTH
            || self.textures.height < player_preview::PREVIEW_HEIGHT
            || self.textures.width < player_preview::HAND_WIDTH.saturating_mul(2)
            || self.textures.height
                < player_preview::PREVIEW_HEIGHT.saturating_add(player_preview::HAND_HEIGHT)
        {
            self.player_preview_icon = None;
            self.left_hand_icon = None;
            self.right_hand_icon = None;
            return;
        }
        let preview = player_preview::render(skin, pose);
        let left_hand = player_preview::render_hand(skin, pose, true);
        let right_hand = player_preview::render_hand(skin, pose, false);
        let mut rgba8 = self.textures.rgba8.to_vec();
        if self.player_preview_page.is_none() {
            rgba8.extend(std::iter::repeat_n(0, layer_bytes));
            self.player_preview_page = Some(page);
        }
        let layer_start = usize::from(page) * layer_bytes;
        for row in 0..player_preview::PREVIEW_HEIGHT as usize {
            let source_start = row * player_preview::PREVIEW_WIDTH as usize * 4;
            let target_start = layer_start + row * self.textures.width as usize * 4;
            let target_end = target_start + player_preview::PREVIEW_WIDTH as usize * 4;
            rgba8[target_start..target_end].copy_from_slice(
                &preview[source_start..source_start + player_preview::PREVIEW_WIDTH as usize * 4],
            );
        }
        let copy_raster = |rgba8: &mut [u8], raster: &[u8], origin: [u32; 2]| {
            let raster_width = player_preview::HAND_WIDTH as usize;
            let raster_height = player_preview::HAND_HEIGHT as usize;
            let texture_width = self.textures.width as usize;
            for row in 0..raster_height {
                let source_start = row * raster_width * 4;
                let target_start = layer_start
                    + ((origin[1] as usize + row) * texture_width + origin[0] as usize) * 4;
                rgba8[target_start..target_start + raster_width * 4]
                    .copy_from_slice(&raster[source_start..source_start + raster_width * 4]);
            }
        };
        copy_raster(&mut rgba8, &left_hand, [0, player_preview::PREVIEW_HEIGHT]);
        copy_raster(
            &mut rgba8,
            &right_hand,
            [player_preview::HAND_WIDTH, player_preview::PREVIEW_HEIGHT],
        );
        let layers = self
            .player_preview_page
            .map_or(self.textures.layers, |page| {
                self.textures.layers.max(u32::from(page) + 1)
            });
        let mut identity = Sha256::new();
        identity.update(self.base_texture_identity);
        identity.update(b"cinnabar-player-preview-v2");
        identity.update(source_hash);
        identity.update(pose.body_yaw_degrees.to_bits().to_le_bytes());
        identity.update(pose.head_yaw_degrees.to_bits().to_le_bytes());
        identity.update(pose.pitch_degrees.to_bits().to_le_bytes());
        identity.update([u8::from(pose.sneaking)]);
        let identity: [u8; 32] = identity.finalize().into();
        self.textures = Arc::new(UiRenderTextureArray {
            identity,
            width: self.textures.width,
            height: self.textures.height,
            layers,
            rgba8: rgba8.into(),
        });
        self.player_preview_source_hash = Some(source_hash);
        self.player_preview_pose = Some(pose);
        self.player_preview_icon = Some(IconRef {
            page,
            uv: [
                0,
                0,
                player_preview::PREVIEW_WIDTH as u16,
                player_preview::PREVIEW_HEIGHT as u16,
            ],
        });
        self.left_hand_icon = Some(IconRef {
            page,
            uv: [
                0,
                player_preview::PREVIEW_HEIGHT as u16,
                player_preview::HAND_WIDTH as u16,
                player_preview::PREVIEW_HEIGHT as u16 + player_preview::HAND_HEIGHT as u16,
            ],
        });
        self.right_hand_icon = Some(IconRef {
            page,
            uv: [
                player_preview::HAND_WIDTH as u16,
                player_preview::PREVIEW_HEIGHT as u16,
                player_preview::HAND_WIDTH.saturating_mul(2) as u16,
                player_preview::PREVIEW_HEIGHT as u16 + player_preview::HAND_HEIGHT as u16,
            ],
        });
    }

    pub(crate) const fn player_preview_icon(&self) -> Option<IconRef> {
        self.player_preview_icon
    }

    pub(crate) const fn player_hand_icons(&self) -> (Option<IconRef>, Option<IconRef>) {
        (self.left_hand_icon, self.right_hand_icon)
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

        if let Some(hud_textures) = self.hud_textures.as_ref()
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

        let hud_nodes = runtime.hud().view_nodes(now_millis);
        let mut toast_rows = 0usize;
        for node in hud_nodes.iter() {
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

        if let Some(opacity) = self.scoreboard_opacity
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
        if self.hud_frame.tab_list_open {
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

        let chat_focused = runtime.chat_focused();
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
                let mut visible = String::with_capacity(suggestion.len().saturating_add(2));
                visible.push_str(if selected { "> " } else { "  " });
                visible.push_str(suggestion);
                let layout = self
                    .layouts
                    .layout(metrics.request(bounded_visible_text(&visible), wrap_width, &self.font))
                    .map_err(UiPresentationError::Text)?;
                suggestion_layouts.push((index, layout, [220, 220, 220, 255]));
            }
        }

        let suggestion_reserved_height = suggestion_layouts
            .iter()
            .map(|(_, layout, _)| layout.size_64()[1] as f32 / 64.0 + 4.0)
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
        for (index, layout, color) in suggestion_layouts {
            let layout_height = layout.size_64()[1] as f32 / 64.0;
            if layout_height > suggestion_cursor - chat_region_top {
                break;
            }
            let y = suggestion_cursor - layout_height;
            positioned_suggestions.push((index, layout, y, suggestion_cursor, color));
            suggestion_cursor = (y - 4.0).max(chat_region_top);
        }
        let chat = runtime.chat().messages();
        let first = chat.len().saturating_sub(MAX_PRESENTED_CHAT_ROWS);
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
            let content_top = visible_chat
                .iter()
                .map(|(_, top, _, _)| *top)
                .chain(positioned_suggestions.iter().map(|(_, _, top, _, _)| *top))
                .chain(std::iter::once(editor_y))
                .fold(editor_y, f32::min);
            let panel_top = (content_top - 4.0).max(chat_region_top);
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

            for (_, layout, y, bottom, color) in &positioned_suggestions {
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

        // Hit rects are compared against window-logical pointer positions, so
        // translate the content-relative rows by the safe-area origin.
        if let Some(message) = self.loading_message {
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
            .map(|(index, _, top, bottom, _)| {
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
        Ok(input)
    }
}

/// Observes the held HUD inputs — jump for the mount jump-charge ramp and
/// the player-list action for the tab overlay — before the frame publishes.
pub(crate) fn observe_mount_jump_input(
    input: Res<crate::semantic_controls::SemanticInputSnapshot>,
    mut runtime: ResMut<UiRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
    time: Res<Time<Real>>,
) {
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.set_mount_jump_held(input.phase(semantic_input::Action::Jump).held, now_millis);
    presentation.hud_frame_mut().tab_list_open =
        input.phase(semantic_input::Action::PlayerList).held;
}

/// The platform's safe-area insets for the primary surface, in logical px.
/// Win32 and macOS desktop surfaces carry no display cutouts, so their real
/// reported inset is zero on every edge; platforms that report cutouts bind
/// their values here and every consumer — HUD geometry, retained layout,
/// render clipping — picks them up through `set_safe_area`.
pub(crate) fn platform_safe_area_insets() -> SafeArea {
    SafeArea::ZERO
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_ui_runtime(
    mut runtime: ResMut<UiRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
    mut scene: ResMut<UiRenderScene>,
    stats: Res<UiRenderStats>,
    visibility: Res<CaveVisibilityCache>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut client_world: ResMut<ClientWorld>,
    camera_settings: Res<CameraSettingsAuthority>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    frame_poll: Res<WorldStreamFramePoll>,
    time: Res<Time<Real>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let physical_size = [window.physical_width(), window.physical_height()];
    if physical_size.contains(&0) {
        return;
    }
    let logical_width = physical_size[0] as f32 / window.scale_factor();
    let logical_height = physical_size[1] as f32 / window.scale_factor();
    let Ok(dpi_scale) = DpiScale::new(window.scale_factor()) else {
        record_fatal_error(
            &mut client_world.fatal_error,
            "primary window reported an unsupported UI DPI scale".to_owned(),
        );
        return;
    };
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.hud.expire(now_millis);
    let terrain_ready_target = frame_poll
        .cohort
        .filter(|status| status.is_exact())
        .map_or(MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION, |status| {
            status.expected.max(MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION)
        });
    // BDS keeps the scene behind its loading branch until the primary client
    // has a coherent, renderable world. The ordinary direct-server path does
    // not use AcceptanceRun (that tracker intentionally requires a mutation
    // target), so derive the same boundary from the normal stream queues.
    let terrain_ready = frame_poll.cohort.is_some_and(|status| status.is_exact())
        && visibility.visible_rendered >= terrain_ready_target
        && client_world.stream.as_ref().is_some_and(|stream| {
            let stats = stream.stats();
            stats.pending_light_jobs == 0
                && stats.in_flight_light_jobs == 0
                && stats.pending_mesh_jobs == 0
                && stats.in_flight_mesh_jobs == 0
                && stream.pending_mesh_change_count() == 0
                && stream.unacknowledged_mesh_count() == 0
        });
    runtime.drain_pending_inventory();
    runtime.expire_gameplay_effects(now_millis);
    runtime.observe_selected_item_identity(now_millis);
    let player_preview_skin = client_world.stream.as_ref().and_then(|stream| {
        let profile = stream.actor_player_profile(stream.local_player_runtime_id())?;
        let protocol::PlayerSkin::Standard(skin) = &profile.skin else {
            return None;
        };
        normalize_actor_skin(&ActorSkinPixels {
            width: skin.width,
            height: skin.height,
            rgba8: Arc::clone(&skin.rgba8),
        })
    });
    let player_preview_pose = client_world
        .stream
        .as_ref()
        .and_then(|stream| stream.actor(stream.local_player_runtime_id()))
        .map_or_else(player_preview::PlayerPreviewPose::default, |actor| {
            let sneaking = matches!(
                actor.metadata.get(&0),
                Some(protocol::ActorMetadataValue::Flags(flags)) if flags & (1_u64 << 1) != 0
            );
            player_preview::PlayerPreviewPose::new(
                actor.body_yaw,
                actor.head_yaw,
                actor.pitch,
                sneaking,
            )
        });
    presentation.set_player_preview_skin(player_preview_skin.as_deref(), player_preview_pose);
    refresh_hud_frame(
        &mut runtime,
        &mut presentation,
        client_world.stream.as_ref(),
        &camera_settings,
        now_millis,
    );
    let below_name_anchors = client_world
        .stream
        .as_ref()
        .zip(cameras.single().ok())
        .map(|(stream, (camera, camera_transform))| {
            project_below_name_anchors(
                runtime.scoreboards(),
                stream,
                camera,
                camera_transform,
                [logical_width, logical_height],
                presentation.safe_area,
            )
        })
        .unwrap_or_default();
    presentation.set_below_name_anchors(below_name_anchors);
    let loading_message = match client_world.stream.as_ref() {
        None => Some("Connecting to server..."),
        Some(_) if frame_poll.cohort.is_none() && visibility.visible_rendered == 0 => {
            Some("Connecting to server...")
        }
        Some(_) if terrain_ready => None,
        Some(_) => Some("Loading terrain..."),
    };
    presentation.set_loading_message(loading_message);
    if presentation.scoreboard_opacity.is_some() {
        presentation
            .refresh_scoreboard_owner_names(runtime.scoreboards(), client_world.stream.as_ref());
    }
    let input = match presentation.build(&runtime, now_millis, physical_size, dpi_scale) {
        Ok(input) => input,
        Err(error) => {
            record_fatal_error(&mut client_world.fatal_error, error.to_string());
            return;
        }
    };
    if let Err(error) = scene.publish(input, &stats) {
        record_fatal_error(
            &mut client_world.fatal_error,
            UiPresentationError::Render(error).to_string(),
        );
    }
}

/// Refreshes the per-frame HUD inputs that need the world stream: derived
/// armor points, per-slot durability, the selected-item name, and the mount's
/// authoritative health. Without a stream every derived value fails closed.
pub(crate) fn refresh_hud_frame(
    runtime: &mut UiRuntime,
    presentation: &mut UiPresentationRuntime,
    stream: Option<&client_world::WorldStream>,
    camera_settings: &CameraSettingsAuthority,
    now_millis: u64,
) {
    let resolve_identifier = |stack: &protocol::NetworkItemStack| {
        stream.and_then(|stream| stream.canonical_item_stack(stack)?.identifier)
    };

    let derived_armor = runtime.gameplay_hud().armor().map(|slots| {
        let identifiers = [
            &slots.helmet,
            &slots.chestplate,
            &slots.leggings,
            &slots.boots,
        ]
        .map(|stack| {
            (!stack.is_empty())
                .then(|| resolve_identifier(stack))
                .flatten()
        });
        item_facts::total_armor_points(identifiers.iter().map(|identifier| identifier.as_deref()))
    });
    runtime.set_derived_armor(derived_armor);

    let mount_health = runtime
        .gameplay_hud()
        .mount_unique_id()
        .and_then(|unique| stream.and_then(|stream| stream.actor_health_by_unique(unique)));

    let mut hotbar_durability = [None; 9];
    let mut hotbar_icons = [None; 9];
    for (slot, durability) in hotbar_durability.iter_mut().enumerate() {
        if let Some(stack) = runtime.presented_hotbar_stack(slot as u8) {
            let identifier = resolve_identifier(stack);
            *durability = item_facts::durability_fraction(stack, identifier.as_deref());
            let icon = identifier
                .as_deref()
                .and_then(|identifier| presentation.item_icon(identifier, stack.metadata));
            hotbar_icons[slot] = icon;
        }
    }
    let offhand_durability = runtime.gameplay_hud().offhand_stack().and_then(|stack| {
        let identifier = resolve_identifier(stack);
        item_facts::durability_fraction(stack, identifier.as_deref())
    });
    let offhand_icon = runtime.gameplay_hud().offhand_stack().and_then(|stack| {
        let identifier = resolve_identifier(stack);
        identifier
            .as_deref()
            .and_then(|identifier| presentation.item_icon(identifier, stack.metadata))
    });
    let held_item_icon = runtime.selected_stack().and_then(|stack| {
        resolve_identifier(stack)
            .as_deref()
            .and_then(|identifier| presentation.item_icon(identifier, stack.metadata))
    });
    let selected_item_name = runtime.selected_stack().and_then(|stack| {
        resolve_identifier(stack)
            .map(|identifier| Arc::from(runtime.localized_item_name(&identifier)))
    });

    // The mount jump bar activates while riding a mount whose authoritative
    // attributes include jump strength; the charge follows the held jump
    // input's ramp and stays at zero while the input is released.
    let mount_jump = runtime.gameplay_hud().mount_unique_id().and_then(|unique| {
        stream
            .filter(|stream| {
                stream.actor_has_attribute_by_unique(unique, "minecraft:horse.jump_strength")
            })
            .map(|_| runtime.mount_jump_charge(now_millis))
    });

    let first_person =
        camera_settings.perspective() == semantic_input::PerspectiveMode::FirstPerson;
    let player_preview_icon = presentation.player_preview_icon();
    let (left_hand_icon, right_hand_icon) = presentation.player_hand_icons();
    let frame = presentation.hud_frame_mut();
    frame.first_person = first_person;
    frame.mount_health = mount_health;
    frame.hotbar_durability = hotbar_durability;
    frame.offhand_durability = offhand_durability;
    frame.hotbar_icons = hotbar_icons;
    frame.offhand_icon = offhand_icon;
    frame.held_item_icon = held_item_icon;
    frame.player_preview = player_preview_icon;
    frame.left_hand = left_hand_icon;
    frame.right_hand = right_hand_icon;
    frame.viewmodel_pitch_degrees = stream
        .and_then(|stream| stream.actor(stream.local_player_runtime_id()))
        .map_or(0.0, |actor| actor.pitch);
    frame.selected_item_name = selected_item_name;
    frame.mount_jump = mount_jump;
    // Bedrock is authoritative for melee readiness and exposes no cooldown
    // state: the charge is exactly full, which the reference presents as a
    // hidden indicator. The presentation branch below full stays witnessed.
    frame.attack_indicator_charge = Some(1.0);

    // Odd remote gameplay data is skipped and counted, never fatal; surface
    // each counter change once so live sessions record what was dropped.
    let diagnostics = runtime.gameplay_hud().diagnostics();
    if diagnostics != presentation.last_hud_diagnostics {
        bevy::log::debug!(
            skipped_effect_actions = diagnostics.skipped_effect_actions,
            evicted_effects = diagnostics.evicted_effects,
            odd_metadata_values = diagnostics.odd_metadata_values,
            dropped_inventory_events = diagnostics.dropped_inventory_events,
            odd_attribute_values = diagnostics.odd_attribute_values,
            odd_hud_packets = diagnostics.odd_hud_packets,
            oversized_chat_rows = diagnostics.oversized_chat_rows,
            unknown_effect_ids = diagnostics.unknown_effect_ids,
            "gameplay HUD skipped odd remote data"
        );
        presentation.last_hud_diagnostics = diagnostics;
    }
}

fn project_below_name_anchors(
    scoreboards: &ui::ScoreboardStore,
    stream: &client_world::WorldStream,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    logical_size: [f32; 2],
    safe_area: SafeArea,
) -> Vec<BelowNameAnchor> {
    let content_width = (logical_size[0] - safe_area.left() - safe_area.right()).max(0.0);
    let content_height = (logical_size[1] - safe_area.top() - safe_area.bottom()).max(0.0);
    stream
        .render_players()
        .into_iter()
        .filter_map(|(actor, _profile)| {
            let below_name = scoreboards
                .below_name_for_owner(&ui::ScoreOwner::Player(actor.unique_id))
                .or_else(|| {
                    scoreboards.below_name_for_owner(&ui::ScoreOwner::Entity(actor.unique_id))
                })?;
            let name = stream.actor_display_name(actor.unique_id)?;
            let position = Vec3::from_array(actor.position) + Vec3::Y * 2.35;
            let viewport = camera.world_to_viewport(camera_transform, position).ok()?;
            let x = viewport.x - safe_area.left();
            let y = viewport.y - safe_area.top();
            (x.is_finite()
                && y.is_finite()
                && x >= 0.0
                && x <= content_width
                && y >= 0.0
                && y <= content_height)
                .then_some(BelowNameAnchor {
                    x,
                    y,
                    name,
                    score: below_name.0,
                    objective: below_name.1,
                })
        })
        .take(retained_hud::MAX_PRESENTED_BELOW_NAME_ROWS)
        .collect()
}

fn bounded_visible_text(value: &str) -> &str {
    if value.len() <= MAX_PRESENTED_TEXT_BYTES {
        return value;
    }
    let mut end = MAX_PRESENTED_TEXT_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn hud_position(role: HudViewRole, ordinal: usize, width: f32, height: f32) -> [f32; 2] {
    match role {
        HudViewRole::Health => [12.0, (height - 42.0).max(0.0)],
        HudViewRole::Hunger => [(width - 180.0).max(0.0), (height - 42.0).max(0.0)],
        HudViewRole::Armor => [12.0, (height - 62.0).max(0.0)],
        HudViewRole::Air => [(width - 180.0).max(0.0), (height - 62.0).max(0.0)],
        HudViewRole::Title => [(width * 0.3).max(0.0), (height * 0.3).max(0.0)],
        HudViewRole::Subtitle => [(width * 0.3).max(0.0), (height * 0.3 + 24.0).max(0.0)],
        HudViewRole::ActionBar => [(width * 0.35).max(0.0), (height - 90.0).max(0.0)],
        HudViewRole::ToastTitle | HudViewRole::ToastMessage => {
            [(width - 320.0).max(0.0), 12.0 + ordinal as f32 * 18.0]
        }
    }
}

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> Result<UiRect, UiPresentationError> {
    UiRect::new(
        UiPoint::new(left, top).map_err(UiPresentationError::Geometry)?,
        UiPoint::new(right, bottom).map_err(UiPresentationError::Geometry)?,
    )
    .map_err(UiPresentationError::Geometry)
}

#[cfg(test)]
pub(crate) mod tests;
