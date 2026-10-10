//! Container screens beyond chests: per-kind layouts, progress widgets, the
//! enchanting and beacon controls, the creative catalog, hover and tooltips.
//!
//! Widget art is drawn from solid rectangles; shapes and colors need
//! comparison against a running reference client.

use std::sync::Arc;

use protocol::{NetworkItemStack, WindowKind};
use ui::{TextLayoutRequest, TextStyle, UiNode, UiNodeId, UiVisual};

use super::{HudFrame, HudLayout, IconRef, UiPresentationError, UiRuntime, rect};
use crate::ui_runtime::inventory_actions::visible_creative_entries;
use crate::ui_runtime::inventory_ledger::InventoryTarget;
use crate::ui_runtime::presentation::inventory_pointer::{InventoryCellHit, InventoryScreen};
use crate::ui_runtime::presentation::screens::{
    self, CREATIVE_PANEL, GRID_CELLS, GRID_COLUMNS, GRID_ROWS, PlacedSlot, SEARCH_TAB, SLOT_SIZE,
    STONECUTTER_CELLS, TAB_COUNT, Widget, tab_origin, tab_size,
};

const FURNACE_COOK_TICKS: f32 = 200.0;
const FAST_COOK_TICKS: f32 = 100.0;
const BREW_TICKS: f32 = 400.0;
const DEFAULT_FUEL_TOTAL: f32 = 20.0;
const TEXT_GUI_PX: f32 = 9.0;

static EMPTY_FURNACE_ICONS: std::sync::LazyLock<Arc<[Option<IconRef>]>> =
    std::sync::LazyLock::new(|| Arc::from([]));
const UI_ICON_SLOTS: usize = protocol::UI_SLOT_COUNT;

/// One tooltip line and its color.
#[derive(Clone, Debug)]
pub struct TooltipLine {
    pub text: String,
    pub color: [u8; 4],
}

/// Item icons for the UI-slot cells and the creative catalog page.
#[derive(Clone, Debug)]
pub struct WindowIcons {
    pub ui: [Option<IconRef>; UI_ICON_SLOTS],
    pub creative: [Option<IconRef>; GRID_CELLS],
    pub creative_tabs: [Option<IconRef>; TAB_COUNT as usize],
    /// Output icons of the stonecutter's recipe cells.
    pub recipe: [Option<IconRef>; STONECUTTER_CELLS],
    /// The result a recipe screen would produce now, drawn while the server
    /// previews none.
    pub recipe_output: Option<(Option<IconRef>, NetworkItemStack)>,
    /// Faint silhouettes for empty armor, shield and smithing-template slots;
    /// absent while the item atlas carries no such art.
    pub ghost_armor: [Option<IconRef>; 4],
    pub ghost_shield: Option<IconRef>,
    pub ghost_template: Option<IconRef>,
    /// Output icons of the recipe book's visible page.
    pub book: [Option<IconRef>; screens::BOOK_CELLS],
    pub book_button: Option<IconRef>,
    /// Whether a further recipe-book page exists.
    pub book_more: bool,
    /// Icons of the engine-drawn recipe book's entries, in list order.
    pub book_entries: Vec<Option<IconRef>>,
    pub furnace_entries: Arc<[Option<IconRef>]>,
}

impl Default for WindowIcons {
    fn default() -> Self {
        Self {
            ui: [None; UI_ICON_SLOTS],
            creative: [None; GRID_CELLS],
            creative_tabs: [None; TAB_COUNT as usize],
            recipe: [None; STONECUTTER_CELLS],
            recipe_output: None,
            ghost_armor: [None; 4],
            ghost_shield: None,
            ghost_template: None,
            book: [None; screens::BOOK_CELLS],
            book_button: None,
            book_more: false,
            book_entries: Vec::new(),
            furnace_entries: Arc::clone(&EMPTY_FURNACE_ICONS),
        }
    }
}

/// Durability bar fractions per inventory surface.
#[derive(Clone, Debug)]
pub struct Durability {
    pub player: [Option<f32>; 36],
    pub storage: [Option<f32>; 54],
    pub ui: [Option<f32>; UI_ICON_SLOTS],
}

impl Default for Durability {
    fn default() -> Self {
        Self {
            player: [None; 36],
            storage: [None; 54],
            ui: [None; UI_ICON_SLOTS],
        }
    }
}

/// Resolved strings for the open screen.
#[derive(Clone, Debug, Default)]
pub struct WindowText {
    pub title: Option<String>,
    /// The actor or block entity's custom name, which vanilla shows unlocalized.
    pub custom_title: Option<String>,
    /// The open block entity's NBT `id` (`Chest`, `Barrel`, …).
    pub block_entity: Option<String>,
    pub inventory_label: Option<String>,
    pub tooltip: Vec<TooltipLine>,
    /// Beacon effect names by effect id.
    pub effect_names: Vec<(i32, String)>,
    pub book_title: Option<String>,
}

fn stack_of<'a>(
    player_runtime: &'a player_state::PlayerState,
    runtime: &UiRuntime,
    hit: InventoryCellHit,
) -> Option<&'a NetworkItemStack> {
    let ledger = runtime.inventory_ledger(player_runtime);
    match hit {
        InventoryCellHit::Player(slot) => ledger.displayed_stack(slot),
        InventoryCellHit::Storage(slot) => ledger.furnace_visual_stack(slot),
        InventoryCellHit::Craft(slot) => ledger.target_stack(InventoryTarget::Craft(slot)),
        InventoryCellHit::CraftOutput => ledger.created_output_stack(),
        _ => None,
    }
}

fn icon_and_durability(frame: &HudFrame, hit: InventoryCellHit) -> (Option<IconRef>, Option<f32>) {
    match hit {
        InventoryCellHit::Player(slot) => (
            frame
                .inventory_icons
                .0
                .get(usize::from(slot))
                .copied()
                .flatten(),
            frame
                .durability
                .player
                .get(usize::from(slot))
                .copied()
                .flatten(),
        ),
        InventoryCellHit::Storage(slot) => (
            frame
                .storage_icons
                .0
                .get(usize::from(slot))
                .copied()
                .flatten(),
            frame
                .durability
                .storage
                .get(usize::from(slot))
                .copied()
                .flatten(),
        ),
        InventoryCellHit::Craft(slot) => (
            frame
                .window_icons
                .ui
                .get(usize::from(slot))
                .copied()
                .flatten(),
            frame
                .durability
                .ui
                .get(usize::from(slot))
                .copied()
                .flatten(),
        ),
        InventoryCellHit::CraftOutput => (
            frame.window_icons.ui[usize::from(protocol::CREATED_OUTPUT_SLOT)],
            frame.durability.ui[usize::from(protocol::CREATED_OUTPUT_SLOT)],
        ),
        _ => (None, None),
    }
}

impl HudLayout<'_> {
    /// Draws `text` at a GUI position and returns its size in GUI pixels.
    pub(super) fn ui_text(
        &mut self,
        text: &str,
        position: [f32; 2],
        color: [u8; 4],
        shadow: bool,
    ) -> Result<[f32; 2], UiPresentationError> {
        let layout = self
            .layouts
            .layout(TextLayoutRequest {
                text,
                style: TextStyle::default(),
                width_64: 1_024 * 64,
                line_height_64: super::super::TEXT_LINE_HEIGHT_64,
                baseline_64: super::super::TEXT_BASELINE_64,
                scale: self.text_scale(TEXT_GUI_PX),
                font: self.font,
                wrap: self.geometry.text_wrap(),
            })
            .map_err(UiPresentationError::Text)?;
        let scale = self.geometry.scale;
        let size = [
            layout.size_64()[0] as f32 / 64.0 / scale,
            layout.size_64()[1] as f32 / 64.0 / scale,
        ];
        if shadow {
            self.text_gui_shadowed(Arc::clone(&layout), position, color)?;
        } else {
            self.text_gui(layout, position, color)?;
        }
        Ok(size)
    }

    pub(super) fn slot_frame(
        &mut self,
        position: [f32; 2],
        size: f32,
    ) -> Result<(), UiPresentationError> {
        self.solid_gui(position, [size, size], [139, 139, 139, 255])?;
        self.solid_gui(position, [size, 1.0], [55, 55, 55, 255])?;
        self.solid_gui(position, [1.0, size], [55, 55, 55, 255])?;
        self.solid_gui(
            [position[0] + 1.0, position[1] + size - 1.0],
            [size - 1.0, 1.0],
            [255, 255, 255, 255],
        )?;
        self.solid_gui(
            [position[0] + size - 1.0, position[1] + 1.0],
            [1.0, size - 1.0],
            [255, 255, 255, 255],
        )
    }

    /// Dim backdrop plus the panel; returns the panel origin.
    fn screen_panel(&mut self, screen: InventoryScreen) -> Result<[f32; 2], UiPresentationError> {
        let g = self.geometry;
        self.solid_gui([0.0, 0.0], [g.gui_width, g.gui_height], [0, 0, 0, 150])?;
        let origin = screens::panel_origin(screen, [g.gui_width, g.gui_height]);
        self.panel(origin, screens::panel_size(screen))?;
        Ok(origin)
    }

    /// One placed slot: frame, item, count and durability bar.
    fn placed_slot(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
        origin: [f32; 2],
        slot: &PlacedSlot,
    ) -> Result<(), UiPresentationError> {
        let position = [origin[0] + slot.pos[0], origin[1] + slot.pos[1]];
        let inset = if slot.output {
            self.slot_frame(position, 26.0)?;
            5.0
        } else {
            self.slot_frame(position, SLOT_SIZE)?;
            1.0
        };
        let cell = [position[0] + inset, position[1] + inset];
        if let Some(stack) = stack_of(player_runtime, runtime, slot.hit) {
            let (icon, durability) = icon_and_durability(frame, slot.hit);
            if let Some(icon) = icon {
                self.icon_gui(icon, cell)?;
            }
            self.stack_decorations(stack, cell, durability)?;
        } else if slot.hit == InventoryCellHit::Craft(53) {
            self.ghost_icon(frame.window_icons.ghost_template, cell)?;
        } else if slot.hit == InventoryCellHit::CraftOutput
            && let Some((icon, stack)) = &frame.window_icons.recipe_output
        {
            if let Some(icon) = icon {
                self.icon_gui(*icon, cell)?;
            }
            self.stack_decorations(stack, cell, None)?;
        }
        Ok(())
    }

    /// An empty-slot silhouette at half strength.
    pub(super) fn ghost_icon(
        &mut self,
        icon: Option<IconRef>,
        cell: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let Some(icon) = icon else {
            return Ok(());
        };
        let g = self.geometry;
        let [x, y] = g.logical(cell);
        let node = UiNode::new(
            UiNodeId::new(*self.next_id),
            None,
            rect(x, y, x + 16.0 * g.scale, y + 16.0 * g.scale)?,
        )
        .with_visual(UiVisual::Sprite {
            texture_page: icon.page,
            uv: icon.uv,
            color: [255, 255, 255, 110],
        });
        self.nodes.push(node);
        *self.next_id = self.next_id.saturating_add(1);
        Ok(())
    }

    fn held_item(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
    ) -> Result<(), UiPresentationError> {
        if let (Some(stack), Some(pointer)) = (
            runtime.inventory_ledger(player_runtime).cursor_stack(),
            runtime.inventory_pointer_gui(),
        ) {
            if let Some(icon) = frame.cursor_icon {
                self.icon_gui(icon, [pointer[0] - 8.0, pointer[1] - 8.0])?;
            }
            self.stack_decorations(stack, [pointer[0] - 8.0, pointer[1] - 8.0], None)?;
        }
        Ok(())
    }

    pub(super) fn window_screen(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
        kind: WindowKind,
        cells: usize,
    ) -> Result<(), UiPresentationError> {
        let screen = InventoryScreen::Window(kind, cells);
        let Some(layout) = screens::window_layout(kind, cells) else {
            return Ok(());
        };
        let origin = self.screen_panel(screen)?;
        let title = frame
            .window_text
            .title
            .clone()
            .unwrap_or_else(|| default_title(kind).to_owned());
        let mut title_pos = [origin[0] + layout.title[0], origin[1] + layout.title[1]];
        if layout.title_centered {
            let width = self.measure(&title)?;
            title_pos[0] = origin[0] + ((layout.panel[0] - width) * 0.5).floor();
        }
        self.inventory_label(&title, title_pos)?;
        let label = frame
            .window_text
            .inventory_label
            .as_deref()
            .unwrap_or("Inventory");
        self.inventory_label(
            label,
            [origin[0] + layout.label[0], origin[1] + layout.label[1]],
        )?;
        self.progress_widgets(player_runtime, runtime, kind, origin)?;
        for slot in screens::screen_slots(screen) {
            self.placed_slot(player_runtime, runtime, frame, origin, &slot)?;
        }
        self.window_widgets(player_runtime, runtime, frame, kind, origin)?;
        self.held_item(player_runtime, runtime, frame)
    }

    pub(super) fn measure(&mut self, text: &str) -> Result<f32, UiPresentationError> {
        let layout = self
            .layouts
            .layout(TextLayoutRequest {
                text,
                style: TextStyle::default(),
                width_64: 1_024 * 64,
                line_height_64: super::super::TEXT_LINE_HEIGHT_64,
                baseline_64: super::super::TEXT_BASELINE_64,
                scale: self.text_scale(TEXT_GUI_PX),
                font: self.font,
                wrap: self.geometry.text_wrap(),
            })
            .map_err(UiPresentationError::Text)?;
        Ok(layout.size_64()[0] as f32 / 64.0 / self.geometry.scale)
    }

    /// An arrow made of rectangles, filled left to right by `fraction`.
    fn arrow(&mut self, at: [f32; 2], fraction: f32) -> Result<(), UiPresentationError> {
        // (x, y, width, height) of each vertical run of the 24x16 arrow.
        const PIECES: [[f32; 4]; 4] = [
            [0.0, 5.0, 17.0, 6.0],
            [17.0, 1.0, 2.0, 14.0],
            [19.0, 3.0, 2.0, 10.0],
            [21.0, 5.0, 2.0, 6.0],
        ];
        let filled = fraction.clamp(0.0, 1.0) * 23.0;
        for [x, y, w, h] in PIECES {
            self.solid_gui([at[0] + x, at[1] + y], [w, h], [139, 139, 139, 255])?;
            let fill = (filled - x).clamp(0.0, w);
            if fill > 0.0 {
                self.solid_gui([at[0] + x, at[1] + y], [fill, h], [255, 255, 255, 255])?;
            }
        }
        Ok(())
    }

    /// Furnace flame and arrow, brewing bubbles and fuel bar from window data.
    fn progress_widgets(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        kind: WindowKind,
        origin: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let ledger = runtime.inventory_ledger(player_runtime);
        let data = |property: i32| ledger.window_data(property).map(|value| value as f32);
        match kind {
            WindowKind::Furnace | WindowKind::BlastFurnace | WindowKind::Smoker => {
                let total = if kind == WindowKind::Furnace {
                    FURNACE_COOK_TICKS
                } else {
                    FAST_COOK_TICKS
                };
                self.arrow(
                    [origin[0] + 79.0, origin[1] + 34.0],
                    data(0).map_or(0.0, |ticks| ticks / total),
                )?;
                let lit = match (data(1), data(2)) {
                    (Some(remaining), Some(duration)) if duration > 0.0 => {
                        (remaining / duration).clamp(0.0, 1.0)
                    }
                    _ => 0.0,
                };
                let flame = [origin[0] + 56.0, origin[1] + 36.0];
                self.solid_gui(flame, [14.0, 14.0], [70, 70, 70, 255])?;
                let height = (13.0 * lit).ceil();
                if height > 0.0 {
                    self.solid_gui(
                        [flame[0] + 3.0, flame[1] + 14.0 - height],
                        [8.0, height],
                        [255, 140, 0, 255],
                    )?;
                }
            }
            WindowKind::Brewing => {
                let progress = data(0)
                    .filter(|remaining| *remaining > 0.0)
                    .map_or(0.0, |remaining| {
                        1.0 - (remaining / BREW_TICKS).clamp(0.0, 1.0)
                    });
                let bubbles = [origin[0] + 100.0, origin[1] + 16.0];
                self.solid_gui(bubbles, [3.0, 28.0], [139, 139, 139, 255])?;
                if progress > 0.0 {
                    self.solid_gui(
                        bubbles,
                        [3.0, (28.0 * progress).round()],
                        [90, 170, 255, 255],
                    )?;
                }
                let fuel = data(1).unwrap_or(0.0);
                let total = data(2)
                    .filter(|total| *total > 0.0)
                    .unwrap_or(DEFAULT_FUEL_TOTAL);
                let bar = [origin[0] + 60.0, origin[1] + 44.0];
                self.solid_gui(bar, [18.0, 4.0], [55, 55, 55, 255])?;
                let width = (18.0 * (fuel / total).clamp(0.0, 1.0)).round();
                if width > 0.0 {
                    self.solid_gui(bar, [width, 4.0], [120, 200, 60, 255])?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A bordered button body; `shade` is the fill grey.
    fn button(
        &mut self,
        at: [f32; 2],
        size: [f32; 2],
        border: [u8; 4],
        shade: u8,
    ) -> Result<(), UiPresentationError> {
        self.solid_gui(at, size, border)?;
        self.solid_gui(
            [at[0] + 1.0, at[1] + 1.0],
            [size[0] - 2.0, size[1] - 2.0],
            [shade, shade, shade, 255],
        )
    }

    /// Enchant options, beacon effects, stonecutter recipes, loom patterns and the anvil name field.
    fn window_widgets(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
        kind: WindowKind,
        origin: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let hovered = match runtime.screen_state().hover {
            Some(InventoryCellHit::Widget(widget)) => Some(widget),
            _ => None,
        };
        let level = runtime.hud().experience().map_or(0, |xp| xp.level);
        let state = runtime.screen_state();
        let stonecutter_choice = runtime
            .active_screen_recipe(player_runtime)
            .map(|recipe| recipe.id);
        let stonecutter_ids: Vec<u32> = if kind == WindowKind::Stonecutter {
            runtime
                .stonecutter_options(player_runtime)
                .iter()
                .map(|recipe| recipe.id)
                .collect()
        } else {
            Vec::new()
        };
        for (widget, pos, size) in screens::widget_rects(kind) {
            let at = [origin[0] + pos[0], origin[1] + pos[1]];
            let hot = hovered == Some(widget);
            match widget {
                Widget::EnchantOption(index) => {
                    let option = runtime
                        .inventory_ledger(player_runtime)
                        .enchant_options()
                        .and_then(|options| options.get(usize::from(index)));
                    let shade = match (option, hot) {
                        (None, _) => 60,
                        (Some(_), true) => 130,
                        (Some(_), false) => 100,
                    };
                    self.button(at, size, [0, 0, 0, 255], shade)?;
                    if let Some(option) = option {
                        let cost = option.cost.to_string();
                        let color = if u32::from(option.cost) <= level {
                            [128, 255, 32, 255]
                        } else {
                            [255, 96, 96, 255]
                        };
                        let width = self.measure(&cost)?;
                        self.ui_text(
                            &cost,
                            [at[0] + size[0] - width - 4.0, at[1] + 5.0],
                            color,
                            true,
                        )?;
                    }
                }
                Widget::BeaconEffect { id, secondary } => {
                    let (primary, second) = state.beacon;
                    let selected = if secondary {
                        second == id
                    } else {
                        primary == id
                    };
                    let unlocked = screens::BEACON_LEVEL_FOR
                        .iter()
                        .find(|(effect, _)| *effect == id)
                        .is_some_and(|(_, needed)| {
                            state.beacon_level.is_none_or(|have| have >= *needed)
                        });
                    let border = if selected {
                        [120, 230, 120, 255]
                    } else {
                        [0, 0, 0, 255]
                    };
                    let shade = if !unlocked {
                        60
                    } else if hot {
                        150
                    } else {
                        110
                    };
                    self.button(at, size, border, shade)?;
                    if let Some(role) = super::effect_icon_role(id) {
                        let tint = if unlocked {
                            [255; 4]
                        } else {
                            [255, 255, 255, 90]
                        };
                        self.sprite_gui(role, [at[0] + 2.0, at[1] + 2.0], tint)?;
                    }
                }
                Widget::BeaconUpgrade => {
                    let (primary, second) = state.beacon;
                    let unlocked = state.beacon_level.is_none_or(|have| have >= 4) && primary != 0;
                    let selected = primary != 0 && second == primary;
                    let border = if selected {
                        [120, 230, 120, 255]
                    } else {
                        [0, 0, 0, 255]
                    };
                    let shade = if !unlocked {
                        60
                    } else if hot {
                        150
                    } else {
                        110
                    };
                    self.button(at, size, border, shade)?;
                    if let Some(role) = super::effect_icon_role(primary) {
                        self.sprite_gui(role, [at[0] + 2.0, at[1] + 2.0], [255; 4])?;
                        self.ui_text("+", [at[0] + 14.0, at[1] + 12.0], [255; 4], true)?;
                    }
                }
                Widget::BeaconConfirm => {
                    let ready = state.beacon.0 != 0;
                    let shade = if ready { 90 } else { 60 };
                    let border = if ready {
                        [60, 160, 60, 255]
                    } else {
                        [0, 0, 0, 255]
                    };
                    self.button(at, size, border, shade)?;
                    self.ui_text("OK", [at[0] + 5.0, at[1] + 7.0], [255; 4], true)?;
                }
                Widget::StonecutterRecipe(index) => {
                    let Some(id) = stonecutter_ids.get(usize::from(index)) else {
                        self.button(at, size, [0, 0, 0, 255], 60)?;
                        continue;
                    };
                    let chosen = stonecutter_choice == Some(*id);
                    let border = if chosen {
                        [120, 230, 120, 255]
                    } else {
                        [0, 0, 0, 255]
                    };
                    self.button(at, size, border, if hot { 150 } else { 110 })?;
                    if let Some(icon) = frame.window_icons.recipe[usize::from(index)] {
                        self.icon_gui(icon, [at[0], at[1] + 1.0])?;
                    }
                }
                Widget::LoomPattern(index) => {
                    let position = state.loom_row * screens::LOOM_COLUMNS + usize::from(index);
                    let Some(pattern) =
                        crate::ui_runtime::screen_recipes::LOOM_PATTERNS.get(position)
                    else {
                        continue;
                    };
                    let chosen = state.loom_pattern.as_deref() == Some(*pattern);
                    let border = if chosen {
                        [120, 230, 120, 255]
                    } else {
                        [0, 0, 0, 255]
                    };
                    self.button(at, size, border, if hot { 150 } else { 110 })?;
                    self.ui_text(pattern, [at[0] + 2.0, at[1] + 3.0], [255; 4], false)?;
                }
                Widget::AnvilName => {
                    let border = if state.anvil_focused {
                        [255, 255, 255, 255]
                    } else {
                        [160, 160, 160, 255]
                    };
                    self.solid_gui(at, size, border)?;
                    self.solid_gui(
                        [at[0] + 1.0, at[1] + 1.0],
                        [size[0] - 2.0, size[1] - 2.0],
                        [0, 0, 0, 255],
                    )?;
                    let text = format!(
                        "{}{}",
                        state.anvil_name,
                        if state.anvil_focused { "_" } else { "" }
                    );
                    self.ui_text(
                        &text,
                        [at[0] + 3.0, at[1] + 2.0],
                        [224, 224, 224, 255],
                        false,
                    )?;
                }
                // Book controls draw with their own panels.
                Widget::BookToggle
                | Widget::RecipeFilter
                | Widget::CrafterSlot(_)
                | Widget::InventoryLayout(_)
                | Widget::FurnaceTab(_)
                | Widget::FurnaceClearRecipe
                | Widget::LoomPatternAt(_)
                | Widget::BookRecipe(_)
                | Widget::BookPage { .. }
                | Widget::Reader(_) => {}
            }
        }
        Ok(())
    }

    pub(super) fn creative_screen(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
    ) -> Result<(), UiPresentationError> {
        let screen = InventoryScreen::Creative;
        let origin = self.screen_panel(screen)?;
        let state = runtime.screen_state();
        for tab in 0..TAB_COUNT {
            let at = tab_origin(tab);
            let position = [origin[0] + at[0], origin[1] + at[1]];
            let shade = if tab == state.creative_tab { 198 } else { 139 };
            self.solid_gui(position, tab_size(), [0, 0, 0, 255])?;
            self.solid_gui(
                [position[0] + 1.0, position[1] + 1.0],
                [tab_size()[0] - 2.0, tab_size()[1] - 1.0],
                [shade, shade, shade, 255],
            )?;
            if let Some(icon) = frame.window_icons.creative_tabs[usize::from(tab)] {
                self.icon_gui(icon, [position[0] + 6.0, position[1] + 6.0])?;
            }
        }
        let title = frame.window_text.title.clone().unwrap_or_default();
        self.inventory_label(&title, [origin[0] + 8.0, origin[1] + 6.0])?;
        if state.creative_tab == SEARCH_TAB {
            let field = [origin[0] + 82.0, origin[1] + 5.0];
            self.solid_gui(field, [89.0, 12.0], [0, 0, 0, 255])?;
            let text = format!(
                "{}{}",
                state.search,
                if state.search_focused { "_" } else { "" }
            );
            self.ui_text(&text, [field[0] + 2.0, field[1] + 2.0], [255; 4], false)?;
        }
        let entries = visible_creative_entries(runtime.inventory_ledger(player_runtime), state);
        for slot in screens::creative_slots() {
            let position = [origin[0] + slot.pos[0], origin[1] + slot.pos[1]];
            self.slot_frame(position, SLOT_SIZE)?;
            let cell = [position[0] + 1.0, position[1] + 1.0];
            match slot.hit {
                InventoryCellHit::CreativeGrid(index) => {
                    let has_entry = entries
                        .get(state.creative_row * GRID_COLUMNS + usize::from(index))
                        .is_some();
                    if has_entry && let Some(icon) = frame.window_icons.creative[usize::from(index)]
                    {
                        self.icon_gui(icon, cell)?;
                    }
                }
                hit => {
                    if let Some(stack) = stack_of(player_runtime, runtime, hit) {
                        let (icon, durability) = icon_and_durability(frame, hit);
                        if let Some(icon) = icon {
                            self.icon_gui(icon, cell)?;
                        }
                        self.stack_decorations(stack, cell, durability)?;
                    }
                }
            }
        }
        // Scrollbar: a track with a thumb placed by the first visible row.
        let track = [origin[0] + CREATIVE_PANEL[0] - 20.0, origin[1] + 18.0];
        self.solid_gui(track, [12.0, 90.0], [85, 85, 85, 255])?;
        let rows = entries
            .len()
            .div_ceil(GRID_COLUMNS)
            .saturating_sub(GRID_ROWS);
        let fraction = if rows == 0 {
            0.0
        } else {
            state.creative_row as f32 / rows as f32
        };
        self.solid_gui(
            [track[0], track[1] + (75.0 * fraction).round()],
            [12.0, 15.0],
            [198, 198, 198, 255],
        )?;
        self.held_item(player_runtime, runtime, frame)
    }

    /// Hover highlight and the item tooltip, drawn over any inventory screen.
    pub(super) fn inventory_overlays(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        frame: &HudFrame,
        screen: InventoryScreen,
    ) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let origin = screens::panel_origin(screen, [g.gui_width, g.gui_height]);
        let Some(hover) = runtime.screen_state().hover else {
            return Ok(());
        };
        if let Some(slot) = screens::screen_slots(screen)
            .into_iter()
            .find(|slot| slot.hit == hover)
        {
            let inset = if slot.output { 5.0 } else { 1.0 };
            self.solid_gui(
                [
                    origin[0] + slot.pos[0] + inset,
                    origin[1] + slot.pos[1] + inset,
                ],
                [16.0, 16.0],
                [255, 255, 255, 128],
            )?;
        }
        if runtime
            .inventory_ledger(player_runtime)
            .cursor_stack()
            .is_some()
            || frame.window_text.tooltip.is_empty()
        {
            return Ok(());
        }
        let Some(pointer) = runtime.inventory_pointer_gui() else {
            return Ok(());
        };
        let mut width = 0.0_f32;
        for line in &frame.window_text.tooltip {
            width = width.max(self.measure(&line.text)?);
        }
        let height = frame.window_text.tooltip.len() as f32 * 10.0 - 1.0;
        let x = (pointer[0] + 12.0).min(g.gui_width - width - 8.0).max(4.0);
        let y = (pointer[1] - 12.0).clamp(4.0, (g.gui_height - height - 8.0).max(4.0));
        self.solid_gui(
            [x - 4.0, y - 4.0],
            [width + 8.0, height + 8.0],
            [40, 0, 80, 255],
        )?;
        self.solid_gui(
            [x - 3.0, y - 3.0],
            [width + 6.0, height + 6.0],
            [16, 0, 16, 240],
        )?;
        for (index, line) in frame.window_text.tooltip.clone().iter().enumerate() {
            self.ui_text(&line.text, [x, y + index as f32 * 10.0], line.color, true)?;
        }
        Ok(())
    }
}

/// English fallback for the screen title when no translation exists.
pub const fn default_title(kind: WindowKind) -> &'static str {
    match kind {
        WindowKind::Storage => "Container",
        WindowKind::Workbench => "Crafting",
        WindowKind::Furnace => "Furnace",
        WindowKind::BlastFurnace => "Blast Furnace",
        WindowKind::Smoker => "Smoker",
        WindowKind::Enchanting => "Enchant",
        WindowKind::Brewing => "Brewing Stand",
        WindowKind::Anvil => "Repair & Name",
        WindowKind::Dispenser => "Dispenser",
        WindowKind::Dropper => "Dropper",
        WindowKind::Hopper => "Item Hopper",
        WindowKind::Horse => "Horse",
        WindowKind::Beacon => "Beacon",
        WindowKind::Loom => "Loom",
        WindowKind::Grindstone => "Repair & Disenchant",
        WindowKind::Stonecutter => "Stonecutter",
        WindowKind::Cartography => "Cartography Table",
        WindowKind::Smithing => "Upgrade Gear",
        WindowKind::Crafter => "Crafter",
        WindowKind::Lectern => "Lectern",
    }
}

/// The language key of the screen title.
pub const fn title_key(kind: WindowKind) -> &'static str {
    match kind {
        WindowKind::Storage => "container.chest",
        WindowKind::Workbench => "container.crafting",
        WindowKind::Furnace => "container.furnace",
        WindowKind::BlastFurnace => "container.blast_furnace",
        WindowKind::Smoker => "container.smoker",
        WindowKind::Enchanting => "container.enchant",
        WindowKind::Brewing => "container.brewing",
        WindowKind::Anvil => "container.repair",
        WindowKind::Dispenser => "container.dispenser",
        WindowKind::Dropper => "container.dropper",
        WindowKind::Hopper => "container.hopper",
        WindowKind::Horse => "container.horse",
        WindowKind::Beacon => "tile.beacon.name",
        WindowKind::Loom => "container.loom",
        WindowKind::Grindstone => "container.grindstone_title",
        WindowKind::Stonecutter => "container.stonecutter",
        WindowKind::Cartography => "container.cartography_table",
        WindowKind::Smithing => "container.upgrade",
        WindowKind::Crafter => "container.crafter",
        WindowKind::Lectern => "container.lectern",
    }
}
