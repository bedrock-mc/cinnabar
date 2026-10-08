//! Geometry of the creative catalog screen: tab strip, item grid, hotbar row.

use super::{InventoryCellHit, PlacedSlot, SLOT_SIZE};

pub const CREATIVE_PANEL: [f32; 2] = [195.0, 136.0];
pub const GRID_COLUMNS: usize = 9;
pub const GRID_ROWS: usize = 5;
pub const GRID_CELLS: usize = GRID_COLUMNS * GRID_ROWS;
pub const TAB_COUNT: u8 = 5;
/// The last tab is the search tab.
pub const SEARCH_TAB: u8 = TAB_COUNT - 1;
const GRID_ORIGIN: [f32; 2] = [9.0, 18.0];
const HOTBAR_ORIGIN: [f32; 2] = [9.0, 112.0];
const TAB_SIZE: [f32; 2] = [28.0, 28.0];
const TAB_STRIDE: f32 = 29.0;
const TAB_TOP: f32 = -28.0;

/// Every catalog and hotbar cell in panel-relative coordinates.
pub fn creative_slots() -> Vec<PlacedSlot> {
    let grid = (0..GRID_CELLS).map(|index| PlacedSlot {
        pos: [
            GRID_ORIGIN[0] + (index % GRID_COLUMNS) as f32 * SLOT_SIZE,
            GRID_ORIGIN[1] + (index / GRID_COLUMNS) as f32 * SLOT_SIZE,
        ],
        hit: InventoryCellHit::CreativeGrid(index as u8),
        output: false,
    });
    let hotbar = (0..9u8).map(|column| PlacedSlot {
        pos: [
            HOTBAR_ORIGIN[0] + f32::from(column) * SLOT_SIZE,
            HOTBAR_ORIGIN[1],
        ],
        hit: InventoryCellHit::Player(column),
        output: false,
    });
    grid.chain(hotbar).collect()
}

/// Top-left of tab `index`.
pub fn tab_origin(index: u8) -> [f32; 2] {
    [f32::from(index) * TAB_STRIDE, TAB_TOP]
}

pub const fn tab_size() -> [f32; 2] {
    TAB_SIZE
}

/// The tab under a panel-relative point, if any.
pub fn tab_at(local: [f32; 2]) -> Option<u8> {
    (0..TAB_COUNT).find(|index| {
        let origin = tab_origin(*index);
        local[0] >= origin[0]
            && local[0] < origin[0] + TAB_SIZE[0]
            && local[1] >= origin[1]
            && local[1] < origin[1] + TAB_SIZE[1]
    })
}
