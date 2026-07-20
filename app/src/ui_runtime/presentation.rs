use std::{fmt, sync::Arc};

use assets::{HudTextureRole, RuntimeFontCatalog, RuntimeHudCatalog};
use bevy::{
    prelude::{Query, Res, ResMut, Resource, Time, With},
    time::Real,
    window::{PrimaryWindow, Window},
};
use render::{
    MAX_UI_TEXTURE_BYTES, MAX_UI_TEXTURE_LAYERS, UiRenderInput, UiRenderScene, UiRenderStats,
    UiRenderTextureArray,
};
use sha2::{Digest, Sha256};
use ui::{
    DpiScale, HudViewRole, SafeArea, TextLayoutCache, TextLayoutRequest, TextStyle, UiNode,
    UiNodeId, UiPoint, UiRect, UiScale, UiTree, UiVisual,
};

use super::{UiRuntime, render_adapter::UiRenderViewport};
use crate::{
    runtime::{shutdown::record_fatal_error, world::ClientWorld},
    ui_runtime::render_adapter::adapt_ui_draw_list,
};

mod chat;
mod retained_hud;
mod survival_hud;

use chat::visible_suggestion_range;
use retained_hud::{
    PresentedScoreboardCache, ScoreboardOpacityAuthority, ScoreboardOwnerNameAuthority,
};

const TEXT_CACHE_ENTRIES: usize = 1_024;
const TEXT_CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_PRESENTED_CHAT_ROWS: usize = 8;
const MAX_PRESENTED_CHAT_SUGGESTIONS: usize = 8;
const MAX_PRESENTED_TOAST_ROWS: usize = 8;
const MAX_PRESENTED_TEXT_BYTES: usize = 512;
const CHAT_TEXT_SCALE: f32 = 0.5;
// Java-style chat presentation (Hybrid HUD): unfocused chat lines get an always-on translucent
// black backdrop, matching Java Edition's per-line chat background (drawn at textBackgroundOpacity,
// default 0.5 -> byte alpha 128). Recorded as a Hybrid deviation in plan.md.
const CHAT_LINE_BACKDROP_COLOR: [u8; 4] = [0, 0, 0, 128];
const CHAT_LINE_BACKDROP_PAD: f32 = 2.0;
// Vanilla experience-level green, drawn above the XP bar.
const XP_LEVEL_COLOR: [u8; 4] = [128, 255, 32, 255];
const VANILLA_HUD_ATLAS_SIDE: u32 = 128;
const HUD_ATLAS_GUTTER: u32 = 1;

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
    layouts: TextLayoutCache,
    revision: u64,
    scoreboard: PresentedScoreboardCache,
    scoreboard_owner_names: ScoreboardOwnerNameAuthority,
    scoreboard_opacity: Option<ScoreboardOpacityAuthority>,
    chat_hit_logical_size: Option<[f32; 2]>,
    chat_suggestion_hits: Vec<(usize, UiRect)>,
}

impl UiPresentationRuntime {
    pub fn new(font: Arc<RuntimeFontCatalog>) -> Result<Self, UiPresentationError> {
        Self::with_optional_hud(font, None)
    }

    pub fn with_hud(
        font: Arc<RuntimeFontCatalog>,
        hud: Arc<RuntimeHudCatalog>,
    ) -> Result<Self, UiPresentationError> {
        Self::with_optional_hud(font, Some(hud))
    }

    fn with_optional_hud(
        font: Arc<RuntimeFontCatalog>,
        hud: Option<Arc<RuntimeHudCatalog>>,
    ) -> Result<Self, UiPresentationError> {
        let (textures, solid_texture_page, hud_textures) = if let Some(hud) = hud.as_deref() {
            font_texture_array_with_optional_hud(&font, Some(hud))?
        } else {
            let (textures, solid_texture_page) = font_texture_array(&font)?;
            (textures, solid_texture_page, None)
        };
        Ok(Self {
            font,
            textures: Arc::new(textures),
            solid_texture_page,
            hud_textures,
            layouts: TextLayoutCache::new(TEXT_CACHE_ENTRIES, TEXT_CACHE_BYTES),
            revision: 0,
            scoreboard: PresentedScoreboardCache::default(),
            scoreboard_owner_names: ScoreboardOwnerNameAuthority::default(),
            scoreboard_opacity: None,
            chat_hit_logical_size: None,
            chat_suggestion_hits: Vec::with_capacity(MAX_PRESENTED_CHAT_SUGGESTIONS),
        })
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
        let responsive_survival_geometry = self
            .hud_textures
            .as_ref()
            .and_then(|textures| survival_hud::responsive_geometry(logical_width, textures));
        let viewport = rect(0.0, 0.0, logical_width, logical_height)?;
        let wrap_width = ((logical_width * 0.45).clamp(1.0, 640.0) * 64.0) as u32;
        let chat_content_width = wrap_width as f32 / 64.0;
        let chat_left = 12.0_f32.min(logical_width);
        let chat_right = (chat_left + chat_content_width)
            .min(logical_width)
            .max(chat_left);
        let mut nodes = Vec::new();
        let mut next_id = 1u32;

        if let Some(hud_textures) = self.hud_textures.as_ref()
            && let Some(geometry) = responsive_survival_geometry
        {
            survival_hud::append(
                &mut nodes,
                &mut next_id,
                runtime,
                logical_height,
                hud_textures,
                geometry,
            )?;
        }

        // Experience level: green number centered just above the XP bar. The bar sprites are drawn
        // by `survival_hud`; the level text needs the layout cache, so it is rendered here.
        if let Some(geometry) = responsive_survival_geometry
            && runtime.survival_stats_visible()
            && let Some(xp) = runtime.hud().experience()
            && xp.level > 0
        {
            let level_text = xp.level.to_string();
            let layout = self
                .layouts
                .layout(TextLayoutRequest {
                    text: &level_text,
                    style: TextStyle::default(),
                    width_64: (logical_width.max(1.0) * 64.0) as u32,
                    scale: UiScale::default(),
                    font: &self.font,
                })
                .map_err(UiPresentationError::Text)?;
            let [bar_left, bar_top, bar_right, _] = geometry.xp_bar_rect(logical_height);
            let text_width = layout.size_64()[0] as f32 / 64.0;
            let text_height = layout.size_64()[1] as f32 / 64.0;
            let x = ((bar_left + bar_right) * 0.5 - text_width * 0.5).max(0.0);
            let y = (bar_top - text_height - 1.0).max(0.0);
            nodes.push(
                UiNode::new(
                    UiNodeId::new(next_id),
                    None,
                    rect(x, y, x + text_width, y + text_height)?,
                )
                .with_visual(UiVisual::Text {
                    layout,
                    color: XP_LEVEL_COLOR,
                }),
            );
            next_id = next_id.saturating_add(1);
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
                .layout(TextLayoutRequest {
                    text,
                    style: TextStyle::default(),
                    width_64: wrap_width,
                    scale: UiScale::default(),
                    font: &self.font,
                })
                .map_err(UiPresentationError::Text)?;
            let [x, y] = hud_position(node.role, nodes.len(), logical_width, logical_height);
            nodes.push(
                UiNode::new(
                    UiNodeId::new(next_id),
                    None,
                    rect(
                        x,
                        y,
                        (x + logical_width * 0.45).min(logical_width),
                        logical_height,
                    )?,
                )
                .with_visual(UiVisual::Text {
                    layout,
                    color: [255; 4],
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
                self.solid_texture_page,
                logical_width,
                logical_height,
                scoreboard,
                opacity,
            )?;
        }

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
            let mut visible = String::with_capacity(editor.len_bytes().saturating_add(3));
            visible.push_str("> ");
            visible.push_str(&editor.as_str()[..editor.cursor_byte()]);
            visible.push('|');
            visible.push_str(&editor.as_str()[editor.cursor_byte()..]);
            editor_layout = Some(
                self.layouts
                    .layout(TextLayoutRequest {
                        text: bounded_visible_text(&visible),
                        style: TextStyle::default(),
                        width_64: wrap_width,
                        scale: chat_text_scale(),
                        font: &self.font,
                    })
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
                    .layout(TextLayoutRequest {
                        text: bounded_visible_text(&visible),
                        style: TextStyle::default(),
                        width_64: wrap_width,
                        scale: chat_text_scale(),
                        font: &self.font,
                    })
                    .map_err(UiPresentationError::Text)?;
                suggestion_layouts.push((index, layout, [220, 220, 220, 255]));
            }
        }
        let suggestion_reserved_height = suggestion_layouts
            .iter()
            .map(|(_, layout, _)| layout.size_64()[1] as f32 / 64.0 + 4.0)
            .sum::<f32>();
        let chat_region_top = (logical_height - 220.0 - suggestion_reserved_height).max(0.0);
        let bottom_hud_top = responsive_survival_geometry.map_or_else(
            || (logical_height - 42.0).max(chat_region_top),
            |geometry| geometry.bottom_row_top(logical_height).max(chat_region_top),
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
        let chat = runtime.chat().view_nodes();
        let first = chat.len().saturating_sub(MAX_PRESENTED_CHAT_ROWS);
        let chat_bottom = if chat_focused {
            suggestion_cursor
        } else {
            (logical_height - 72.0).max(chat_region_top)
        };
        let mut chat_cursor = chat_bottom;
        let mut visible_chat = Vec::new();
        for node in chat[first..].iter().rev() {
            let text = bounded_visible_text(&node.text);
            let layout = self
                .layouts
                .layout(TextLayoutRequest {
                    text,
                    style: TextStyle::default(),
                    width_64: wrap_width,
                    scale: chat_text_scale(),
                    font: &self.font,
                })
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
                            .layout(TextLayoutRequest {
                                text: &text[..boundaries[middle]],
                                style: TextStyle::default(),
                                width_64: wrap_width,
                                scale: chat_text_scale(),
                                font: &self.font,
                            })
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
                        visible_chat.push((layout, chat_cursor - height, chat_cursor));
                    }
                }
                break;
            }
            let y = chat_cursor - layout_height;
            visible_chat.push((layout, y, chat_cursor));
            chat_cursor = (y - 4.0).max(chat_region_top);
        }
        if chat_focused {
            let panel_left = 8.0_f32.min(logical_width);
            let panel_right = (panel_left + chat_content_width + 8.0)
                .min(logical_width)
                .max(panel_left);
            let content_top = visible_chat
                .iter()
                .map(|(_, top, _)| *top)
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
        // Java-style unfocused chat: draw a translucent per-line backdrop behind each visible
        // line. When focused, the unified chat panel above already provides the background, so
        // these are skipped to avoid double-darkening. Backdrops precede the text nodes so they
        // render underneath.
        if !chat_focused {
            let backdrop_left = (chat_left - CHAT_LINE_BACKDROP_PAD).max(0.0);
            for (_, top, bottom) in &visible_chat {
                nodes.push(
                    UiNode::new(
                        UiNodeId::new(next_id),
                        None,
                        rect(backdrop_left, *top, chat_right, *bottom)?,
                    )
                    .with_visual(UiVisual::Solid {
                        texture_page: self.solid_texture_page,
                        color: CHAT_LINE_BACKDROP_COLOR,
                    }),
                );
                next_id = next_id.saturating_add(1);
            }
        }
        for (layout, y, bottom) in visible_chat.into_iter().rev() {
            nodes.push(
                UiNode::new(
                    UiNodeId::new(next_id),
                    None,
                    rect(chat_left, y, chat_right, bottom)?,
                )
                .with_visual(UiVisual::Text {
                    layout,
                    color: [255; 4],
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
                    }),
                );
                next_id = next_id.saturating_add(1);
            }
        }

        let chat_suggestion_hits = positioned_suggestions
            .iter()
            .map(|(index, _, top, bottom, _)| {
                rect(chat_left, *top, chat_right, *bottom).map(|bounds| (*index, bounds))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut tree = UiTree::new(nodes).map_err(UiPresentationError::Tree)?;
        tree.layout(viewport, UiScale::default(), SafeArea::ZERO)
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
                safe_area: SafeArea::ZERO,
            },
        )
        .map_err(UiPresentationError::Adapter)?;
        self.chat_hit_logical_size = Some([logical_width, logical_height]);
        self.chat_suggestion_hits = chat_suggestion_hits;
        Ok(input)
    }
}

pub(crate) fn publish_ui_runtime(
    mut runtime: ResMut<UiRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
    mut scene: ResMut<UiRenderScene>,
    stats: Res<UiRenderStats>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut client_world: ResMut<ClientWorld>,
    time: Res<Time<Real>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let physical_size = [window.physical_width(), window.physical_height()];
    if physical_size.contains(&0) {
        return;
    }
    let Ok(dpi_scale) = DpiScale::new(window.scale_factor()) else {
        record_fatal_error(
            &mut client_world.fatal_error,
            "primary window reported an unsupported UI DPI scale".to_owned(),
        );
        return;
    };
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    runtime.hud.expire(now_millis);
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct HudSprite {
    uv: [u16; 4],
    size: [u16; 2],
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HudTexturePages {
    page: u16,
    sprites: [HudSprite; HudTextureRole::ALL.len()],
}

impl HudTexturePages {
    fn sprite(&self, role: HudTextureRole) -> HudSprite {
        self.sprites[role as usize]
    }
}

fn font_texture_array_with_optional_hud(
    font: &RuntimeFontCatalog,
    hud: Option<&RuntimeHudCatalog>,
) -> Result<(UiRenderTextureArray, u16, Option<HudTexturePages>), UiPresentationError> {
    let mut width = font
        .pages()
        .iter()
        .map(|page| page.width)
        .max()
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let mut height = font
        .pages()
        .iter()
        .map(|page| page.height)
        .max()
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    if hud.is_some() {
        width = width.max(VANILLA_HUD_ATLAS_SIDE);
        height = height.max(VANILLA_HUD_ATLAS_SIDE);
    }
    let font_layers =
        u32::try_from(font.pages().len()).map_err(|_| UiPresentationError::InvalidFontTexture)?;
    if font_layers >= MAX_UI_TEXTURE_LAYERS {
        return Err(UiPresentationError::InvalidFontTexture);
    }
    let solid_texture_page =
        u16::try_from(font_layers).map_err(|_| UiPresentationError::InvalidFontTexture)?;
    let hud_layers = u32::from(hud.is_some());
    let layers = font_layers
        .checked_add(1)
        .and_then(|layers| layers.checked_add(hud_layers))
        .filter(|layers| *layers <= MAX_UI_TEXTURE_LAYERS)
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let layer_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(height as usize))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let total_bytes = layer_bytes
        .checked_mul(layers as usize)
        .filter(|bytes| *bytes <= MAX_UI_TEXTURE_BYTES)
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let mut rgba8 = vec![0; total_bytes];
    for (layer, page) in font.pages().iter().enumerate() {
        let page_width = page.width as usize;
        let page_height = page.height as usize;
        for row in 0..page_height {
            let source_start = row * page_width * 4;
            let source_end = source_start + page_width * 4;
            let target_start = layer * layer_bytes + row * width as usize * 4;
            rgba8[target_start..target_start + page_width * 4]
                .copy_from_slice(&page.rgba8[source_start..source_end]);
        }
    }
    let solid_start = usize::from(solid_texture_page) * layer_bytes;
    rgba8[solid_start..solid_start + layer_bytes].fill(255);
    let hud_textures = if let Some(hud) = hud {
        let page = solid_texture_page
            .checked_add(1)
            .ok_or(UiPresentationError::InvalidFontTexture)?;
        let layer_start = usize::from(page) * layer_bytes;
        let mut cursor = [0u32, 0u32];
        let mut row_height = 0u32;
        let mut sprites = [HudSprite::default(); HudTextureRole::ALL.len()];
        for texture in hud.textures() {
            let gutter_span = HUD_ATLAS_GUTTER
                .checked_mul(2)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let padded_width = texture
                .width
                .checked_add(gutter_span)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let padded_height = texture
                .height
                .checked_add(gutter_span)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let row_right = cursor[0]
                .checked_add(padded_width)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            if row_right > width {
                cursor[0] = 0;
                cursor[1] = cursor[1]
                    .checked_add(row_height)
                    .ok_or(UiPresentationError::InvalidFontTexture)?;
                row_height = 0;
            }
            let padded_right = cursor[0]
                .checked_add(padded_width)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let padded_bottom = cursor[1]
                .checked_add(padded_height)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            if padded_right > width || padded_bottom > height {
                return Err(UiPresentationError::InvalidFontTexture);
            }
            let left = cursor[0]
                .checked_add(HUD_ATLAS_GUTTER)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let top = cursor[1]
                .checked_add(HUD_ATLAS_GUTTER)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let right = left
                .checked_add(texture.width)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            let bottom = top
                .checked_add(texture.height)
                .ok_or(UiPresentationError::InvalidFontTexture)?;
            for padded_y in 0..padded_height {
                let source_y = padded_y
                    .saturating_sub(HUD_ATLAS_GUTTER)
                    .min(texture.height - 1);
                for padded_x in 0..padded_width {
                    let source_x = padded_x
                        .saturating_sub(HUD_ATLAS_GUTTER)
                        .min(texture.width - 1);
                    let source_start =
                        (source_y as usize * texture.width as usize + source_x as usize) * 4;
                    let target_start = layer_start
                        + ((cursor[1] + padded_y) as usize * width as usize
                            + (cursor[0] + padded_x) as usize)
                            * 4;
                    rgba8[target_start..target_start + 4]
                        .copy_from_slice(&texture.rgba8[source_start..source_start + 4]);
                }
            }
            sprites[texture.role as usize] = HudSprite {
                uv: [
                    u16::try_from(left).map_err(|_| UiPresentationError::InvalidFontTexture)?,
                    u16::try_from(top).map_err(|_| UiPresentationError::InvalidFontTexture)?,
                    u16::try_from(right).map_err(|_| UiPresentationError::InvalidFontTexture)?,
                    u16::try_from(bottom).map_err(|_| UiPresentationError::InvalidFontTexture)?,
                ],
                size: [
                    u16::try_from(texture.width)
                        .map_err(|_| UiPresentationError::InvalidFontTexture)?,
                    u16::try_from(texture.height)
                        .map_err(|_| UiPresentationError::InvalidFontTexture)?,
                ],
            };
            cursor[0] = padded_right;
            row_height = row_height.max(padded_height);
        }
        Some(HudTexturePages { page, sprites })
    } else {
        None
    };
    let texture_identity = if let Some(hud) = hud {
        let mut identity = Sha256::new();
        identity.update(font.identity().carrier_sha256);
        identity.update(hud.source_manifest_sha256());
        for texture in hud.textures() {
            identity.update(texture.pixels_sha256);
        }
        identity.finalize().into()
    } else {
        font.identity().carrier_sha256
    };
    Ok((
        UiRenderTextureArray {
            identity: texture_identity,
            width,
            height,
            layers,
            rgba8: rgba8.into(),
        },
        solid_texture_page,
        hud_textures,
    ))
}

fn font_texture_array(
    font: &RuntimeFontCatalog,
) -> Result<(UiRenderTextureArray, u16), UiPresentationError> {
    let width = font
        .pages()
        .iter()
        .map(|page| page.width)
        .max()
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let height = font
        .pages()
        .iter()
        .map(|page| page.height)
        .max()
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let font_layers =
        u32::try_from(font.pages().len()).map_err(|_| UiPresentationError::InvalidFontTexture)?;
    if font_layers >= MAX_UI_TEXTURE_LAYERS {
        return Err(UiPresentationError::InvalidFontTexture);
    }
    let solid_texture_page =
        u16::try_from(font_layers).map_err(|_| UiPresentationError::InvalidFontTexture)?;
    let layers = font_layers
        .checked_add(1)
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let layer_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(height as usize))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let total_bytes = layer_bytes
        .checked_mul(layers as usize)
        .filter(|bytes| *bytes <= MAX_UI_TEXTURE_BYTES)
        .ok_or(UiPresentationError::InvalidFontTexture)?;
    let mut rgba8 = vec![0; total_bytes];
    for (layer, page) in font.pages().iter().enumerate() {
        let page_width = page.width as usize;
        let page_height = page.height as usize;
        for row in 0..page_height {
            let source_start = row * page_width * 4;
            let source_end = source_start + page_width * 4;
            let target_start = layer * layer_bytes + row * width as usize * 4;
            rgba8[target_start..target_start + page_width * 4]
                .copy_from_slice(&page.rgba8[source_start..source_end]);
        }
    }
    let solid_start = usize::from(solid_texture_page) * layer_bytes;
    rgba8[solid_start..solid_start + layer_bytes].fill(255);
    Ok((
        UiRenderTextureArray {
            identity: font.identity().carrier_sha256,
            width,
            height,
            layers,
            rgba8: rgba8.into(),
        },
        solid_texture_page,
    ))
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

fn chat_text_scale() -> UiScale {
    UiScale::new(CHAT_TEXT_SCALE).expect("the reviewed compact chat scale is valid")
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
mod tests;
