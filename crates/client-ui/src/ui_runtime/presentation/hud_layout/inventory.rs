//! Java-proportioned player inventory presentation.
//!
//! The screen consumes real Bedrock window-0, armor, offhand and crafting
//! state from the ledger; the output cell previews the grid's unique recipe.

use std::sync::Arc;

use ui::{TextLayoutRequest, TextStyle, UiNode, UiNodeId, UiScale, UiVisual};

use crate::ui_runtime::presentation::inventory_pointer::InventoryScreen;
use {
    super::{HudFrame, HudLayout, UiPresentationError, UiRuntime, rect},
    ui::IconRef,
};

const PANEL_SIZE: [f32; 2] = [176.0, 166.0];
const SLOT_SIZE: f32 = 18.0;

/// The active crafting grid's icons in row-major order plus the previewed
/// output of its unique recipe.
#[derive(Clone, Debug, Default)]
pub struct CraftingFrame {
    pub icons: [Option<IconRef>; 9],
    pub output: Option<(Option<IconRef>, protocol::NetworkItemStack)>,
}

#[derive(Clone, Debug)]
pub struct StorageIcons(pub [Option<IconRef>; 54]);

impl Default for StorageIcons {
    fn default() -> Self {
        Self([None; 54])
    }
}

impl HudLayout<'_> {
    pub(super) fn inventory_screen(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
    ) -> Result<(), UiPresentationError> {
        let screen = InventoryScreen::of_runtime(player_runtime, runtime);
        // The container-routing setting hands the screens the engine lays out to JSON-UI.
        if frame.engine_containers
            && crate::ui_runtime::presentation::forms::engine_screen_for(player_runtime, runtime)
        {
            return Ok(());
        }
        match screen {
            InventoryScreen::Window(kind, cells) => {
                self.window_screen(player_runtime, runtime, frame, kind, cells)?
            }
            InventoryScreen::Creative => self.creative_screen(player_runtime, runtime, frame)?,
            InventoryScreen::Book => return self.book_screen(runtime, frame),
            _ => self.classic_screen(player_runtime, runtime, frame, screen)?,
        }
        self.inventory_overlays(player_runtime, runtime, frame, screen)
    }

    fn classic_screen(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
        screen: InventoryScreen,
    ) -> Result<(), UiPresentationError> {
        use crate::ui_runtime::presentation::inventory_pointer::{
            WORKBENCH_GRID, WORKBENCH_OUTPUT,
        };
        if let InventoryScreen::Storage(slot_count) = screen {
            return self.storage_screen(player_runtime, runtime, frame, slot_count);
        }
        let g = self.geometry;
        self.solid_gui([0.0, 0.0], [g.gui_width, g.gui_height], [0, 0, 0, 150])?;
        let origin = [
            ((g.gui_width - PANEL_SIZE[0]) * 0.5).floor(),
            ((g.gui_height - PANEL_SIZE[1]) * 0.5).floor(),
        ];
        self.panel(origin, PANEL_SIZE)?;

        if screen == InventoryScreen::Workbench {
            let title = frame.window_text.title.as_deref().unwrap_or("Crafting");
            self.inventory_label(title, [origin[0] + 28.0, origin[1] + 6.0])?;
            self.crafting_cells(
                player_runtime,
                runtime,
                frame,
                origin,
                WORKBENCH_GRID,
                3,
                WORKBENCH_OUTPUT,
            )?;
            self.player_cells(player_runtime, runtime, frame, origin)?;
            self.recipe_book(runtime, frame, screen, origin)?;
            return self.carried_item(player_runtime, runtime, frame);
        }

        // Armor, paper doll, offhand, and the 2x2 personal crafting grid.
        let armor = runtime.local_armor(player_runtime);
        let worn = [
            &armor.helmet,
            &armor.chestplate,
            &armor.leggings,
            &armor.boots,
        ];
        for (row, stack) in worn.into_iter().enumerate() {
            let slot = [origin[0] + 8.0, origin[1] + 8.0 + row as f32 * SLOT_SIZE];
            self.inventory_slot(slot)?;
            if let Some(icon) = frame.armor_icons[row] {
                self.inventory_item(Some(icon), slot, None, None)?;
            } else if stack.is_empty() {
                self.ghost_icon(
                    frame.window_icons.ghost_armor[row],
                    [slot[0] + 1.0, slot[1] + 1.0],
                )?;
            }
        }
        self.player_box([origin[0] + 26.0, origin[1] + 8.0], [51.0, 72.0])?;
        if runtime
            .inventory_ledger(player_runtime)
            .storage_generation()
            .is_none()
            && let Some(preview) = frame.player_preview
        {
            self.inventory_preview(preview, [origin[0] + 30.0, origin[1] + 10.0])?;
        }
        let offhand_slot = [origin[0] + 77.0, origin[1] + 62.0];
        self.inventory_slot(offhand_slot)?;
        if runtime.gameplay_hud().offhand_stack().is_none() {
            self.ghost_icon(
                frame.window_icons.ghost_shield,
                [offhand_slot[0] + 1.0, offhand_slot[1] + 1.0],
            )?;
        }
        if let Some(stack) = runtime.gameplay_hud().offhand_stack() {
            self.inventory_item(
                frame.offhand_icon,
                offhand_slot,
                Some(stack),
                frame.offhand_durability,
            )?;
        }

        self.crafting_cells(
            player_runtime,
            runtime,
            frame,
            origin,
            [98.0, 18.0],
            2,
            [152.0, 28.0],
        )?;
        self.player_cells(player_runtime, runtime, frame, origin)?;
        self.recipe_book(runtime, frame, screen, origin)?;
        self.carried_item(player_runtime, runtime, frame)
    }

    /// One crafting grid, its arrow and the previewed output cell.
    #[allow(
        clippy::too_many_arguments,
        reason = "Player authority is borrowed separately from UI state."
    )]
    fn crafting_cells(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
        origin: [f32; 2],
        grid: [f32; 2],
        width: usize,
        output: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let first_slot: u8 = if width == 3 { 32 } else { 28 };
        for index in 0..width * width {
            let slot = [
                origin[0] + grid[0] + (index % width) as f32 * SLOT_SIZE,
                origin[1] + grid[1] + (index / width) as f32 * SLOT_SIZE,
            ];
            self.inventory_slot(slot)?;
            let target =
                inventory::inventory_ledger::InventoryTarget::Craft(first_slot + index as u8);
            if let Some(stack) = runtime
                .inventory_ledger(player_runtime)
                .target_stack(target)
            {
                self.inventory_item(
                    frame.crafting.icons[index],
                    slot,
                    Some(stack),
                    frame.durability.ui[usize::from(first_slot) + index],
                )?;
            }
        }
        // Crafting arrow and result slot use original pixel geometry rather
        // than a borrowed texture or proprietary icon.
        let arrow = [origin[0] + output[0] - 17.0, origin[1] + output[1] + 3.0];
        self.solid_gui(arrow, [12.0, 4.0], [139, 139, 139, 255])?;
        self.solid_gui(
            [arrow[0] + 8.0, arrow[1] - 3.0],
            [4.0, 10.0],
            [139, 139, 139, 255],
        )?;
        let output = [origin[0] + output[0], origin[1] + output[1]];
        // The result cell draws in the enlarged 26x26 frame around its item.
        self.slot_frame([output[0] - 4.0, output[1] - 4.0], 26.0)?;
        if let Some((icon, stack)) = &frame.crafting.output {
            self.inventory_item(*icon, output, Some(stack), None)?;
        }
        Ok(())
    }

    fn player_cells(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
        origin: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        // Bedrock window 0: hotbar 0..8, storage 9..35.
        for row in 0..3 {
            for column in 0..9 {
                let inventory_index = 9 + row * 9 + column;
                let slot = [
                    origin[0] + 8.0 + column as f32 * SLOT_SIZE,
                    origin[1] + 84.0 + row as f32 * SLOT_SIZE,
                ];
                self.inventory_slot(slot)?;
                if let Some(stack) = runtime
                    .inventory_ledger(player_runtime)
                    .displayed_stack(inventory_index as u8)
                {
                    self.inventory_item(
                        frame.inventory_icons.0[inventory_index],
                        slot,
                        Some(stack),
                        frame.durability.player[inventory_index],
                    )?;
                }
                if runtime
                    .inventory_ledger(player_runtime)
                    .slot_pending(inventory_index as u8)
                {
                    self.solid_gui(
                        [slot[0] + 1.0, slot[1] + 1.0],
                        [16.0, 16.0],
                        [255, 255, 255, 48],
                    )?;
                }
            }
        }
        for column in 0..9 {
            let slot = [
                origin[0] + 8.0 + column as f32 * SLOT_SIZE,
                origin[1] + 142.0,
            ];
            self.inventory_slot(slot)?;
            if let Some(stack) = runtime
                .inventory_ledger(player_runtime)
                .displayed_stack(column as u8)
            {
                self.inventory_item(
                    frame.inventory_icons.0[column],
                    slot,
                    Some(stack),
                    frame.durability.player[column],
                )?;
            }
            if runtime
                .inventory_ledger(player_runtime)
                .slot_pending(column as u8)
            {
                self.solid_gui(
                    [slot[0] + 1.0, slot[1] + 1.0],
                    [16.0, 16.0],
                    [255, 255, 255, 48],
                )?;
            }
        }
        Ok(())
    }

    /// Draws the carried stack over every crafting and recipe-book surface.
    fn carried_item(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
    ) -> Result<(), UiPresentationError> {
        if let (Some(stack), Some(pointer)) = (
            runtime.inventory_ledger(player_runtime).cursor_stack(),
            runtime.inventory_pointer_gui(),
        ) {
            self.inventory_item(
                frame.cursor_icon,
                [pointer[0] - 8.0, pointer[1] - 8.0],
                Some(stack),
                None,
            )?;
        }
        Ok(())
    }

    fn storage_screen(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
        slot_count: usize,
    ) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        self.solid_gui([0.0, 0.0], [g.gui_width, g.gui_height], [0, 0, 0, 150])?;
        let rows = slot_count / 9;
        let panel_size = [176.0, 114.0 + rows as f32 * SLOT_SIZE];
        let origin = [
            ((g.gui_width - panel_size[0]) * 0.5).floor(),
            ((g.gui_height - panel_size[1]) * 0.5).floor(),
        ];
        self.panel(origin, panel_size)?;
        let title = frame.window_text.title.as_deref().unwrap_or("Container");
        self.inventory_label(title, [origin[0] + 8.0, origin[1] + 6.0])?;
        for index in 0..slot_count {
            let slot = [
                origin[0] + 8.0 + (index % 9) as f32 * SLOT_SIZE,
                origin[1] + 18.0 + (index / 9) as f32 * SLOT_SIZE,
            ];
            self.inventory_slot(slot)?;
            if let Some(stack) = runtime
                .inventory_ledger(player_runtime)
                .storage_stack(index as u8)
            {
                self.inventory_item(
                    frame.storage_icons.0[index],
                    slot,
                    Some(stack),
                    frame.durability.storage[index],
                )?;
            }
            if runtime
                .inventory_ledger(player_runtime)
                .storage_slot_pending(index as u8)
            {
                self.solid_gui(
                    [slot[0] + 1.0, slot[1] + 1.0],
                    [16.0, 16.0],
                    [255, 255, 255, 48],
                )?;
            }
        }
        let player_y = origin[1] + 32.0 + rows as f32 * SLOT_SIZE;
        self.inventory_label("Inventory", [origin[0] + 8.0, player_y - 12.0])?;
        for row in 0..3 {
            for column in 0..9 {
                let index = 9 + row * 9 + column;
                self.storage_player_slot(
                    player_runtime,
                    runtime,
                    frame,
                    index,
                    [
                        origin[0] + 8.0 + column as f32 * SLOT_SIZE,
                        player_y + row as f32 * SLOT_SIZE,
                    ],
                )?;
            }
        }
        for column in 0..9 {
            self.storage_player_slot(
                player_runtime,
                runtime,
                frame,
                column,
                [origin[0] + 8.0 + column as f32 * SLOT_SIZE, player_y + 58.0],
            )?;
        }
        if let (Some(stack), Some(pointer)) = (
            runtime.inventory_ledger(player_runtime).cursor_stack(),
            runtime.inventory_pointer_gui(),
        ) {
            self.inventory_item(
                frame.cursor_icon,
                [pointer[0] - 8.0, pointer[1] - 8.0],
                Some(stack),
                None,
            )?;
        }
        Ok(())
    }

    fn storage_player_slot(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
        index: usize,
        slot: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        self.inventory_slot(slot)?;
        if let Some(stack) = runtime
            .inventory_ledger(player_runtime)
            .displayed_stack(index as u8)
        {
            self.inventory_item(
                frame.inventory_icons.0[index],
                slot,
                Some(stack),
                frame.durability.player[index],
            )?;
        }
        if runtime
            .inventory_ledger(player_runtime)
            .slot_pending(index as u8)
        {
            self.solid_gui(
                [slot[0] + 1.0, slot[1] + 1.0],
                [16.0, 16.0],
                [255, 255, 255, 48],
            )?;
        }
        Ok(())
    }

    pub(super) fn panel(
        &mut self,
        origin: [f32; 2],
        size: [f32; 2],
    ) -> Result<(), UiPresentationError> {
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
        icon: Option<IconRef>,
        slot: [f32; 2],
        stack: Option<&protocol::NetworkItemStack>,
        durability: Option<f32>,
    ) -> Result<(), UiPresentationError> {
        let cell = [slot[0] + 1.0, slot[1] + 1.0];
        if let Some(icon) = icon {
            self.icon_gui(icon, cell)?;
        }
        if let Some(stack) = stack {
            self.stack_decorations(stack, cell, durability)?;
        }
        Ok(())
    }

    pub(super) fn inventory_label(
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
                line_height_64: ui::TEXT_LINE_HEIGHT_64,
                baseline_64: ui::TEXT_BASELINE_64,
                scale: UiScale::default(),
                font: self.font,
                wrap: Default::default(),
            })
            .map_err(UiPresentationError::Text)?;
        self.text_gui(Arc::clone(&layout), position, [64, 64, 64, 255])
    }
}
