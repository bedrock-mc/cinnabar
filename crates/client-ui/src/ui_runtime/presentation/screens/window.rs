//! Per-kind layouts of the non-chest container screens.

use protocol::WindowKind;

use super::{InventoryCellHit, PlacedSlot, SLOT_SIZE};

/// One screen's panel, labels, cells and where the player inventory starts.
#[derive(Debug, Clone)]
pub struct WindowLayout {
    pub panel: [f32; 2],
    pub title: [f32; 2],
    pub title_centered: bool,
    pub label: [f32; 2],
    pub slots: Vec<PlacedSlot>,
    /// Top-left of the player inventory's first main row.
    pub player: [f32; 2],
}

/// A clickable control that is not an item cell.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Widget {
    EnchantOption(u8),
    BeaconEffect {
        id: i32,
        secondary: bool,
    },
    /// The secondary effect that upgrades the chosen primary one.
    BeaconUpgrade,
    BeaconConfirm,
    /// One stonecutter recipe cell by position on the visible page.
    StonecutterRecipe(u8),
    /// One loom pattern cell by position on the visible page.
    LoomPattern(u8),
    /// One loom pattern by position in the whole list.
    LoomPatternAt(u8),
    AnvilName,
    /// Opens or closes the recipe book.
    BookToggle,
    /// Flips the survival recipe book between craftable-only and every recipe.
    RecipeFilter,
    /// Picks the inventory layout by its radio index: survival, recipe book, or
    /// the creative wide list.
    InventoryLayout(u8),
    FurnaceTab(u8),
    FurnaceClearRecipe,
    /// Re-enables one disabled crafter slot.
    CrafterSlot(u8),
    /// One recipe cell by position on the visible page.
    BookRecipe(u8),
    BookPage {
        next: bool,
    },
    /// A book reader or editor button.
    Reader(super::ReaderButton),
}

pub const STONECUTTER_COLUMNS: usize = 4;
pub const STONECUTTER_CELLS: usize = 12;
pub const LOOM_COLUMNS: usize = 4;
pub const LOOM_CELLS: usize = 16;
/// The beacon level each selectable effect needs.
pub const BEACON_LEVEL_FOR: [(i32, u8); 6] = [(1, 1), (3, 1), (11, 2), (8, 2), (5, 3), (10, 4)];

const EFFECT_SIZE: f32 = 22.0;
/// Beacon effect buttons by tier: `(effect id, panel-relative position)`.
const BEACON_PRIMARY: [(i32, [f32; 2]); 5] = [
    (1, [76.0, 22.0]),
    (3, [102.0, 22.0]),
    (11, [76.0, 47.0]),
    (8, [102.0, 47.0]),
    (5, [89.0, 72.0]),
];
const BEACON_SECONDARY: (i32, [f32; 2]) = (10, [147.0, 72.0]);
const BEACON_UPGRADE: [f32; 2] = [171.0, 72.0];
const BEACON_CONFIRM: [f32; 2] = [164.0, 107.0];

fn slot(hit: InventoryCellHit, x: f32, y: f32) -> PlacedSlot {
    PlacedSlot {
        pos: [x, y],
        hit,
        output: false,
    }
}

fn craft(slot_index: u8, x: f32, y: f32) -> PlacedSlot {
    slot(InventoryCellHit::Craft(slot_index), x, y)
}

fn storage(index: u8, x: f32, y: f32) -> PlacedSlot {
    slot(InventoryCellHit::Storage(index), x, y)
}

fn output(x: f32, y: f32) -> PlacedSlot {
    slot(InventoryCellHit::CraftOutput, x, y)
}

fn standard(slots: Vec<PlacedSlot>) -> WindowLayout {
    WindowLayout {
        panel: [176.0, 166.0],
        title: [8.0, 6.0],
        title_centered: false,
        label: [8.0, 72.0],
        slots,
        player: [8.0, 84.0],
    }
}

/// The layout for `kind` with `cells` window cells, or `None` for the chest and
/// workbench screens, which have their own.
pub fn window_layout(kind: WindowKind, cells: usize) -> Option<WindowLayout> {
    let grid = |first: u8, count: usize, columns: usize, x: f32, y: f32| -> Vec<PlacedSlot> {
        let count = count.min(usize::from(u8::MAX) + 1 - usize::from(first));
        (0..count)
            .map(|index| {
                storage(
                    first + index as u8,
                    x + (index % columns) as f32 * SLOT_SIZE,
                    y + (index / columns) as f32 * SLOT_SIZE,
                )
            })
            .collect()
    };
    Some(match kind {
        WindowKind::Storage | WindowKind::Workbench | WindowKind::Lectern => return None,
        WindowKind::Dispenser | WindowKind::Dropper => WindowLayout {
            title_centered: true,
            ..standard(grid(0, 9, 3, 62.0, 17.0))
        },
        WindowKind::Crafter => WindowLayout {
            title_centered: true,
            ..standard(grid(0, 9, 3, 26.0, 17.0))
        },
        WindowKind::Hopper => WindowLayout {
            panel: [176.0, 133.0],
            label: [8.0, 39.0],
            player: [8.0, 51.0],
            ..standard(grid(0, 5, 5, 44.0, 20.0))
        },
        WindowKind::Furnace | WindowKind::BlastFurnace | WindowKind::Smoker => {
            let mut result = storage(2, 112.0, 31.0);
            result.output = true;
            WindowLayout {
                title_centered: true,
                ..standard(vec![storage(0, 56.0, 17.0), storage(1, 56.0, 53.0), result])
            }
        }
        WindowKind::Brewing => WindowLayout {
            title_centered: true,
            ..standard(vec![
                storage(0, 79.0, 17.0),
                storage(1, 56.0, 51.0),
                storage(2, 79.0, 58.0),
                storage(3, 102.0, 51.0),
                storage(4, 17.0, 17.0),
            ])
        },
        WindowKind::Anvil => WindowLayout {
            title: [60.0, 6.0],
            ..standard(vec![
                craft(1, 27.0, 47.0),
                craft(2, 76.0, 47.0),
                output(134.0, 47.0),
            ])
        },
        WindowKind::Enchanting => WindowLayout {
            title: [12.0, 5.0],
            label: [8.0, 73.0],
            ..standard(vec![craft(14, 15.0, 47.0), craft(15, 35.0, 47.0)])
        },
        WindowKind::Grindstone => standard(vec![
            craft(16, 49.0, 19.0),
            craft(17, 49.0, 40.0),
            output(129.0, 34.0),
        ]),
        WindowKind::Loom => WindowLayout {
            title: [8.0, 4.0],
            ..standard(vec![
                craft(9, 13.0, 26.0),
                craft(10, 33.0, 26.0),
                craft(11, 23.0, 45.0),
                output(143.0, 57.0),
            ])
        },
        WindowKind::Smithing => WindowLayout {
            title: [44.0, 15.0],
            ..standard(vec![
                craft(53, 8.0, 48.0),
                craft(51, 26.0, 48.0),
                craft(52, 44.0, 48.0),
                output(98.0, 48.0),
            ])
        },
        WindowKind::Cartography => WindowLayout {
            title: [8.0, 4.0],
            ..standard(vec![
                craft(12, 15.0, 15.0),
                craft(13, 15.0, 52.0),
                output(146.0, 39.0),
            ])
        },
        WindowKind::Stonecutter => WindowLayout {
            title: [8.0, 4.0],
            ..standard(vec![craft(3, 20.0, 33.0), output(143.0, 33.0)])
        },
        WindowKind::Beacon => WindowLayout {
            panel: [230.0, 219.0],
            title: [8.0, 6.0],
            title_centered: true,
            label: [36.0, 125.0],
            slots: vec![craft(27, 149.0, 107.0)],
            player: [36.0, 137.0],
        },
        WindowKind::Horse => {
            let mut slots = vec![storage(0, 8.0, 18.0), storage(1, 8.0, 36.0)];
            let chest = cells.saturating_sub(2);
            slots.extend(grid(2, chest, (chest / 3).max(1), 80.0, 18.0));
            standard(slots)
        }
    })
}

/// The clickable controls of `kind`'s screen, panel-relative `(position, size)`.
pub fn widget_rects(kind: WindowKind) -> Vec<(Widget, [f32; 2], [f32; 2])> {
    match kind {
        WindowKind::Enchanting => (0..3u8)
            .map(|index| {
                (
                    Widget::EnchantOption(index),
                    [60.0, 14.0 + f32::from(index) * 19.0],
                    [108.0, 19.0],
                )
            })
            .collect(),
        WindowKind::Beacon => {
            let mut widgets: Vec<_> = BEACON_PRIMARY
                .iter()
                .map(|(id, pos)| {
                    (
                        Widget::BeaconEffect {
                            id: *id,
                            secondary: false,
                        },
                        *pos,
                        [EFFECT_SIZE; 2],
                    )
                })
                .collect();
            widgets.push((
                Widget::BeaconEffect {
                    id: BEACON_SECONDARY.0,
                    secondary: true,
                },
                BEACON_SECONDARY.1,
                [EFFECT_SIZE; 2],
            ));
            widgets.push((Widget::BeaconUpgrade, BEACON_UPGRADE, [EFFECT_SIZE; 2]));
            widgets.push((Widget::BeaconConfirm, BEACON_CONFIRM, [EFFECT_SIZE; 2]));
            widgets
        }
        WindowKind::Stonecutter => (0..STONECUTTER_CELLS)
            .map(|index| {
                (
                    Widget::StonecutterRecipe(index as u8),
                    [
                        52.0 + (index % STONECUTTER_COLUMNS) as f32 * 16.0,
                        14.0 + (index / STONECUTTER_COLUMNS) as f32 * 18.0,
                    ],
                    [16.0, 18.0],
                )
            })
            .collect(),
        WindowKind::Loom => (0..LOOM_CELLS)
            .map(|index| {
                (
                    Widget::LoomPattern(index as u8),
                    [
                        60.0 + (index % LOOM_COLUMNS) as f32 * 14.0,
                        13.0 + (index / LOOM_COLUMNS) as f32 * 14.0,
                    ],
                    [14.0, 14.0],
                )
            })
            .collect(),
        WindowKind::Anvil => vec![(Widget::AnvilName, [62.0, 24.0], [103.0, 12.0])],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;

    #[test]
    fn review_oversized_horse_storage_has_no_wrapped_slot_ids() {
        let layout = window_layout(WindowKind::Horse, 300).unwrap();
        let mut seen = std::collections::HashSet::new();
        for slot in layout.slots {
            if let InventoryCellHit::Storage(id) = slot.hit {
                assert!(seen.insert(id));
            }
        }
    }
}
