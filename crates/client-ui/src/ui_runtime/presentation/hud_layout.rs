//! Java-reference gameplay HUD layout.
//!
//! Geometry, ordering, and visibility follow the approved clean-room Java
//! Edition 26.2 default-resource presentation, expressed in GUI pixels and
//! scaled by the Java GUI-scale rule. Every value is observable behavior
//! (positions, sizes, timings) recorded from a legally obtained client;
//! Bedrock remains authoritative for all state. Timings that Java derives
//! from internal counters are pinned here as milliseconds and called out as
//! bounded approximations pending the native comparison gallery.

use std::sync::Arc;

use assets::{HudTextureRole, RuntimeFontCatalog};
use ui::{
    SafeArea, TextLayoutCache, TextLayoutRequest, TextShadow, TextStyle, UiNode, UiNodeId, UiScale,
    UiVisual,
};

use super::{HudTexturePages, IconRef, UiPresentationError, UiRuntime, rect};

mod inventory;
mod pinned;
mod player;
mod reader;
mod recipe_book;
mod sleep;
mod status_motion;
mod status_rows;
pub(super) use status_rows::{HeartPaint, HungerPaint, capture as capture_hud_paint};
mod windows;

pub(super) use inventory::{CraftingFrame, StorageIcons};
pub use pinned::{BOSS_TINTS, effect_icon_role, gui_scale};
use pinned::{BOTTOM_STACK_HEIGHT, HOTBAR_WIDTH, hsv_to_rgb};
pub use sleep::SleepTimeline;
pub(super) use windows::{Durability, TooltipLine, WindowIcons, WindowText, title_key};

#[derive(Clone, Debug)]
pub struct InventoryIcons(pub [Option<IconRef>; 36]);

impl Default for InventoryIcons {
    fn default() -> Self {
        Self([None; 36])
    }
}

/// Frame inputs the layout cannot read from `UiRuntime` alone: camera state
/// plus item facts resolved against the world stream's authoritative item
/// registry immediately before presentation.
#[derive(Clone, Debug, Default)]
pub struct HudFrame {
    pub now_millis: u64,
    /// The crosshair is a first-person-only surface in the reference.
    pub first_person: bool,
    /// Authoritative `(current, maximum)` health of the ridden actor.
    pub mount_health: Option<(f32, f32)>,
    pub hotbar_durability: [Option<f32>; 9],
    pub offhand_durability: Option<f32>,
    /// Exact stack state published for each occupied hotbar cell this frame.
    pub hotbar_stacks: [Option<protocol::NetworkItemStack>; 9],
    /// Item icons resolved from the authoritative registry this frame.
    pub hotbar_icons: [Option<IconRef>; 9],
    pub inventory_icons: InventoryIcons,
    pub storage_icons: StorageIcons,
    pub crafting: CraftingFrame,
    pub window_icons: WindowIcons,
    pub durability: Durability,
    pub window_text: WindowText,
    pub cursor_icon: Option<IconRef>,
    pub armor_icons: [Option<IconRef>; 4],
    pub offhand_icon: Option<IconRef>,
    /// Geometry-rendered offhand carrier; the original icon remains reserved
    /// for the compact hotbar slot.
    pub offhand_viewmodel_icon: Option<IconRef>,
    /// The selected main-hand item rendered in the first-person view. The
    /// world renderer does not yet own an item model, so this is the
    /// nearest-neighbour item carrier used by the compatibility viewmodel.
    pub held_item_icon: Option<IconRef>,
    /// Cached software-rendered 3-D local avatar shown by the personal
    /// inventory screen.
    pub player_preview: Option<IconRef>,
    /// The native HUD hold timer is active. Settings are applied during binding.
    pub paper_doll_visible: bool,
    /// Skin-backed first-person arm carriers. These are separate from the
    /// item atlas so an empty hand still has the same silhouette as the
    /// player's authoritative skin.
    pub left_hand: Option<IconRef>,
    pub right_hand: Option<IconRef>,
    /// The near-camera rig owns the first-person hand, so the CPU hand/item carriers stay undrawn.
    pub hand_rig_active: bool,
    /// Local actor pitch used by the compatibility viewmodel to keep the
    /// hand/item carrier aligned with the camera-facing native path.
    pub viewmodel_pitch_degrees: f32,
    /// Presented name of the selected stack, resolved this frame.
    pub selected_item_name: Option<std::sync::Arc<str>>,
    /// Jump charge in `0.0..=1.0` while riding a jump-capable mount: the
    /// mount jump bar replaces the experience bar for the ride's duration.
    pub mount_jump: Option<f32>,
    /// Melee charge in `0.0..=1.0`. Bedrock exposes no attack-cooldown
    /// state, so the production authority pins this at exactly 1.0 (always
    /// ready); the reference hides the indicator at full charge, so it
    /// draws only for sub-full values.
    pub attack_indicator_charge: Option<f32>,
    /// The local player's floored feet position.
    pub player_block: Option<[i32; 3]>,
    /// The absolute world tick, when the session has a clock.
    pub world_time: Option<f64>,
    /// A held filled map shows the position whatever the world rule says.
    pub holding_filled_map: bool,
    /// Lightning is falling: the bed screen talks of a thunderstorm.
    pub thunderstorm: bool,
    /// The stream's current dimension, which picks the loading backdrop.
    pub dimension: i32,
    pub sleep: SleepTimeline,
    pub engine_containers: bool, // container screens draw through JSON-UI instead
    pub item_names: std::collections::HashMap<(i32, u32), std::sync::Arc<str>>, // tooltip names
}

/// Per-frame layout geometry derived from the Java GUI-scale rule. All
/// emitted coordinates are relative to the safe content rect: the retained
/// tree translates root nodes by the safe-area origin during layout.
#[derive(Clone, Copy)]
pub(super) struct HudGeometry {
    /// Logical pixels per GUI pixel.
    pub scale: f32,
    /// Viewport in GUI px, inset by the safe area.
    pub gui_width: f32,
    pub gui_height: f32,
}

impl HudGeometry {
    pub(super) fn new(
        physical_size: [u32; 2],
        dpi_scale: f32,
        safe_area: SafeArea,
        preference: Option<u8>,
    ) -> Option<Self> {
        if physical_size.contains(&0) || !dpi_scale.is_finite() || dpi_scale <= 0.0 {
            return None;
        }
        let k = gui_scale(physical_size, preference) as f32;
        let scale = k / dpi_scale;
        let logical_width = physical_size[0] as f32 / dpi_scale;
        let logical_height = physical_size[1] as f32 / dpi_scale;
        let inner_width = logical_width - safe_area.left() - safe_area.right();
        let inner_height = logical_height - safe_area.top() - safe_area.bottom();
        let gui_width = inner_width / scale;
        let gui_height = inner_height / scale;
        // Fail closed when the safe viewport cannot contain the fixed-width
        // hotbar or the fixed-height bottom stack: an inset or short viewport
        // renders no gameplay HUD rather than a clipped one.
        if !(gui_width.is_finite() && gui_height.is_finite())
            || gui_width < HOTBAR_WIDTH
            || gui_height < BOTTOM_STACK_HEIGHT
        {
            return None;
        }
        Some(Self {
            scale,
            gui_width,
            gui_height,
        })
    }

    fn logical(&self, gui: [f32; 2]) -> [f32; 2] {
        [gui[0] * self.scale, gui[1] * self.scale]
    }
}

pub(super) struct HudLayout<'a> {
    pub nodes: &'a mut Vec<UiNode>,
    pub next_id: &'a mut u32,
    pub textures: &'a HudTexturePages,
    pub layouts: &'a mut TextLayoutCache,
    pub font: &'a Arc<RuntimeFontCatalog>,
    pub solid_page: u16,
    pub geometry: HudGeometry,
    /// Logical height of one text line at `UiScale` 1.0, measured once per
    /// frame so text tracks the GUI scale.
    text_line_logical: f32,
}

impl<'a> HudLayout<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        nodes: &'a mut Vec<UiNode>,
        next_id: &'a mut u32,
        textures: &'a HudTexturePages,
        layouts: &'a mut TextLayoutCache,
        font: &'a Arc<RuntimeFontCatalog>,
        solid_page: u16,
        geometry: HudGeometry,
    ) -> Result<Self, UiPresentationError> {
        let probe = layouts
            .layout(TextLayoutRequest {
                text: "0",
                style: TextStyle::default(),
                width_64: 64 * 64,
                line_height_64: super::TEXT_LINE_HEIGHT_64,
                baseline_64: super::TEXT_BASELINE_64,
                scale: UiScale::default(),
                font,
                wrap: Default::default(),
            })
            .map_err(UiPresentationError::Text)?;
        let text_line_logical = (probe.size_64()[1] as f32 / 64.0).max(1.0);
        Ok(Self {
            nodes,
            next_id,
            textures,
            layouts,
            font,
            solid_page,
            geometry,
            text_line_logical,
        })
    }

    /// The Java-styled surfaces outside the engine HUD: the legacy inventory
    /// screens for a container scene, else the sleep overlay and first-person hands.
    pub(super) fn append(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
        container: bool,
    ) -> Result<(), UiPresentationError> {
        if container {
            self.inventory_screen(player_runtime, runtime, frame)?;
            return Ok(());
        }
        self.sleep_overlay(frame)?;
        let mode_allows_hotbar = player_runtime
            .facts
            .player_game_mode()
            .is_none_or(|mode| mode.shows_hotbar());
        if frame.first_person && mode_allows_hotbar {
            self.held_items(frame)?;
        }
        Ok(())
    }

    /// Item carriers may contain a small number of larger source rasters, but
    /// the gameplay hotbar presents every icon in the fixed 16x16 Java cell.
    fn icon_gui(&mut self, icon: IconRef, gui: [f32; 2]) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let [x, y] = g.logical(gui);
        let node = UiNode::new(
            UiNodeId::new(*self.next_id),
            None,
            rect(x, y, x + 16.0 * g.scale, y + 16.0 * g.scale)?,
        )
        .with_visual(icon.visual([255; 4]));
        self.nodes.push(node);
        *self.next_id = self.next_id.saturating_add(1);
        Ok(())
    }

    /// Count text and durability bar for one 16x16 item cell.
    fn stack_decorations(
        &mut self,
        stack: &protocol::NetworkItemStack,
        cell: [f32; 2],
        durability: Option<f32>,
    ) -> Result<(), UiPresentationError> {
        if let Some(fraction) = durability {
            // 13x2 GUI-px bar: dark track, hue sweeping green->red with wear.
            let bar_left = cell[0] + 2.0;
            let bar_top = cell[1] + 13.0;
            self.solid_gui([bar_left, bar_top], [13.0, 2.0], [0, 0, 0, 255])?;
            let width = (fraction * 13.0).round().clamp(0.0, 13.0);
            if width > 0.0 {
                let hue = fraction / 3.0;
                self.solid_gui([bar_left, bar_top], [width, 1.0], hsv_to_rgb(hue))?;
            }
        }
        if stack.count > 1 {
            let text = stack.count.to_string();
            let scale = self.text_scale(9.0);
            let layout = self
                .layouts
                .layout(TextLayoutRequest {
                    text: &text,
                    style: TextStyle::default(),
                    width_64: (64.0 * 64.0) as u32,
                    line_height_64: super::TEXT_LINE_HEIGHT_64,
                    baseline_64: super::TEXT_BASELINE_64,
                    scale,
                    font: self.font,
                    wrap: Default::default(),
                })
                .map_err(UiPresentationError::Text)?;
            let size = [
                layout.size_64()[0] as f32 / 64.0 / self.geometry.scale,
                layout.size_64()[1] as f32 / 64.0 / self.geometry.scale,
            ];
            // Bottom-right corner of the cell, shadowed.
            let position = [cell[0] + 17.0 - size[0], cell[1] + 17.0 - size[1]];
            self.text_gui_shadowed(layout, position, [255; 4])?;
        }
        Ok(())
    }

    fn text_scale(&self, gui_px: f32) -> UiScale {
        let target_logical = gui_px * self.geometry.scale;
        let ratio = (target_logical / self.text_line_logical)
            .clamp(UiScale::DISPLAY_MIN, UiScale::DISPLAY_MAX);
        UiScale::new_display(ratio).unwrap_or_default()
    }

    fn sprite_gui(
        &mut self,
        role: HudTextureRole,
        gui: [f32; 2],
        color: [u8; 4],
    ) -> Result<(), UiPresentationError> {
        let sprite = self.textures.sprite(role);
        let g = self.geometry;
        let [x, y] = g.logical(gui);
        let node = UiNode::new(
            UiNodeId::new(*self.next_id),
            None,
            rect(
                x,
                y,
                x + f32::from(sprite.size[0]) * g.scale,
                y + f32::from(sprite.size[1]) * g.scale,
            )?,
        )
        .with_visual(UiVisual::Sprite {
            texture_page: self.textures.page,
            uv: sprite.uv,
            color,
        });
        self.nodes.push(node);
        *self.next_id = self.next_id.saturating_add(1);
        Ok(())
    }

    fn solid_gui(
        &mut self,
        gui: [f32; 2],
        size: [f32; 2],
        color: [u8; 4],
    ) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let [x, y] = g.logical(gui);
        let node = UiNode::new(
            UiNodeId::new(*self.next_id),
            None,
            rect(x, y, x + size[0] * g.scale, y + size[1] * g.scale)?,
        )
        .with_visual(UiVisual::Solid {
            texture_page: self.solid_page,
            color,
        });
        self.nodes.push(node);
        *self.next_id = self.next_id.saturating_add(1);
        Ok(())
    }

    fn text_gui(
        &mut self,
        layout: Arc<ui::TextLayout>,
        gui: [f32; 2],
        color: [u8; 4],
    ) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let [x, y] = g.logical(gui);
        let width = layout.size_64()[0] as f32 / 64.0;
        let height = layout.size_64()[1] as f32 / 64.0;
        let node = UiNode::new(
            UiNodeId::new(*self.next_id),
            None,
            rect(x, y, x + width, y + height)?,
        )
        .with_visual(UiVisual::Text {
            layout,
            color,
            shadow: TextShadow::None,
        });
        self.nodes.push(node);
        *self.next_id = self.next_id.saturating_add(1);
        Ok(())
    }

    /// Reference HUD text carries a one-GUI-px drop shadow.
    fn text_gui_shadowed(
        &mut self,
        layout: Arc<ui::TextLayout>,
        gui: [f32; 2],
        color: [u8; 4],
    ) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let [x, y] = g.logical(gui);
        let width = layout.size_64()[0] as f32 / 64.0;
        let height = layout.size_64()[1] as f32 / 64.0;
        let node = UiNode::new(
            UiNodeId::new(*self.next_id),
            None,
            rect(x, y, x + width, y + height)?,
        )
        .with_visual(UiVisual::Text {
            layout,
            color,
            shadow: TextShadow::Offset64(super::TEXT_SHADOW_OFFSET_64),
        });
        self.nodes.push(node);
        *self.next_id = self.next_id.saturating_add(1);
        Ok(())
    }
}
