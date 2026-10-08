//! Slot geometry of every container screen, shared by drawing and hit testing.
//!
//! Offsets are GUI pixels from the panel's top-left corner. Layout values come
//! from remembered public GUI texture layouts and need screenshot measurement.

use super::inventory_pointer::{InventoryCellHit, InventoryScreen};

mod book;
mod creative;
mod reader;
mod window;

pub use reader::{
    PAGE_TEXT_ORIGIN, PAGE_TEXT_WIDTH, READER_PANEL, ReaderButton, reader_buttons, reader_hit,
};

pub use book::{
    BOOK_CELL_SIZE, BOOK_CELLS, BOOK_PANEL, book_hit, book_origin, cell_origin as book_cell_origin,
    page_origin as book_page_origin, page_size as book_page_size,
    toggle_origin as book_toggle_origin, toggle_size as book_toggle_size,
};

pub use creative::{
    CREATIVE_PANEL, GRID_CELLS, GRID_COLUMNS, GRID_ROWS, SEARCH_TAB, TAB_COUNT, creative_slots,
    tab_at, tab_origin, tab_size,
};

pub use window::{
    BEACON_LEVEL_FOR, LOOM_CELLS, LOOM_COLUMNS, STONECUTTER_CELLS, Widget, widget_rects,
    window_layout,
};

pub const SLOT_SIZE: f32 = 18.0;
const PERSONAL_PANEL: [f32; 2] = [176.0, 166.0];
pub const WORKBENCH_GRID: [f32; 2] = [30.0, 17.0];
pub const WORKBENCH_OUTPUT: [f32; 2] = [124.0, 35.0];

/// One slot: its top-left corner and what a hit on it addresses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedSlot {
    pub pos: [f32; 2],
    pub hit: InventoryCellHit,
    /// Result cells draw with the enlarged output frame.
    pub output: bool,
}

/// The panel's size in GUI pixels.
pub fn panel_size(screen: InventoryScreen) -> [f32; 2] {
    match screen {
        InventoryScreen::Storage(count) => [176.0, 114.0 + (count / 9) as f32 * SLOT_SIZE],
        InventoryScreen::Window(kind, cells) => {
            window_layout(kind, cells).map_or(PERSONAL_PANEL, |layout| layout.panel)
        }
        InventoryScreen::Creative => CREATIVE_PANEL,
        InventoryScreen::Book => READER_PANEL,
        _ => PERSONAL_PANEL,
    }
}

/// The panel's top-left corner for a viewport of `gui` pixels.
pub fn panel_origin(screen: InventoryScreen, gui: [f32; 2]) -> [f32; 2] {
    let size = panel_size(screen);
    [
        ((gui[0] - size[0]) * 0.5).floor(),
        ((gui[1] - size[1]) * 0.5).floor(),
    ]
}

fn player_slots(x: f32, y: f32) -> impl Iterator<Item = PlacedSlot> {
    (0..27u8)
        .map(move |index| {
            let (row, column) = (index / 9, index % 9);
            PlacedSlot {
                pos: [
                    x + f32::from(column) * SLOT_SIZE,
                    y + f32::from(row) * SLOT_SIZE,
                ],
                hit: InventoryCellHit::Player(9 + index),
                output: false,
            }
        })
        .chain((0..9u8).map(move |column| PlacedSlot {
            pos: [x + f32::from(column) * SLOT_SIZE, y + 58.0],
            hit: InventoryCellHit::Player(column),
            output: false,
        }))
}

fn grid(first: u8, width: u8, at: [f32; 2]) -> impl Iterator<Item = PlacedSlot> {
    (0..width * width).map(move |index| PlacedSlot {
        pos: [
            at[0] + f32::from(index % width) * SLOT_SIZE,
            at[1] + f32::from(index / width) * SLOT_SIZE,
        ],
        hit: InventoryCellHit::Craft(first + index),
        output: false,
    })
}

/// Every slot of `screen` in panel-relative coordinates.
pub fn screen_slots(screen: InventoryScreen) -> Vec<PlacedSlot> {
    match screen {
        InventoryScreen::Personal => {
            let mut slots: Vec<PlacedSlot> = grid(28, 2, [98.0, 18.0]).collect();
            slots.push(PlacedSlot {
                pos: [148.0, 24.0],
                hit: InventoryCellHit::CraftOutput,
                output: true,
            });
            slots.extend((0..4u8).map(|row| PlacedSlot {
                pos: [8.0, 8.0 + f32::from(row) * SLOT_SIZE],
                hit: InventoryCellHit::Armor(row),
                output: false,
            }));
            slots.push(PlacedSlot {
                pos: [77.0, 62.0],
                hit: InventoryCellHit::Offhand,
                output: false,
            });
            slots.extend(player_slots(8.0, 84.0));
            slots
        }
        InventoryScreen::Workbench => {
            let mut slots: Vec<PlacedSlot> = grid(32, 3, WORKBENCH_GRID).collect();
            slots.push(PlacedSlot {
                pos: [WORKBENCH_OUTPUT[0] - 4.0, WORKBENCH_OUTPUT[1] - 4.0],
                hit: InventoryCellHit::CraftOutput,
                output: true,
            });
            slots.extend(player_slots(8.0, 84.0));
            slots
        }
        InventoryScreen::Storage(count) => {
            let rows = count / 9;
            let mut slots: Vec<PlacedSlot> = (0..count)
                .map(|index| PlacedSlot {
                    pos: [
                        8.0 + (index % 9) as f32 * SLOT_SIZE,
                        18.0 + (index / 9) as f32 * SLOT_SIZE,
                    ],
                    hit: InventoryCellHit::Storage(index as u8),
                    output: false,
                })
                .collect();
            slots.extend(player_slots(8.0, 32.0 + rows as f32 * SLOT_SIZE));
            slots
        }
        InventoryScreen::Window(kind, cells) => window_layout(kind, cells)
            .map(|layout| {
                let mut slots = layout.slots.clone();
                slots.extend(player_slots(layout.player[0], layout.player[1]));
                slots
            })
            .unwrap_or_default(),
        InventoryScreen::Creative => creative_slots(),
        InventoryScreen::Book => Vec::new(),
    }
}

/// The slot under `point` (panel-relative), if any.
pub fn slot_at(slots: &[PlacedSlot], point: [f32; 2]) -> Option<PlacedSlot> {
    slots.iter().copied().find(|slot| {
        let size = if slot.output { 26.0 } else { SLOT_SIZE };
        point[0] >= slot.pos[0]
            && point[0] < slot.pos[0] + size
            && point[1] >= slot.pos[1]
            && point[1] < slot.pos[1] + size
    })
}

#[cfg(test)]
mod tests {
    use protocol::WindowKind;

    use super::*;

    const KINDS: [WindowKind; 17] = [
        WindowKind::Furnace,
        WindowKind::BlastFurnace,
        WindowKind::Smoker,
        WindowKind::Enchanting,
        WindowKind::Brewing,
        WindowKind::Anvil,
        WindowKind::Dispenser,
        WindowKind::Dropper,
        WindowKind::Hopper,
        WindowKind::Horse,
        WindowKind::Beacon,
        WindowKind::Loom,
        WindowKind::Grindstone,
        WindowKind::Stonecutter,
        WindowKind::Cartography,
        WindowKind::Smithing,
        WindowKind::Crafter,
    ];

    /// Every screen lists each hit once and keeps all slots inside its panel.
    #[test]
    fn slots_are_unique_and_inside_the_panel() {
        for kind in KINDS {
            let screen = InventoryScreen::Window(kind, 17);
            let slots = screen_slots(screen);
            let panel = panel_size(screen);
            for (index, slot) in slots.iter().enumerate() {
                assert!(
                    slot.pos[0] >= 0.0 && slot.pos[0] + SLOT_SIZE <= panel[0],
                    "{kind:?}"
                );
                assert!(
                    slot.pos[1] >= 0.0 && slot.pos[1] + SLOT_SIZE <= panel[1],
                    "{kind:?}"
                );
                assert!(
                    slots[..index].iter().all(|other| other.hit != slot.hit),
                    "{kind:?} repeats {:?}",
                    slot.hit
                );
            }
            let players = slots
                .iter()
                .filter(|slot| matches!(slot.hit, InventoryCellHit::Player(_)))
                .count();
            assert_eq!(players, 36, "{kind:?}");
        }
    }

    #[test]
    fn the_furnace_result_hit_covers_its_enlarged_frame() {
        let slots = screen_slots(InventoryScreen::Window(WindowKind::Furnace, 3));
        let hit = slot_at(&slots, [112.0 + 25.0, 31.0 + 25.0]).expect("inside the frame");
        assert_eq!(hit.hit, InventoryCellHit::Storage(2));
    }

    #[test]
    fn horse_chest_cells_follow_the_content_length() {
        let slots = screen_slots(InventoryScreen::Window(WindowKind::Horse, 17));
        let storage = slots
            .iter()
            .filter(|slot| matches!(slot.hit, InventoryCellHit::Storage(_)))
            .count();
        assert_eq!(storage, 17);
    }
}
