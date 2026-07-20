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
    SafeArea, TextLayoutCache, TextLayoutRequest, TextStyle, UiNode, UiNodeId, UiScale, UiVisual,
};

use super::{HudSprite, HudTexturePages, UiPresentationError, UiRuntime, rect};
use crate::ui_runtime::gameplay_hud::HudEffect;

mod pinned;

use pinned::{
    BOSS_TINTS, BOTTOM_STACK_HEIGHT, HARMFUL_EFFECT_IDS, HOTBAR_CAP_ALPHA, HOTBAR_WIDTH,
    LABEL_FADE_MILLIS, LABEL_WINDOW_MILLIS, MAX_HEART_ROWS, MAX_MOUNT_HEARTS,
    MAX_PRESENTED_BOSS_BARS, XP_LEVEL_COLOR, damage_flash_phase, effect_blink_alpha, heart_role,
    hotbar_slot_role, hsv_to_rgb,
};
pub(crate) use pinned::{effect_icon_role, java_gui_scale};

/// Frame inputs the layout cannot read from `UiRuntime` alone: camera state
/// plus item facts resolved against the world stream's authoritative item
/// registry immediately before presentation.
#[derive(Clone, Debug, Default)]
pub(crate) struct HudFrame {
    pub now_millis: u64,
    /// The crosshair is a first-person-only surface in the reference.
    pub first_person: bool,
    /// Authoritative `(current, maximum)` health of the ridden actor.
    pub mount_health: Option<(f32, f32)>,
    /// Remaining-durability fraction per hotbar slot, resolved this frame.
    pub hotbar_durability: [Option<f32>; 9],
    pub offhand_durability: Option<f32>,
    /// Presented name of the selected stack, resolved this frame.
    pub selected_item_name: Option<std::sync::Arc<str>>,
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
        let k = java_gui_scale(physical_size, preference) as f32;
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

    /// Logical y of the highest bottom-anchored HUD row (the selected-item
    /// label zone), used by chat to avoid overlap.
    pub(super) fn bottom_row_top_logical(&self) -> f32 {
        (self.gui_height - BOTTOM_STACK_HEIGHT) * self.scale
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
                scale: UiScale::default(),
                font,
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

    pub(super) fn append(
        &mut self,
        runtime: &UiRuntime,
        frame: &HudFrame,
    ) -> Result<(), UiPresentationError> {
        // Visibility gates on the authoritative game mode directly, never on
        // inferred slot retention: a live switch to spectator with a retained
        // local slot must still drop the hotbar and crosshair.
        let mode = runtime.player_game_mode();
        let mode_allows_hotbar = mode.is_none_or(|mode| mode.shows_hotbar());
        let shows_hotbar = mode_allows_hotbar && runtime.selected_hotbar_slot().is_some();
        let survival_stats = runtime.survival_stats_visible();

        if frame.first_person && mode_allows_hotbar {
            self.crosshair()?;
        }
        if shows_hotbar {
            self.hotbar(runtime, frame)?;
        }
        // The estimated session clock drives expiry-sensitive surfaces, so
        // finite effects keep counting down between packets.
        let now_tick = runtime.estimated_server_tick(frame.now_millis);
        if survival_stats {
            self.health_rows(runtime, frame, now_tick)?;
            self.armor_row(runtime)?;
            if frame.mount_health.is_some() {
                self.mount_health_rows(frame)?;
            } else {
                self.hunger_row(runtime, now_tick)?;
            }
            self.air_row(runtime)?;
            self.experience_bar(runtime)?;
        }
        self.effects(runtime, now_tick)?;
        self.boss_bars(runtime)?;
        Ok(())
    }

    /// 15x15 invert-blend crosshair centered exactly on the framebuffer
    /// center: the fractional GUI remainder of a non-divisible viewport is
    /// kept rather than floored, so the quad's center equals width/2 and
    /// height/2 in physical pixels at every GUI scale, aspect, and DPI.
    fn crosshair(&mut self) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let x = (g.gui_width - 15.0) / 2.0;
        let y = (g.gui_height - 15.0) / 2.0;
        let sprite = self.textures.sprite(HudTextureRole::Crosshair);
        let [left, top] = g.logical([x, y]);
        let node = UiNode::new(
            UiNodeId::new(*self.next_id),
            None,
            rect(left, top, left + 15.0 * g.scale, top + 15.0 * g.scale)?,
        )
        .with_visual(UiVisual::InvertedSprite {
            texture_page: self.textures.page,
            uv: sprite.uv,
        });
        self.nodes.push(node);
        *self.next_id = self.next_id.saturating_add(1);
        Ok(())
    }

    fn hotbar(&mut self, runtime: &UiRuntime, frame: &HudFrame) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let left = (g.gui_width - HOTBAR_WIDTH) / 2.0;
        let top = g.gui_height - 22.0;
        self.sprite_gui(
            HudTextureRole::HotbarStartCap,
            [left, top],
            [255, 255, 255, HOTBAR_CAP_ALPHA],
        )?;
        for slot in 0..9u8 {
            self.sprite_gui(
                hotbar_slot_role(slot),
                [left + 1.0 + f32::from(slot) * 20.0, top],
                [255; 4],
            )?;
        }
        self.sprite_gui(
            HudTextureRole::HotbarEndCap,
            [left + HOTBAR_WIDTH - 1.0, top],
            [255, 255, 255, HOTBAR_CAP_ALPHA],
        )?;
        // The 24x24 selection frame centers on the selected 20x22 slot.
        if let Some(selected) = runtime.selected_hotbar_slot() {
            self.sprite_gui(
                HudTextureRole::SelectedHotbarSlot,
                [left + f32::from(selected) * 20.0 - 1.0, top - 1.0],
                [255; 4],
            )?;
        }
        // Offhand: the reference draws a lone slot left of the hotbar only
        // while the offhand holds an item. The official sample pack carries no
        // dedicated offhand frame, so the closest official slot art is reused.
        let offhand = runtime.gameplay_hud().offhand_stack().cloned();
        if offhand.is_some() {
            self.sprite_gui(HudTextureRole::Hotbar0, [left - 29.0, top], [255; 4])?;
        }
        // Stack counts and durability bars over each occupied slot.
        for slot in 0..9u8 {
            let Some(stack) = runtime.gameplay_hud().hotbar_stack(slot).cloned() else {
                continue;
            };
            let cell = [left + 3.0 + f32::from(slot) * 20.0, g.gui_height - 19.0];
            self.stack_decorations(&stack, cell, frame.hotbar_durability[usize::from(slot)])?;
        }
        if let Some(stack) = offhand {
            self.stack_decorations(
                &stack,
                [left - 29.0 + 3.0, g.gui_height - 19.0],
                frame.offhand_durability,
            )?;
        }
        self.selected_item_label(runtime, frame)?;
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
                    scale,
                    font: self.font,
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

    fn selected_item_label(
        &mut self,
        runtime: &UiRuntime,
        frame: &HudFrame,
    ) -> Result<(), UiPresentationError> {
        let Some(changed) = runtime.selected_item_changed_millis() else {
            return Ok(());
        };
        let elapsed = frame.now_millis.saturating_sub(changed);
        if elapsed >= LABEL_WINDOW_MILLIS {
            return Ok(());
        }
        if runtime.selected_stack().is_none() {
            return Ok(());
        }
        let Some(name) = frame.selected_item_name.clone() else {
            return Ok(());
        };
        let remaining = LABEL_WINDOW_MILLIS - elapsed;
        let alpha = if remaining >= LABEL_FADE_MILLIS {
            255.0
        } else {
            255.0 * remaining as f32 / LABEL_FADE_MILLIS as f32
        } as u8;
        if alpha == 0 {
            return Ok(());
        }
        let g = self.geometry;
        let scale = self.text_scale(9.0);
        let layout = self
            .layouts
            .layout(TextLayoutRequest {
                text: super::bounded_visible_text(&name),
                style: TextStyle::default(),
                width_64: (g.gui_width.max(1.0) * g.scale * 64.0) as u32,
                scale,
                font: self.font,
            })
            .map_err(UiPresentationError::Text)?;
        let width = layout.size_64()[0] as f32 / 64.0 / g.scale;
        // Centered above the hotbar; without survival stats the row drops by
        // 14 GUI px exactly like the reference.
        let y = if runtime.survival_stats_visible() {
            g.gui_height - 59.0
        } else {
            g.gui_height - 45.0
        };
        self.text_gui_shadowed(
            layout,
            [(g.gui_width - width) / 2.0, y],
            [255, 255, 255, alpha],
        )?;
        Ok(())
    }

    fn health_rows(
        &mut self,
        runtime: &UiRuntime,
        frame: &HudFrame,
        now_tick: Option<u64>,
    ) -> Result<(), UiPresentationError> {
        let Some(health) = runtime.hud().health() else {
            return Ok(());
        };
        let scale = u32::from(health.scale());
        // Half-heart units on the reference 20-point scale.
        let current = u32::from(health.current()).div_ceil(scale.max(1));
        let maximum = u32::from(health.maximum()) / scale.max(1);
        let absorption = runtime
            .hud()
            .absorption()
            .map(|stat| u32::from(stat.current()).div_ceil(u32::from(stat.scale()).max(1)))
            .unwrap_or(0);
        let health_hearts = maximum.div_ceil(2).min(u32::from(MAX_HEART_ROWS) * 10);
        let absorption_hearts = absorption.div_ceil(2).min(20);
        let total_hearts = (health_hearts + absorption_hearts).max(1);
        let rows = total_hearts.div_ceil(10).max(1) as u16;
        let row_height = (10 - (rows.saturating_sub(2))).max(3) as f32;

        let variant = runtime.gameplay_hud().heart_variant(now_tick);
        let flash = damage_flash_phase(runtime.last_health_drop_millis(), frame.now_millis);
        let g = self.geometry;
        let base = [(g.gui_width - HOTBAR_WIDTH) / 2.0, g.gui_height - 39.0];
        for index in 0..total_hearts {
            let row = index / 10;
            let column = index % 10;
            let position = [
                base[0] + column as f32 * 8.0,
                base[1] - row as f32 * row_height,
            ];
            self.sprite_gui(HudTextureRole::HeartBackground, position, [255; 4])?;
            let foreground = if index < health_hearts {
                let filled = current.saturating_sub(index * 2);
                heart_role(variant, flash, filled)
            } else {
                let filled = absorption.saturating_sub((index - health_hearts) * 2);
                match filled {
                    0 => None,
                    1 => Some(HudTextureRole::AbsorptionHeartHalf),
                    _ => Some(HudTextureRole::AbsorptionHeartFull),
                }
            };
            if let Some(role) = foreground {
                self.sprite_gui(role, position, [255; 4])?;
            }
        }
        Ok(())
    }

    /// Armor sits one row above the highest heart row and appears only while
    /// the authoritative equipped armor total is nonzero.
    fn armor_row(&mut self, runtime: &UiRuntime) -> Result<(), UiPresentationError> {
        let Some(armor) = runtime.hud().armor() else {
            return Ok(());
        };
        let points = u32::from(armor.current()).div_ceil(u32::from(armor.scale()).max(1));
        if points == 0 {
            return Ok(());
        }
        let Some(health) = runtime.hud().health() else {
            return Ok(());
        };
        let scale = u32::from(health.scale()).max(1);
        let maximum = u32::from(health.maximum()) / scale;
        let absorption = runtime
            .hud()
            .absorption()
            .map(|stat| u32::from(stat.current()).div_ceil(u32::from(stat.scale()).max(1)))
            .unwrap_or(0);
        let hearts = (maximum.div_ceil(2).min(u32::from(MAX_HEART_ROWS) * 10)
            + absorption.div_ceil(2).min(20))
        .max(1);
        let rows = hearts.div_ceil(10).max(1) as u16;
        let row_height = (10 - (rows.saturating_sub(2))).max(3) as f32;
        let g = self.geometry;
        let y = g.gui_height - 39.0 - (rows.saturating_sub(1)) as f32 * row_height - 10.0;
        let x = (g.gui_width - HOTBAR_WIDTH) / 2.0;
        for index in 0..10u32 {
            let position = [x + index as f32 * 8.0, y];
            let remaining = points.saturating_sub(index * 2);
            let role = match remaining {
                0 => HudTextureRole::ArmorEmpty,
                1 => HudTextureRole::ArmorHalf,
                _ => HudTextureRole::ArmorFull,
            };
            self.sprite_gui(role, position, [255; 4])?;
        }
        Ok(())
    }

    fn hunger_row(
        &mut self,
        runtime: &UiRuntime,
        now_tick: Option<u64>,
    ) -> Result<(), UiPresentationError> {
        let Some(hunger) = runtime.hud().hunger() else {
            return Ok(());
        };
        let scale = u32::from(hunger.scale()).max(1);
        let current = u32::from(hunger.current()).div_ceil(scale);
        let effect = runtime.gameplay_hud().hunger_effect_active(now_tick);
        let (background, full, half) = if effect {
            (
                HudTextureRole::HungerEffectBackground,
                HudTextureRole::HungerEffectFull,
                HudTextureRole::HungerEffectHalf,
            )
        } else {
            (
                HudTextureRole::HungerBackground,
                HudTextureRole::HungerFull,
                HudTextureRole::HungerHalf,
            )
        };
        let g = self.geometry;
        let right = (g.gui_width + HOTBAR_WIDTH) / 2.0;
        for index in 0..10u32 {
            let position = [right - index as f32 * 8.0 - 9.0, g.gui_height - 39.0];
            self.sprite_gui(background, position, [255; 4])?;
            let remaining = current.saturating_sub(index * 2);
            let role = match remaining {
                0 => None,
                1 => Some(half),
                _ => Some(full),
            };
            if let Some(role) = role {
                self.sprite_gui(role, position, [255; 4])?;
            }
        }
        Ok(())
    }

    /// Mount hearts replace the hunger row while riding, right-aligned like
    /// the reference, capped at 30 hearts across up to three rows.
    fn mount_health_rows(&mut self, frame: &HudFrame) -> Result<(), UiPresentationError> {
        let Some((current, maximum)) = frame.mount_health else {
            return Ok(());
        };
        let hearts = ((maximum + 0.5) / 2.0) as u16;
        let hearts = hearts.clamp(1, MAX_MOUNT_HEARTS);
        let filled_halves = current.clamp(0.0, maximum).ceil() as u32;
        let g = self.geometry;
        let right = (g.gui_width + HOTBAR_WIDTH) / 2.0;
        for index in 0..u32::from(hearts) {
            let row = index / 10;
            let column = index % 10;
            let position = [
                right - (column as f32 % 10.0) * 8.0 - 9.0,
                g.gui_height - 39.0 - row as f32 * 10.0,
            ];
            self.sprite_gui(HudTextureRole::HeartBackground, position, [255; 4])?;
            let remaining = filled_halves.saturating_sub(index * 2);
            let role = match remaining {
                0 => None,
                1 => Some(HudTextureRole::MountHeartHalf),
                _ => Some(HudTextureRole::MountHeartFull),
            };
            if let Some(role) = role {
                self.sprite_gui(role, position, [255; 4])?;
            }
        }
        Ok(())
    }

    /// Air bubbles above the hunger column, visible only while submerged
    /// (air below its maximum), with the reference's popping tail.
    fn air_row(&mut self, runtime: &UiRuntime) -> Result<(), UiPresentationError> {
        let Some(air) = runtime.hud().air() else {
            return Ok(());
        };
        let current = u32::from(air.current());
        let maximum = u32::from(air.maximum()).max(1);
        if current >= maximum {
            return Ok(());
        }
        let full = (current.saturating_sub(2) * 10).div_ceil(maximum);
        let popping = (current * 10).div_ceil(maximum).saturating_sub(full);
        let g = self.geometry;
        let right = (g.gui_width + HOTBAR_WIDTH) / 2.0;
        for index in 0..(full + popping).min(10) {
            let role = if index < full {
                HudTextureRole::BubbleFull
            } else {
                HudTextureRole::BubblePop
            };
            self.sprite_gui(
                role,
                [right - index as f32 * 8.0 - 9.0, g.gui_height - 49.0],
                [255; 4],
            )?;
        }
        Ok(())
    }

    /// The 182x5 classic experience bar with its clipped progress strip and
    /// the outlined green level number.
    fn experience_bar(&mut self, runtime: &UiRuntime) -> Result<(), UiPresentationError> {
        let Some(xp) = runtime.hud().experience() else {
            return Ok(());
        };
        let g = self.geometry;
        let left = (g.gui_width - HOTBAR_WIDTH) / 2.0;
        let top = g.gui_height - 29.0;
        self.sprite_gui(
            HudTextureRole::ExperienceBarBackground182,
            [left, top],
            [255; 4],
        )?;
        let progress = xp.progress.clamp(0.0, 1.0);
        let filled = (progress * 183.0).floor().clamp(0.0, 182.0);
        if filled >= 1.0 {
            let sprite = self
                .textures
                .sprite(HudTextureRole::ExperienceBarProgress182);
            let uv_width = u32::from(sprite.uv[2] - sprite.uv[0]);
            let clipped = ((filled / 182.0) * uv_width as f32).round() as u32;
            let uv = [
                sprite.uv[0],
                sprite.uv[1],
                sprite.uv[0] + clipped.min(uv_width) as u16,
                sprite.uv[3],
            ];
            let [x, y] = g.logical([left, top]);
            let node = UiNode::new(
                UiNodeId::new(*self.next_id),
                None,
                rect(x, y, x + filled * g.scale, y + 5.0 * g.scale)?,
            )
            .with_visual(UiVisual::Sprite {
                texture_page: self.textures.page,
                uv,
                color: [255; 4],
            });
            self.nodes.push(node);
            *self.next_id = self.next_id.saturating_add(1);
        }
        if xp.level > 0 {
            let text = xp.level.to_string();
            let scale = self.text_scale(9.0);
            let layout = self
                .layouts
                .layout(TextLayoutRequest {
                    text: &text,
                    style: TextStyle::default(),
                    width_64: (64.0 * 64.0) as u32,
                    scale,
                    font: self.font,
                })
                .map_err(UiPresentationError::Text)?;
            let size = [
                layout.size_64()[0] as f32 / 64.0 / g.scale,
                layout.size_64()[1] as f32 / 64.0 / g.scale,
            ];
            let center = [
                (g.gui_width - size[0]) / 2.0,
                g.gui_height - 31.0 - size[1] / 2.0 - 2.0,
            ];
            // Reference level number: black outline in the four cardinal
            // directions under the green center.
            for offset in [[1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0]] {
                self.text_gui(
                    Arc::clone(&layout),
                    [center[0] + offset[0], center[1] + offset[1]],
                    [0, 0, 0, 255],
                )?;
            }
            self.text_gui(layout, center, XP_LEVEL_COLOR)?;
        }
        Ok(())
    }

    /// Status effects in the top-right corner: beneficial row first, harmful
    /// row below, each entry a 24x24 background with an 18x18 icon, blinking
    /// through the final seconds before expiry.
    fn effects(
        &mut self,
        runtime: &UiRuntime,
        now_tick: Option<u64>,
    ) -> Result<(), UiPresentationError> {
        let mut beneficial: Vec<&HudEffect> = Vec::new();
        let mut harmful: Vec<&HudEffect> = Vec::new();
        for effect in runtime.gameplay_hud().effects() {
            if !effect.visible_at_tick(now_tick) || effect_icon_role(effect.effect_id).is_none() {
                continue;
            }
            if HARMFUL_EFFECT_IDS.contains(&effect.effect_id) {
                harmful.push(effect);
            } else {
                beneficial.push(effect);
            }
        }
        beneficial.sort_by_key(|effect| effect.effect_id);
        harmful.sort_by_key(|effect| effect.effect_id);
        for (row, effects) in [(0u32, beneficial), (1u32, harmful)] {
            let y = 1.0 + row as f32 * 25.0;
            for (column, effect) in effects.into_iter().enumerate() {
                let x = self.geometry.gui_width - 25.0 * (column as f32 + 1.0);
                if x < 0.0 {
                    break;
                }
                let background = if effect.ambient {
                    HudTextureRole::EffectBackgroundAmbient
                } else {
                    HudTextureRole::EffectBackground
                };
                let alpha = effect_blink_alpha(effect, now_tick);
                self.sprite_gui(background, [x, y], [255, 255, 255, alpha])?;
                if let Some(icon) = effect_icon_role(effect.effect_id) {
                    self.sprite_gui(icon, [x + 3.0, y + 3.0], [255, 255, 255, alpha])?;
                }
            }
        }
        Ok(())
    }

    /// Stacked boss bars top-center: title text above each 182x5 track, the
    /// filled strip clipped by authoritative health and tinted by the
    /// authoritative color.
    fn boss_bars(&mut self, runtime: &UiRuntime) -> Result<(), UiPresentationError> {
        let stacked = runtime.boss_bars().stacked();
        if stacked.is_empty() {
            return Ok(());
        }
        let g = self.geometry;
        let mut y = 12.0f32;
        for bar in stacked.iter().take(MAX_PRESENTED_BOSS_BARS) {
            if y + 19.0 > g.gui_height / 3.0 + 19.0 {
                break;
            }
            let title = super::bounded_visible_text(&bar.title);
            if !title.is_empty() {
                let scale = self.text_scale(9.0);
                let layout = self
                    .layouts
                    .layout(TextLayoutRequest {
                        text: title,
                        style: TextStyle::default(),
                        width_64: (g.gui_width.max(1.0) * g.scale * 64.0) as u32,
                        scale,
                        font: self.font,
                    })
                    .map_err(UiPresentationError::Text)?;
                let width = layout.size_64()[0] as f32 / 64.0 / g.scale;
                self.text_gui_shadowed(layout, [(g.gui_width - width) / 2.0, y - 9.0], [255; 4])?;
            }
            let left = (g.gui_width - HOTBAR_WIDTH) / 2.0;
            self.stretched_sprite_gui(
                HudTextureRole::BossProgressEmpty,
                [left, y],
                [HOTBAR_WIDTH, 5.0],
                [255; 4],
                1.0,
            )?;
            let health = bar.health.clamp(0.0, 1.0);
            if health > 0.0 {
                let tint = BOSS_TINTS
                    .iter()
                    .find(|(color, _)| *color == bar.style.color)
                    .map(|(_, tint)| *tint)
                    .unwrap_or([255; 4]);
                self.stretched_sprite_gui(
                    HudTextureRole::BossProgressFilled,
                    [left, y],
                    [HOTBAR_WIDTH * health, 5.0],
                    tint,
                    health,
                )?;
            }
            y += 19.0;
        }
        Ok(())
    }

    fn text_scale(&self, gui_px: f32) -> UiScale {
        let target_logical = gui_px * self.geometry.scale;
        let ratio = (target_logical / self.text_line_logical).clamp(0.5, 4.0);
        UiScale::new(ratio).unwrap_or_default()
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

    /// A sprite stretched to an explicit GUI-px size; `uv_fraction` clips the
    /// source horizontally for partially filled tracks.
    fn stretched_sprite_gui(
        &mut self,
        role: HudTextureRole,
        gui: [f32; 2],
        size: [f32; 2],
        color: [u8; 4],
        uv_fraction: f32,
    ) -> Result<(), UiPresentationError> {
        if size[0] <= 0.0 || size[1] <= 0.0 {
            return Ok(());
        }
        let sprite: HudSprite = self.textures.sprite(role);
        let uv_width = u32::from(sprite.uv[2] - sprite.uv[0]);
        let clipped = ((uv_fraction.clamp(0.0, 1.0)) * uv_width as f32).round() as u32;
        let uv = [
            sprite.uv[0],
            sprite.uv[1],
            sprite.uv[0] + clipped.clamp(1, uv_width) as u16,
            sprite.uv[3],
        ];
        let g = self.geometry;
        let [x, y] = g.logical(gui);
        let node = UiNode::new(
            UiNodeId::new(*self.next_id),
            None,
            rect(x, y, x + size[0] * g.scale, y + size[1] * g.scale)?,
        )
        .with_visual(UiVisual::Sprite {
            texture_page: self.textures.page,
            uv,
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
        .with_visual(UiVisual::Text { layout, color });
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
        let shadow = [
            (u16::from(color[0]) / 4) as u8,
            (u16::from(color[1]) / 4) as u8,
            (u16::from(color[2]) / 4) as u8,
            color[3],
        ];
        self.text_gui(Arc::clone(&layout), [gui[0] + 1.0, gui[1] + 1.0], shadow)?;
        self.text_gui(layout, gui, color)
    }
}
