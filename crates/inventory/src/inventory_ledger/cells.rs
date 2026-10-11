//! Retained cell values shared by confirmed server truth and the folded view.

use protocol::NetworkItemStack;
use std::sync::Arc;

use super::{PLAYER_INVENTORY_SLOT_COUNT, StackResponseOverlay};

/// Armor cells including the body slot.
pub(super) const ARMOR_CELLS: usize = protocol::ARMOR_SLOTS as usize;
/// UI inventory slot of the first crafting cell.
pub(super) const FIRST_CRAFT_SLOT: u8 = *protocol::CRAFTING_INPUT_SLOTS.start();

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum Cell {
    Inventory(u8),
    Storage(u8),
    Cursor,
    Armor(u8),
    Offhand,
    /// One screen input or crafting cell addressed by its UI inventory slot.
    Craft(u8),
    CreatedOutput,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum CellSurface {
    Player,
    Storage,
    Cursor,
    Armor,
    Offhand,
    /// Crafting cells and created output: the rest of the UI inventory.
    Crafting,
}

impl Cell {
    pub(super) const fn surface(self) -> CellSurface {
        match self {
            Self::Inventory(_) => CellSurface::Player,
            Self::Storage(_) => CellSurface::Storage,
            Self::Cursor => CellSurface::Cursor,
            Self::Armor(_) => CellSurface::Armor,
            Self::Offhand => CellSurface::Offhand,
            Self::Craft(_) | Self::CreatedOutput => CellSurface::Crafting,
        }
    }
}

/// One non-empty stack plus the response overlay that travels with it.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct Held {
    pub(super) stack: NetworkItemStack,
    pub(super) overlay: Option<StackResponseOverlay>,
}

impl Held {
    /// `None` for an empty wire stack; empty cells are never retained as values.
    pub(super) fn new(stack: &NetworkItemStack) -> Option<Self> {
        (!stack.is_empty()).then(|| Self {
            stack: stack.clone(),
            overlay: None,
        })
    }
}

#[derive(Debug, Clone)]
pub(super) struct Cells {
    player: Arc<[Option<Held>; PLAYER_INVENTORY_SLOT_COUNT]>,
    cursor: Option<Held>,
    storage: Arc<Vec<Option<Held>>>,
    armor: Arc<[Option<Held>; ARMOR_CELLS]>,
    offhand: Option<Held>,
    /// Screen inputs and grids, indexed by UI inventory slot.
    craft: Arc<[Option<Held>; protocol::UI_SLOT_COUNT]>,
    created_output: Option<Held>,
}

impl Default for Cells {
    fn default() -> Self {
        Self {
            player: Arc::new(std::array::from_fn(|_| None)),
            cursor: None,
            storage: Arc::default(),
            armor: Arc::new(std::array::from_fn(|_| None)),
            offhand: None,
            craft: Arc::new(std::array::from_fn(|_| None)),
            created_output: None,
        }
    }
}

impl Cells {
    pub(super) fn get(&self, cell: Cell) -> Option<&Held> {
        match cell {
            Cell::Inventory(slot) => self.player.get(usize::from(slot))?.as_ref(),
            Cell::Storage(slot) => self.storage.get(usize::from(slot))?.as_ref(),
            Cell::Cursor => self.cursor.as_ref(),
            Cell::Armor(slot) => self.armor.get(usize::from(slot))?.as_ref(),
            Cell::Offhand => self.offhand.as_ref(),
            Cell::Craft(slot) => self.craft.get(craft_index(slot)?)?.as_ref(),
            Cell::CreatedOutput => self.created_output.as_ref(),
        }
    }

    pub(super) fn get_mut(&mut self, cell: Cell) -> Option<&mut Held> {
        self.entry(cell)?.as_mut()
    }

    /// Whether `cell` addresses a retained position at all.
    pub(super) fn contains(&self, cell: Cell) -> bool {
        match cell {
            Cell::Inventory(slot) => usize::from(slot) < PLAYER_INVENTORY_SLOT_COUNT,
            Cell::Storage(slot) => usize::from(slot) < self.storage.len(),
            Cell::Armor(slot) => usize::from(slot) < ARMOR_CELLS,
            Cell::Craft(slot) => craft_index(slot).is_some(),
            Cell::Cursor | Cell::Offhand | Cell::CreatedOutput => true,
        }
    }

    /// Writes one cell; out-of-range addresses are ignored and report `false`.
    pub(super) fn set(&mut self, cell: Cell, value: Option<Held>) -> bool {
        match self.entry(cell) {
            Some(entry) => {
                *entry = value;
                true
            }
            None => false,
        }
    }

    pub(super) fn take(&mut self, cell: Cell) -> Option<Held> {
        self.entry(cell)?.take()
    }

    fn entry(&mut self, cell: Cell) -> Option<&mut Option<Held>> {
        match cell {
            Cell::Inventory(slot) => Arc::make_mut(&mut self.player).get_mut(usize::from(slot)),
            Cell::Storage(slot) => Arc::make_mut(&mut self.storage).get_mut(usize::from(slot)),
            Cell::Cursor => Some(&mut self.cursor),
            Cell::Armor(slot) => Arc::make_mut(&mut self.armor).get_mut(usize::from(slot)),
            Cell::Offhand => Some(&mut self.offhand),
            Cell::Craft(slot) => Arc::make_mut(&mut self.craft).get_mut(craft_index(slot)?),
            Cell::CreatedOutput => Some(&mut self.created_output),
        }
    }

    pub(super) fn replace_storage(&mut self, slots: &[NetworkItemStack]) {
        self.storage = Arc::new(slots.iter().map(Held::new).collect());
    }

    /// Overwrites the run starting at `first`, growing the window up to `max_len`.
    pub(super) fn write_storage(
        &mut self,
        first: usize,
        slots: &[NetworkItemStack],
        max_len: usize,
    ) {
        let end = first.saturating_add(slots.len()).min(max_len);
        self.ensure_storage(end);
        for (offset, stack) in slots.iter().enumerate() {
            let index = first + offset;
            if index >= end {
                break;
            }
            Arc::make_mut(&mut self.storage)[index] = Held::new(stack);
        }
    }

    /// Grows the window to at least `len` empty cells.
    pub(super) fn ensure_storage(&mut self, len: usize) {
        if self.storage.len() < len {
            Arc::make_mut(&mut self.storage).resize_with(len, || None);
        }
    }

    pub(super) fn storage_len(&self) -> usize {
        self.storage.len()
    }

    pub(super) fn clear_storage(&mut self) {
        self.storage = Arc::default();
    }

    /// Empties every screen input, grid cell and the created output.
    pub(super) fn clear_ui(&mut self) {
        self.craft = Arc::new(std::array::from_fn(|_| None));
        self.created_output = None;
    }

    /// Every occupied retained cell in address order.
    pub(super) fn occupied(&self) -> impl Iterator<Item = (Cell, &Held)> {
        let player = self
            .player
            .iter()
            .enumerate()
            .filter_map(|(slot, held)| Some((Cell::Inventory(slot as u8), held.as_ref()?)));
        let cursor = self.cursor.as_ref().map(|held| (Cell::Cursor, held));
        let storage = self
            .storage
            .iter()
            .enumerate()
            .filter_map(|(slot, held)| Some((Cell::Storage(slot as u8), held.as_ref()?)));
        let armor = self
            .armor
            .iter()
            .enumerate()
            .filter_map(|(slot, held)| Some((Cell::Armor(slot as u8), held.as_ref()?)));
        let offhand = self.offhand.as_ref().map(|held| (Cell::Offhand, held));
        let craft = self
            .craft
            .iter()
            .enumerate()
            .filter_map(|(index, held)| Some((Cell::Craft(index as u8), held.as_ref()?)));
        let output = self
            .created_output
            .as_ref()
            .map(|held| (Cell::CreatedOutput, held));
        player
            .chain(cursor)
            .chain(storage)
            .chain(armor)
            .chain(offhand)
            .chain(craft)
            .chain(output)
    }
}

/// UI slots that hold a screen cell; the created output has its own cell.
fn craft_index(slot: u8) -> Option<usize> {
    (slot != protocol::CREATED_OUTPUT_SLOT && protocol::ui_slot_container_name(slot).is_some())
        .then_some(usize::from(slot))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloned_cells_share_storage_and_only_written_surfaces_detach() {
        let mut cells = Cells::default();
        cells.ensure_storage(512);
        let captured = cells.clone();
        assert!(Arc::ptr_eq(&cells.player, &captured.player));
        assert!(Arc::ptr_eq(&cells.storage, &captured.storage));
        let mut stack = NetworkItemStack::empty();
        stack.network_id = 1;
        stack.count = 1;
        cells.set(Cell::Storage(4), Held::new(&stack));
        assert!(captured.get(Cell::Storage(4)).is_none());
        assert_eq!(cells.get(Cell::Storage(4)).unwrap().stack.count, 1);
        assert!(!Arc::ptr_eq(&cells.storage, &captured.storage));
        assert!(Arc::ptr_eq(&cells.player, &captured.player));
        assert!(Arc::ptr_eq(&cells.craft, &captured.craft));
    }
}
