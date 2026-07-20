use std::{fmt, ops::Range, sync::Arc};

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
    BoundedStat, DpiScale, HudViewRole, SafeArea, TextLayoutCache, TextLayoutRequest, TextStyle,
    UiNode, UiNodeId, UiPoint, UiRect, UiScale, UiTree, UiVisual,
};

use super::{UiRuntime, render_adapter::UiRenderViewport};
use crate::{
    runtime::{shutdown::record_fatal_error, world::ClientWorld},
    ui_runtime::render_adapter::adapt_ui_draw_list,
};

const TEXT_CACHE_ENTRIES: usize = 1_024;
const TEXT_CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_PRESENTED_CHAT_ROWS: usize = 8;
const MAX_PRESENTED_CHAT_SUGGESTIONS: usize = 8;
const MAX_PRESENTED_TOAST_ROWS: usize = 8;
const MAX_PRESENTED_TEXT_BYTES: usize = 512;
const CHAT_TEXT_SCALE: f32 = 0.5;
const VANILLA_HUD_ATLAS_SIDE: u32 = 128;
const HUD_ATLAS_GUTTER: u32 = 1;
const VANILLA_SURVIVAL_POINTS: u16 = 20;
// Bedrock 1.26.33.1 resource_packs/vanilla/ui/hud_screen.json defines the
// 9px heart background at alpha 0.65. The exact rendered placement remains
// subject to the required native/live visual gate.
const VANILLA_HUD_BACKGROUND_ALPHA: u8 = 166;

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
        let (textures, solid_texture_page, hud_textures) =
            font_texture_array(&font, hud.as_deref())?;
        Ok(Self {
            font,
            textures: Arc::new(textures),
            solid_texture_page,
            hud_textures,
            layouts: TextLayoutCache::new(TEXT_CACHE_ENTRIES, TEXT_CACHE_BYTES),
            revision: 0,
            chat_hit_logical_size: None,
            chat_suggestion_hits: Vec::with_capacity(MAX_PRESENTED_CHAT_SUGGESTIONS),
        })
    }

    pub(super) fn hit_test_chat_suggestion(
        &self,
        position: UiPoint,
        logical_size: [f32; 2],
    ) -> Option<usize> {
        let expected = self.chat_hit_logical_size?;
        if expected.map(f32::to_bits) != logical_size.map(f32::to_bits) {
            return None;
        }
        self.chat_suggestion_hits
            .iter()
            .find_map(|(index, bounds)| bounds.contains(position).then_some(*index))
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
        let viewport = rect(0.0, 0.0, logical_width, logical_height)?;
        let wrap_width = ((logical_width * 0.45).clamp(1.0, 640.0) * 64.0) as u32;
        let chat_content_width = wrap_width as f32 / 64.0;
        let chat_left = 12.0_f32.min(logical_width);
        let chat_right = (chat_left + chat_content_width)
            .min(logical_width)
            .max(chat_left);
        let mut nodes = Vec::new();
        let mut next_id = 1u32;

        if let Some(hud_textures) = self.hud_textures.as_ref() {
            append_survival_hud(
                &mut nodes,
                &mut next_id,
                runtime,
                logical_width,
                logical_height,
                hud_textures,
            )?;
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
        let bottom_hud_top = (logical_height - 54.0).max(chat_region_top);
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

pub(super) fn visible_suggestion_range(total: usize, selected: Option<usize>) -> Range<usize> {
    let selected = selected.unwrap_or(0).min(total.saturating_sub(1));
    let end = total.min(
        selected
            .saturating_add(1)
            .max(MAX_PRESENTED_CHAT_SUGGESTIONS),
    );
    end.saturating_sub(MAX_PRESENTED_CHAT_SUGGESTIONS)..end
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

fn append_survival_hud(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    runtime: &UiRuntime,
    width: f32,
    height: f32,
    textures: &HudTexturePages,
) -> Result<(), UiPresentationError> {
    let left = ((width - 180.0) * 0.5).max(0.0);
    if let Some(health) = runtime.hud().health()
        && let Some(half_units) = standard_survival_half_units(health)
    {
        append_icon_row(
            nodes,
            next_id,
            textures,
            [left - 1.0, (height - 40.0).max(0.0)],
            false,
            half_units,
            HudTextureRole::HeartBackground,
            HudTextureRole::HeartFull,
            HudTextureRole::HeartHalf,
        )?;
    }
    if let Some(hunger) = runtime.hud().hunger()
        && let Some(half_units) = standard_survival_half_units(hunger)
    {
        append_icon_row(
            nodes,
            next_id,
            textures,
            [left + 180.0 - 9.0, (height - 40.0).max(0.0)],
            true,
            half_units,
            HudTextureRole::HungerBackground,
            HudTextureRole::HungerFull,
            HudTextureRole::HungerHalf,
        )?;
    }
    if let Some(armor) = runtime.hud().armor()
        && armor.current() > 0
        && let Some(half_units) = standard_survival_half_units(armor)
    {
        append_icon_row(
            nodes,
            next_id,
            textures,
            [left - 1.0, (height - 50.0).max(0.0)],
            false,
            half_units,
            HudTextureRole::ArmorEmpty,
            HudTextureRole::ArmorFull,
            HudTextureRole::ArmorHalf,
        )?;
    }
    if let Some(air) = runtime.hud().air()
        && air.current() < air.maximum()
    {
        let filled = u32::from(air.current())
            .saturating_mul(10)
            .div_ceil(u32::from(air.maximum()))
            .min(10) as usize;
        for index in 0..10 {
            let role = if index < filled {
                HudTextureRole::BubbleFull
            } else {
                HudTextureRole::BubbleEmpty
            };
            append_sprite(
                nodes,
                next_id,
                textures,
                role,
                [
                    left + 180.0 - 9.0 - index as f32 * 9.0,
                    (height - 50.0).max(0.0),
                ],
                [255; 4],
            )?;
        }
    }

    if let Some(equipment) = runtime.local_selected_equipment() {
        let hotbar_y = (height - 22.0).max(0.0);
        let roles = [
            HudTextureRole::Hotbar0,
            HudTextureRole::Hotbar1,
            HudTextureRole::Hotbar2,
            HudTextureRole::Hotbar3,
            HudTextureRole::Hotbar4,
            HudTextureRole::Hotbar5,
            HudTextureRole::Hotbar6,
            HudTextureRole::Hotbar7,
            HudTextureRole::Hotbar8,
        ];
        let selected = usize::from(equipment.event.selected_slot);
        if selected >= roles.len() {
            return Ok(());
        }
        for (index, role) in roles.into_iter().enumerate() {
            append_sprite(
                nodes,
                next_id,
                textures,
                role,
                [left + index as f32 * 20.0, hotbar_y],
                [255; 4],
            )?;
        }
        append_sprite(
            nodes,
            next_id,
            textures,
            HudTextureRole::SelectedHotbarSlot,
            [
                left + selected as f32 * 20.0 - 2.0,
                (height - 23.0).max(0.0),
            ],
            [255; 4],
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_icon_row(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    textures: &HudTexturePages,
    origin: [f32; 2],
    reverse: bool,
    half_units: u16,
    background: HudTextureRole,
    full: HudTextureRole,
    half: HudTextureRole,
) -> Result<(), UiPresentationError> {
    for index in 0..10 {
        let direction = if reverse { -1.0 } else { 1.0 };
        let position = [origin[0] + direction * index as f32 * 9.0, origin[1]];
        append_sprite(
            nodes,
            next_id,
            textures,
            background,
            position,
            [255, 255, 255, VANILLA_HUD_BACKGROUND_ALPHA],
        )?;
        let remaining = half_units.saturating_sub(index as u16 * 2);
        let foreground = if remaining >= 2 {
            Some(full)
        } else if remaining == 1 {
            Some(half)
        } else {
            None
        };
        if let Some(role) = foreground {
            append_sprite(nodes, next_id, textures, role, position, [255; 4])?;
        }
    }
    Ok(())
}

fn append_sprite(
    nodes: &mut Vec<UiNode>,
    next_id: &mut u32,
    textures: &HudTexturePages,
    role: HudTextureRole,
    position: [f32; 2],
    color: [u8; 4],
) -> Result<(), UiPresentationError> {
    let sprite = textures.sprite(role);
    let size = [f32::from(sprite.size[0]), f32::from(sprite.size[1])];
    nodes.push(
        UiNode::new(
            UiNodeId::new(*next_id),
            None,
            rect(
                position[0],
                position[1],
                position[0] + size[0],
                position[1] + size[1],
            )?,
        )
        .with_visual(UiVisual::Sprite {
            texture_page: textures.page,
            uv: sprite.uv,
            color,
        }),
    );
    *next_id = (*next_id).saturating_add(1);
    Ok(())
}

fn standard_survival_half_units(stat: BoundedStat) -> Option<u16> {
    let maximum = u32::from(stat.maximum());
    let scale = u32::from(stat.scale());
    if maximum != u32::from(VANILLA_SURVIVAL_POINTS) * scale {
        return None;
    }
    u16::try_from(u32::from(stat.current()).div_ceil(scale))
        .ok()
        .map(|value| value.min(VANILLA_SURVIVAL_POINTS))
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

fn font_texture_array(
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
mod tests {
    use assets::{
        FontTexturePage, GlyphMetrics, HudTexture, RuntimeFontCatalog, RuntimeHudCatalog,
        encode_font_catalog, encode_hud_catalog,
    };
    use protocol::{
        EquipmentEvent, HudEvent, NetworkItemStack, TextCategory, TextEvent, TextKind, UiEvent,
    };
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::ui_runtime::{SequencedLocalEquipment, SequencedUiEvent};

    #[test]
    fn vanilla_health_sprites_replace_corner_stat_text() {
        let font = fixture_font();
        let mut presentation = UiPresentationRuntime::with_hud(font, fixture_hud()).unwrap();
        let mut runtime = UiRuntime::new(1);
        runtime
            .apply(SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Hud(HudEvent::Health { health: 20 }),
            })
            .unwrap();

        let input = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();
        assert_eq!(input.textures.layers, 3);
        assert_eq!(input.vertices.len(), 20 * 4);
        assert_eq!(input.indices.len(), 20 * 6);
        assert_eq!(input.batches.len(), 1);
        assert_eq!(input.batches[0].texture_page, 2);
        let (top, bottom) = vertical_bounds(&input.vertices);
        assert_eq!([top, bottom], [560.0, 569.0]);
        let left = input
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::INFINITY, f32::min);
        let right = input
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!([left, right], [309.0, 399.0]);
        assert!(input.vertices.iter().any(|vertex| vertex.color[3] == 166));
        assert!(input.vertices.iter().any(|vertex| vertex.color[3] == 255));
        let mut scene = UiRenderScene::default();
        scene.publish(input, &UiRenderStats::default()).unwrap();
        assert!(scene.input.is_some());
    }

    #[test]
    fn missing_local_hud_carrier_never_falls_back_to_numeric_corner_text() {
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        let mut runtime = UiRuntime::new(1);
        runtime
            .apply(SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Hud(HudEvent::Health { health: 20 }),
            })
            .unwrap();

        let input = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();
        assert!(input.vertices.is_empty());
        assert!(input.indices.is_empty());
        assert!(input.batches.is_empty());
    }

    #[test]
    fn scaled_health_rounds_only_to_the_native_half_heart_presentation_unit() {
        assert_eq!(
            standard_survival_half_units(BoundedStat::new_scaled(1_750, 2_000, 100).unwrap()),
            Some(18)
        );
        assert_eq!(
            standard_survival_half_units(BoundedStat::new_scaled(1_701, 2_000, 100).unwrap()),
            Some(18)
        );
        assert_eq!(
            standard_survival_half_units(BoundedStat::new_scaled(2_100, 4_000, 100).unwrap()),
            None,
            "nonstandard maximum health needs the still-open multi-row parity contract"
        );
    }

    #[test]
    fn hud_atlas_extrudes_sprite_edges_into_one_texel_guard_band() {
        let font = fixture_font();
        let hud = fixture_hud();
        let (textures, _, hud_pages) = font_texture_array(&font, Some(&hud)).unwrap();
        let hud_pages = hud_pages.unwrap();
        let sprite = hud_pages.sprite(HudTextureRole::HeartFull);
        let [left, top, right, bottom] = sprite.uv.map(usize::from);
        let expected = [HudTextureRole::HeartFull as u8, 2, 3, 255];

        for position in [
            [left - 1, top],
            [right, top],
            [left, top - 1],
            [left, bottom],
        ] {
            assert_eq!(
                atlas_pixel(&textures, hud_pages.page, position),
                expected,
                "HUD sprite guard texel at {position:?} did not repeat the edge"
            );
        }
    }

    #[test]
    fn focused_chat_editor_history_and_suggestions_are_presented() {
        let font = fixture_font();
        let font_page_count = u32::try_from(font.pages().len()).unwrap();
        let mut presentation = UiPresentationRuntime::new(font).unwrap();
        let mut runtime = UiRuntime::new(1);
        let empty = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();
        assert!(
            empty
                .batches
                .iter()
                .all(|batch| batch.texture_page != font_page_count)
        );
        runtime
            .apply(SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: chat_event(&"history ".repeat(12)),
            })
            .unwrap();
        runtime.open_chat();
        runtime.insert_chat_text("/g").unwrap();
        let autocomplete_request = runtime.take_chat_autocomplete_request().unwrap();
        runtime
            .apply(SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 2,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::ChatAutocomplete(protocol::ChatAutocompleteEvent {
                    enum_name: Arc::from("commands"),
                    action: protocol::ChatAutocompleteAction::Replace,
                    suggestions: Arc::from(
                        (0..MAX_PRESENTED_CHAT_SUGGESTIONS)
                            .map(|index| Arc::from(format!("/give-{index}")))
                            .collect::<Vec<_>>(),
                    ),
                }),
            })
            .unwrap();
        assert!(runtime.complete_chat_autocomplete(autocomplete_request));

        let active = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();

        assert!(active.vertices.len() > empty.vertices.len());
        assert!(active.indices.len() > empty.indices.len());
        assert_eq!(
            active.batches.first().map(|batch| batch.texture_page),
            Some(font_page_count),
            "the translucent surface must draw behind history and suggestion text"
        );
    }

    #[test]
    fn focused_chat_editor_uses_a_dedicated_solid_panel_layer() {
        let font = fixture_font();
        let font_page_count = u32::try_from(font.pages().len()).unwrap();
        let mut presentation = UiPresentationRuntime::new(font).unwrap();
        let mut runtime = UiRuntime::new(1);
        runtime.open_chat();
        runtime.insert_chat_text("hello").unwrap();

        let active = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();

        assert_eq!(active.textures.layers, font_page_count + 1);
        let panel_batch = active
            .batches
            .iter()
            .find(|batch| batch.texture_page == font_page_count)
            .expect("focused chat must draw through the dedicated solid panel layer");
        let panel_indices = &active.indices[panel_batch.first_index as usize
            ..(panel_batch.first_index + panel_batch.index_count) as usize];
        let panel_vertices = panel_indices
            .iter()
            .map(|index| active.vertices[*index as usize])
            .collect::<Vec<_>>();
        let layer_bytes = active.textures.width as usize
            * active.textures.height as usize
            * std::mem::size_of::<[u8; 4]>();
        let solid_start = font_page_count as usize * layer_bytes;
        assert!(
            active.textures.rgba8[solid_start..solid_start + layer_bytes]
                .iter()
                .all(|byte| *byte == 255)
        );
        assert!(
            panel_vertices
                .iter()
                .all(|vertex| vertex.color == [0, 0, 0, 176])
        );
        assert!(panel_vertices.iter().any(|vertex| vertex.color[3] >= 128));
        assert!(
            panel_vertices
                .iter()
                .any(|vertex| vertex.position[0] <= 8.0)
        );
        assert!(
            panel_vertices
                .iter()
                .any(|vertex| vertex.position[0] >= 370.0)
        );
        assert!(
            panel_vertices
                .iter()
                .all(|vertex| vertex.position[1] <= 558.0)
        );
        assert!(
            panel_vertices
                .iter()
                .all(|vertex| vertex.position[1] >= 500.0)
        );
    }

    #[test]
    fn focused_chat_uses_compact_java_style_text_and_does_not_dim_the_hud() {
        let font = fixture_font();
        let solid_page = u32::try_from(font.pages().len()).unwrap();
        let mut presentation = UiPresentationRuntime::new(font).unwrap();
        let mut runtime = UiRuntime::new(1);
        runtime
            .apply(SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: chat_event("compact history"),
            })
            .unwrap();
        runtime.open_chat();

        let active = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();
        let panel = active
            .batches
            .iter()
            .find(|batch| batch.texture_page == solid_page)
            .unwrap();
        let panel_vertices = active.indices
            [panel.first_index as usize..(panel.first_index + panel.index_count) as usize]
            .iter()
            .map(|index| active.vertices[*index as usize])
            .collect::<Vec<_>>();
        let (panel_top, panel_bottom) = vertical_bounds(&panel_vertices);
        assert!(panel_bottom <= 558.0, "panel dimmed the bottom HUD band");
        assert!(
            panel_bottom - panel_top <= 60.0,
            "single-row chat used an oversized {panel_top}..{panel_bottom} surface"
        );

        for glyph in active.vertices[4..].chunks_exact(4) {
            let (top, bottom) = vertical_bounds(glyph);
            assert!(
                bottom - top <= 12.0,
                "chat glyph exceeded the approved compact scale: {top}..{bottom}"
            );
        }
    }

    #[test]
    fn wrapped_chat_messages_reserve_their_full_visual_height() {
        let font = fixture_font();
        let mut presentation = UiPresentationRuntime::new(font).unwrap();
        let mut runtime = UiRuntime::new(1);
        let first = "a".repeat(70);
        let second = "b".repeat(70);
        for (fifo_sequence, message) in [(1, first.as_str()), (2, second.as_str())] {
            runtime
                .apply(SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence,
                    local_millis: 0,
                    server_tick: None,
                    event: chat_event(message),
                })
                .unwrap();
        }

        let active = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();
        let first_vertex_count = first.chars().count() * 4;
        let first_bottom = active.vertices[..first_vertex_count]
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::NEG_INFINITY, f32::max);
        let second_top = active.vertices[first_vertex_count..]
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::INFINITY, f32::min);

        assert!(
            first_bottom <= second_top,
            "wrapped chat rows overlap: first bottom {first_bottom}, second top {second_top}"
        );
    }

    #[test]
    fn chat_surface_uses_bounded_width_across_resize_and_dpi() {
        let font = fixture_font();
        let solid_page = u32::try_from(font.pages().len()).unwrap();
        let mut presentation = UiPresentationRuntime::new(font).unwrap();
        let mut runtime = UiRuntime::new(1);
        runtime.open_chat();

        for (physical_size, dpi, maximum_panel_right) in [
            ([3_840, 1_080], 1.0, 656.0),
            ([1_600, 1_200], 2.0, 752.0),
            ([320, 200], 1.0, 320.0),
        ] {
            let active = presentation
                .build(&runtime, 0, physical_size, DpiScale::new(dpi).unwrap())
                .unwrap();
            let panel = active
                .batches
                .iter()
                .find(|batch| batch.texture_page == solid_page)
                .unwrap();
            let right = active.indices
                [panel.first_index as usize..(panel.first_index + panel.index_count) as usize]
                .iter()
                .map(|index| active.vertices[*index as usize].position[0])
                .fold(f32::NEG_INFINITY, f32::max);
            assert!(right <= maximum_panel_right, "panel right edge was {right}");
        }
    }

    #[test]
    fn focused_chat_editor_does_not_overlap_survival_hud_sprites() {
        let font = fixture_font();
        let mut presentation = UiPresentationRuntime::with_hud(font, fixture_hud()).unwrap();
        let mut runtime = UiRuntime::new(1);
        runtime
            .apply(SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Hud(HudEvent::Health { health: 20 }),
            })
            .unwrap();
        runtime.open_chat();

        let active = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();
        let hud_vertices = &active.vertices[..20 * 4];
        let editor_vertex_count = "> |".chars().count() * 4;
        let editor_vertices = &active.vertices[active.vertices.len() - editor_vertex_count..];
        let hud_top = hud_vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::INFINITY, f32::min);
        let editor_bottom = editor_vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::NEG_INFINITY, f32::max);

        assert!(
            editor_bottom <= hud_top,
            "chat editor overlaps survival HUD: editor bottom {editor_bottom}, HUD top {hud_top}"
        );
    }

    #[test]
    fn selected_hotbar_slot_uses_native_slot_geometry_when_authority_is_known() {
        let mut presentation =
            UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
        let mut runtime = UiRuntime::new(1);
        runtime.local_selected_equipment = Some(SequencedLocalEquipment {
            session_id: 1,
            fifo_sequence: 7,
            event: EquipmentEvent {
                actor_runtime_id: 42,
                stack: NetworkItemStack::empty(),
                inventory_slot: 4,
                selected_slot: 4,
                window_id: 0,
                handedness: None,
            },
        });

        let active = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();
        assert_eq!(active.vertices.len(), 10 * 4);
        assert_eq!(active.batches.len(), 1);
        assert_eq!(active.batches[0].texture_page, 2);
        let selected = &active.vertices[9 * 4..10 * 4];
        let (top, bottom) = vertical_bounds(selected);
        assert_eq!([top, bottom], [577.0, 601.0]);
        let left = selected
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::INFINITY, f32::min);
        let right = selected
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!([left, right], [388.0, 412.0]);
    }

    #[test]
    fn invalid_authoritative_hotbar_slot_fails_closed_without_partial_art() {
        let mut presentation =
            UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
        let mut runtime = UiRuntime::new(1);
        runtime.local_selected_equipment = Some(SequencedLocalEquipment {
            session_id: 1,
            fifo_sequence: 7,
            event: EquipmentEvent {
                actor_runtime_id: 42,
                stack: NetworkItemStack::empty(),
                inventory_slot: 9,
                selected_slot: 9,
                window_id: 0,
                handedness: None,
            },
        });

        let active = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();

        assert!(active.vertices.is_empty());
        assert!(active.indices.is_empty());
        assert!(active.batches.is_empty());
    }

    #[test]
    fn autocomplete_rows_reserve_actual_text_height_above_editor_and_history() {
        let font = fixture_font();
        let mut presentation = UiPresentationRuntime::new(font).unwrap();
        let mut runtime = UiRuntime::new(1);
        let history = "history ".repeat(12);
        runtime
            .apply(SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: chat_event(&history),
            })
            .unwrap();
        runtime.open_chat();
        runtime.insert_chat_text("/g").unwrap();
        let autocomplete_request = runtime.take_chat_autocomplete_request().unwrap();
        runtime
            .apply(SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 2,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::ChatAutocomplete(protocol::ChatAutocompleteEvent {
                    enum_name: Arc::from("commands"),
                    action: protocol::ChatAutocompleteAction::Replace,
                    suggestions: Arc::from(
                        (0..MAX_PRESENTED_CHAT_SUGGESTIONS)
                            .map(|index| Arc::from(format!("/give-{index}")))
                            .collect::<Vec<_>>(),
                    ),
                }),
            })
            .unwrap();
        assert!(runtime.complete_chat_autocomplete(autocomplete_request));

        let active = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();
        let panel_vertices = 4;
        let history_vertices = history.chars().count() * 4;
        let editor_vertices = "> /g|".chars().count() * 4;
        let suggestion_vertices = "> /give-0".chars().count() * 4;
        let history_bounds =
            vertical_bounds(&active.vertices[panel_vertices..panel_vertices + history_vertices]);
        let editor_start = panel_vertices + history_vertices;
        let editor_bounds =
            vertical_bounds(&active.vertices[editor_start..editor_start + editor_vertices]);
        let first_suggestion_start = editor_start + editor_vertices;
        let first_suggestion_bounds = vertical_bounds(
            &active.vertices[first_suggestion_start..first_suggestion_start + suggestion_vertices],
        );
        let second_suggestion_bounds = vertical_bounds(
            &active.vertices[first_suggestion_start + suggestion_vertices
                ..first_suggestion_start + suggestion_vertices * 2],
        );

        assert!(first_suggestion_bounds.1 <= editor_bounds.0);
        assert!(second_suggestion_bounds.1 <= first_suggestion_bounds.0);
        let topmost_start =
            first_suggestion_start + suggestion_vertices * (MAX_PRESENTED_CHAT_SUGGESTIONS - 1);
        let topmost_bounds =
            vertical_bounds(&active.vertices[topmost_start..topmost_start + suggestion_vertices]);
        assert!(history_bounds.1 <= topmost_bounds.0);
    }

    #[test]
    fn oversized_latest_chat_message_keeps_a_bounded_visible_portion() {
        let font = fixture_font();
        let mut presentation = UiPresentationRuntime::new(font).unwrap();
        let mut runtime = UiRuntime::new(1);
        runtime
            .apply(SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: chat_event(&"latest ".repeat(60)),
            })
            .unwrap();

        let active = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();
        assert!(!active.vertices.is_empty());
        assert!(
            active
                .vertices
                .iter()
                .all(|vertex| vertex.position[1] >= 380.0 && vertex.position[1] <= 528.0),
            "oversized message escaped bounded presentation region"
        );
    }

    #[test]
    fn maximum_page_font_is_rejected_before_appending_the_solid_layer() {
        let font = fixture_font_with_page_count(MAX_UI_TEXTURE_LAYERS as usize);
        assert!(matches!(
            UiPresentationRuntime::new(font),
            Err(UiPresentationError::InvalidFontTexture)
        ));
    }

    #[test]
    fn hud_layer_is_rejected_when_it_would_exceed_the_texture_layer_limit() {
        let font = fixture_font_with_page_count((MAX_UI_TEXTURE_LAYERS - 1) as usize);
        assert!(matches!(
            UiPresentationRuntime::with_hud(font, fixture_hud()),
            Err(UiPresentationError::InvalidFontTexture)
        ));
    }

    #[test]
    fn suggestion_window_keeps_the_selected_row_visible() {
        assert_eq!(visible_suggestion_range(20, Some(12)), 5..13);
        assert_eq!(visible_suggestion_range(20, Some(19)), 12..20);
        assert_eq!(visible_suggestion_range(3, Some(2)), 0..3);
    }

    #[test]
    fn suggestion_hit_testing_uses_the_exact_rendered_rows_and_width_cap() {
        let font = fixture_font();
        let mut presentation = UiPresentationRuntime::new(font).unwrap();
        let mut runtime = UiRuntime::new(1);
        runtime.open_chat();
        runtime
            .apply(SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::ChatAutocomplete(protocol::ChatAutocompleteEvent {
                    enum_name: Arc::from("commands"),
                    action: protocol::ChatAutocompleteAction::Replace,
                    suggestions: Arc::from(
                        (0..12)
                            .map(|index| Arc::from(format!("/s{index}")))
                            .collect::<Vec<_>>(),
                    ),
                }),
            })
            .unwrap();
        runtime.insert_chat_text("/").unwrap();
        assert!(runtime.service_pending_chat_autocomplete());
        for _ in 0..10 {
            runtime.handle_chat_ui_action(ui::UiAction::Navigate([0, 1]));
        }

        let active = presentation
            .build(&runtime, 0, [800, 600], DpiScale::new(1.0).unwrap())
            .unwrap();
        assert_eq!(
            presentation
                .chat_suggestion_hits
                .iter()
                .map(|(index, _)| *index)
                .collect::<Vec<_>>(),
            (3..11).collect::<Vec<_>>()
        );
        for (index, bounds) in &presentation.chat_suggestion_hits {
            let center = UiPoint::new(
                (bounds.min().x() + bounds.max().x()) * 0.5,
                (bounds.min().y() + bounds.max().y()) * 0.5,
            )
            .unwrap();
            assert_eq!(
                presentation.hit_test_chat_suggestion(center, [800.0, 600.0]),
                Some(*index)
            );
            assert!(active.vertices[4..].iter().any(|vertex| {
                bounds.contains(UiPoint::new(vertex.position[0], vertex.position[1]).unwrap())
            }));
        }
        assert_eq!(
            presentation
                .hit_test_chat_suggestion(UiPoint::new(20.0, 533.0).unwrap(), [800.0, 600.0],),
            Some(3),
            "the old synthetic hit test incorrectly selected row 4 here"
        );

        presentation
            .build(&runtime, 0, [3_840, 1_080], DpiScale::new(1.0).unwrap())
            .unwrap();
        let row_center_y = presentation.chat_suggestion_hits[0].1.min().y()
            + presentation.chat_suggestion_hits[0].1.height() * 0.5;
        assert_eq!(
            presentation.hit_test_chat_suggestion(
                UiPoint::new(1_000.0, row_center_y).unwrap(),
                [3_840.0, 1_080.0],
            ),
            None,
            "ultrawide hit testing escaped the rendered 640px chat cap"
        );
        assert_eq!(
            presentation.hit_test_chat_suggestion(
                UiPoint::new(20.0, row_center_y).unwrap(),
                [800.0, 600.0],
            ),
            None,
            "stale viewport geometry must fail closed"
        );
    }

    fn chat_event(message: &str) -> UiEvent {
        UiEvent::Text(TextEvent {
            category: TextCategory::MessageOnly,
            kind: TextKind::Chat,
            needs_translation: false,
            source: None,
            message: Arc::from(message),
            parameters: Arc::from([]),
            xuid: Arc::from(""),
            platform_chat_id: Arc::from(""),
            filtered_message: None,
        })
    }

    fn vertical_bounds(vertices: &[render::UiRenderVertex]) -> (f32, f32) {
        vertices.iter().fold(
            (f32::INFINITY, f32::NEG_INFINITY),
            |(top, bottom), vertex| (top.min(vertex.position[1]), bottom.max(vertex.position[1])),
        )
    }

    fn fixture_font_with_page_count(page_count: usize) -> Arc<RuntimeFontCatalog> {
        let pages = (0..page_count)
            .map(|index| {
                let pixels = vec![index as u8, (index >> 8) as u8, 255, 255].into_boxed_slice();
                let mut source_sha256 = [1; 32];
                source_sha256[..8].copy_from_slice(&(index as u64).to_le_bytes());
                FontTexturePage {
                    source_path: format!("font/page-{index:03}.png").into(),
                    source_bytes: 4,
                    source_sha256,
                    pixels_sha256: Sha256::digest(&pixels).into(),
                    width: 1,
                    height: 1,
                    rgba8: pixels,
                }
            })
            .collect::<Vec<_>>();
        let glyph = GlyphMetrics {
            codepoint: '\u{fffd}',
            page: 0,
            uv: [0, 0, 1, 1],
            bearing: [0, 0],
            advance_64: 64,
        };
        let manifest = [9; 32];
        let bytes = encode_font_catalog(manifest, &[glyph], &pages).unwrap();
        Arc::new(RuntimeFontCatalog::decode(&bytes, manifest).unwrap())
    }

    fn atlas_pixel(textures: &UiRenderTextureArray, page: u16, position: [usize; 2]) -> [u8; 4] {
        let layer_bytes = textures.width as usize * textures.height as usize * 4;
        let offset = usize::from(page) * layer_bytes
            + (position[1] * textures.width as usize + position[0]) * 4;
        textures.rgba8[offset..offset + 4].try_into().unwrap()
    }

    fn fixture_hud() -> Arc<RuntimeHudCatalog> {
        let textures = HudTextureRole::ALL
            .into_iter()
            .map(|role| {
                let [width, height] = role.expected_size();
                let rgba8 = [role as u8, 2, 3, 255]
                    .repeat(width as usize * height as usize)
                    .into_boxed_slice();
                HudTexture {
                    role,
                    source_bytes: rgba8.len() as u32,
                    source_sha256: Sha256::digest(&rgba8).into(),
                    pixels_sha256: Sha256::digest(&rgba8).into(),
                    width,
                    height,
                    rgba8,
                }
            })
            .collect::<Vec<_>>();
        let manifest = [0x81; 32];
        let bytes = encode_hud_catalog(manifest, &textures).unwrap();
        Arc::new(RuntimeHudCatalog::decode(&bytes).unwrap())
    }

    fn fixture_font() -> Arc<RuntimeFontCatalog> {
        let pixels = vec![255; 16 * 24 * 4].into_boxed_slice();
        let page = FontTexturePage {
            source_path: "font/page.png".into(),
            source_bytes: pixels.len() as u32,
            source_sha256: [1; 32],
            pixels_sha256: Sha256::digest(&pixels).into(),
            width: 16,
            height: 24,
            rgba8: pixels,
        };
        let glyphs = ['/', '0', '2', '\u{fffd}'].map(|codepoint| GlyphMetrics {
            codepoint,
            page: 0,
            uv: [0, 0, 16, 24],
            bearing: [0, 0],
            advance_64: 512,
        });
        let manifest = [7; 32];
        let bytes = encode_font_catalog(manifest, &glyphs, &[page]).unwrap();
        Arc::new(RuntimeFontCatalog::decode(&bytes, manifest).unwrap())
    }
}
