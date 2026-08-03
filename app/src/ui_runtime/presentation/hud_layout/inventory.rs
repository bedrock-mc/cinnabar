//! Java-proportioned player inventory presentation.
//!
//! The screen consumes real Bedrock window-0, armor, and offhand state. It is
//! deliberately read-only until the protocol transaction owner is modeled;
//! drawing empty crafting cells is preferable to inventing client authority.

use std::sync::Arc;

use ui::{TextLayoutRequest, TextStyle, UiNode, UiNodeId, UiScale, UiVisual};

use super::{HudFrame, HudLayout, IconRef, UiPresentationError, UiRuntime, rect};

const PANEL_SIZE: [f32; 2] = [176.0, 166.0];
const SLOT_SIZE: f32 = 18.0;

impl HudLayout<'_> {
    pub(super) fn inventory_screen(
        &mut self,
        runtime: &UiRuntime,
        frame: &HudFrame,
    ) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        self.solid_gui([0.0, 0.0], [g.gui_width, g.gui_height], [0, 0, 0, 150])?;
        let origin = [
            ((g.gui_width - PANEL_SIZE[0]) * 0.5).floor(),
            ((g.gui_height - PANEL_SIZE[1]) * 0.5).floor(),
        ];
        self.panel(origin, PANEL_SIZE)?;

        self.inventory_label("Crafting", [origin[0] + 97.0, origin[1] + 6.0])?;

        // Armor, paper doll, offhand, and the 2x2 personal crafting grid.
        for row in 0..4 {
            let slot = [origin[0] + 8.0, origin[1] + 8.0 + row as f32 * SLOT_SIZE];
            self.inventory_slot(slot)?;
            if let Some(icon) = frame.armor_icons[row] {
                self.inventory_item(icon, slot, None)?;
            }
        }
        self.player_box([origin[0] + 26.0, origin[1] + 8.0], [51.0, 72.0])?;
        if let Some(preview) = frame.player_preview {
            self.inventory_preview(preview, [origin[0] + 30.0, origin[1] + 10.0])?;
        }
        let offhand_slot = [origin[0] + 77.0, origin[1] + 62.0];
        self.inventory_slot(offhand_slot)?;
        if let (Some(icon), Some(stack)) =
            (frame.offhand_icon, runtime.gameplay_hud().offhand_stack())
        {
            self.inventory_item(icon, offhand_slot, Some(stack))?;
        }

        for row in 0..2 {
            for column in 0..2 {
                self.inventory_slot([
                    origin[0] + 98.0 + column as f32 * SLOT_SIZE,
                    origin[1] + 18.0 + row as f32 * SLOT_SIZE,
                ])?;
            }
        }
        // Crafting arrow and result slot use original pixel geometry rather
        // than a borrowed texture or proprietary icon.
        let arrow = [origin[0] + 135.0, origin[1] + 31.0];
        self.solid_gui(arrow, [12.0, 4.0], [139, 139, 139, 255])?;
        self.solid_gui(
            [arrow[0] + 8.0, arrow[1] - 3.0],
            [4.0, 10.0],
            [139, 139, 139, 255],
        )?;
        self.inventory_slot([origin[0] + 152.0, origin[1] + 28.0])?;

        // Bedrock window 0: hotbar 0..8, storage 9..35.
        for row in 0..3 {
            for column in 0..9 {
                let inventory_index = 9 + row * 9 + column;
                let slot = [
                    origin[0] + 8.0 + column as f32 * SLOT_SIZE,
                    origin[1] + 84.0 + row as f32 * SLOT_SIZE,
                ];
                self.inventory_slot(slot)?;
                if let (Some(icon), Some(stack)) = (
                    frame.inventory_icons.0[inventory_index],
                    runtime.gameplay_hud().inventory_stack(inventory_index),
                ) {
                    self.inventory_item(icon, slot, Some(stack))?;
                }
            }
        }
        for column in 0..9 {
            let slot = [
                origin[0] + 8.0 + column as f32 * SLOT_SIZE,
                origin[1] + 142.0,
            ];
            self.inventory_slot(slot)?;
            if let (Some(icon), Some(stack)) = (
                frame.inventory_icons.0[column],
                runtime.gameplay_hud().inventory_stack(column),
            ) {
                self.inventory_item(icon, slot, Some(stack))?;
            }
        }
        Ok(())
    }

    fn panel(&mut self, origin: [f32; 2], size: [f32; 2]) -> Result<(), UiPresentationError> {
        self.solid_gui(origin, size, [198, 198, 198, 255])?;
        self.solid_gui(origin, [size[0], 1.0], [255, 255, 255, 255])?;
        self.solid_gui(origin, [1.0, size[1]], [255, 255, 255, 255])?;
        self.solid_gui(
            [origin[0], origin[1] + size[1] - 1.0],
            [size[0], 1.0],
            [85, 85, 85, 255],
        )?;
        self.solid_gui(
            [origin[0] + size[0] - 1.0, origin[1]],
            [1.0, size[1]],
            [85, 85, 85, 255],
        )
    }

    fn inventory_slot(&mut self, position: [f32; 2]) -> Result<(), UiPresentationError> {
        self.solid_gui(position, [SLOT_SIZE, SLOT_SIZE], [139, 139, 139, 255])?;
        self.solid_gui(position, [SLOT_SIZE, 1.0], [55, 55, 55, 255])?;
        self.solid_gui(position, [1.0, SLOT_SIZE], [55, 55, 55, 255])?;
        self.solid_gui(
            [position[0] + 1.0, position[1] + SLOT_SIZE - 1.0],
            [SLOT_SIZE - 1.0, 1.0],
            [255, 255, 255, 255],
        )?;
        self.solid_gui(
            [position[0] + SLOT_SIZE - 1.0, position[1] + 1.0],
            [1.0, SLOT_SIZE - 1.0],
            [255, 255, 255, 255],
        )
    }

    fn player_box(
        &mut self,
        position: [f32; 2],
        size: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        self.solid_gui(position, size, [0, 0, 0, 255])?;
        self.solid_gui(
            [position[0] + 1.0, position[1] + 1.0],
            [size[0] - 2.0, size[1] - 2.0],
            [28, 28, 28, 255],
        )
    }

    fn inventory_preview(
        &mut self,
        preview: IconRef,
        position: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let [x, y] = g.logical(position);
        self.nodes.push(
            UiNode::new(
                UiNodeId::new(*self.next_id),
                None,
                rect(x, y, x + 43.0 * g.scale, y + 67.0 * g.scale)?,
            )
            .with_visual(UiVisual::Sprite {
                texture_page: preview.page,
                uv: preview.uv,
                color: [255; 4],
            }),
        );
        *self.next_id = self.next_id.saturating_add(1);
        Ok(())
    }

    fn inventory_item(
        &mut self,
        icon: IconRef,
        slot: [f32; 2],
        stack: Option<&protocol::NetworkItemStack>,
    ) -> Result<(), UiPresentationError> {
        let cell = [slot[0] + 1.0, slot[1] + 1.0];
        self.icon_gui(icon, cell)?;
        if let Some(stack) = stack {
            self.stack_decorations(stack, cell, None)?;
        }
        Ok(())
    }

    fn inventory_label(
        &mut self,
        text: &str,
        position: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let layout = self
            .layouts
            .layout(TextLayoutRequest {
                text,
                style: TextStyle::default(),
                width_64: 128 * 64,
                line_height_64: super::super::TEXT_LINE_HEIGHT_64,
                baseline_64: super::super::TEXT_BASELINE_64,
                scale: UiScale::default(),
                font: self.font,
            })
            .map_err(UiPresentationError::Text)?;
        self.text_gui(Arc::clone(&layout), position, [64, 64, 64, 255])
    }
}
