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

/// Personal crafting inputs and cursor share one admitted observation batch.
pub(crate) const PERSONAL_INPUT_COUNT: usize =
    (super::CraftingGrid::Personal.width() as usize).pow(2);
pub(crate) const PERSONAL_UPDATE_COUNT: usize = PERSONAL_INPUT_COUNT + 1;
const _: () = assert!(PERSONAL_UPDATE_COUNT <= u8::BITS as usize);

/// The canonical cell at one personal observation index, with the cursor last.
pub(super) fn personal_cell(index: usize) -> Cell {
    if index == PERSONAL_INPUT_COUNT {
        Cell::Cursor
    } else {
        Cell::Craft(*protocol::CRAFTING_INPUT_SLOTS.start() + index as u8)
    }
}

impl Cell {
    /// The personal observation index for a cursor or personal-grid write.
    pub(super) fn personal_index(self) -> Option<usize> {
        match self {
            Self::Cursor => Some(PERSONAL_INPUT_COUNT),
            Self::Craft(slot)
                if (FIRST_CRAFT_SLOT..FIRST_CRAFT_SLOT + PERSONAL_INPUT_COUNT as u8)
                    .contains(&slot) =>
            {
                Some(usize::from(slot - *protocol::CRAFTING_INPUT_SLOTS.start()))
            }
            _ => None,
        }
    }

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
    /// The stack identity before predictions and responses reassigned its wire ID.
    /// Whole moves retain it; splits and fresh slot writes start a new identity.
    pub(super) source_stack_id: Option<i32>,
    /// The first retained cell, separating siblings with the same wire identity.
    pub(super) source_origin: Option<Cell>,
    pub(super) overlay: Option<StackResponseOverlay>,
    /// The accepted request that installed this held value's server stack identity.
    pub(super) acknowledged_request_id: Option<i32>,
}

impl Held {
    /// Whether both values descend from the same retained stack across wire renaming.
    pub(super) fn same_source(&self, other: &Self) -> bool {
        self.source_stack_id.is_some()
            && self.source_stack_id == other.source_stack_id
            && self.source_origin == other.source_origin
    }

    /// Whether both values retain the same source and item data across wire renaming.
    pub(super) fn same_source_item(&self, other: &Self) -> bool {
        self.same_source(other)
            && super::PlayerInventoryLedger::same_item(&self.stack, &other.stack)
    }

    /// Names a prediction, assigning newly split stacks a distinct creation cell.
    pub(super) fn bind_prediction(&mut self, request_id: i32, cell: Cell) {
        if self.source_stack_id.is_none() {
            self.source_stack_id = Some(request_id);
            self.source_origin = Some(cell);
        }
        self.stack.stack_network_id = request_id;
    }

    /// Binds a fresh value's origin once; whole moves preserve its existing origin.
    fn with_origin(mut self, cell: Cell) -> Self {
        self.source_origin.get_or_insert(cell);
        self
    }

    /// `None` for an empty wire stack; empty cells are never retained as values.
    pub(super) fn new(stack: &NetworkItemStack) -> Option<Self> {
        (!stack.is_empty()).then(|| Self {
            source_stack_id: Some(stack.stack_network_id),
            source_origin: None,
            stack: stack.clone(),
            overlay: None,
            acknowledged_request_id: None,
        })
    }

    /// Inherits materialized item data without changing count, wire identity or lineage.
    /// Returns whether NBT, its digest or the response overlay changed.
    pub(super) fn inherit_item_data(&mut self, source: &Self) -> bool {
        if self.stack.extra_data == source.stack.extra_data
            && self.stack.nbt_digest == source.stack.nbt_digest
            && self.overlay == source.overlay
        {
            return false;
        }
        self.stack.extra_data = Arc::clone(&source.stack.extra_data);
        self.stack.nbt_digest = source.stack.nbt_digest;
        self.overlay = source.overlay.clone();
        true
    }
}

#[derive(Debug, Clone)]
pub(super) struct Cells {
    player: Arc<Vec<Option<Held>>>,
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
            player: Arc::new(vec![None; PLAYER_INVENTORY_SLOT_COUNT]),
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
    /// Compares the retained surfaces used by inventory display projections.
    pub(super) fn shares_presentation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.player, &other.player)
            && Arc::ptr_eq(&self.storage, &other.storage)
            && Arc::ptr_eq(&self.craft, &other.craft)
            && self.created_output == other.created_output
    }

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
            Cell::Inventory(slot) => usize::from(slot) < self.player.len(),
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
                *entry = value.map(|held| held.with_origin(cell));
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
        self.storage = Arc::new(
            slots
                .iter()
                .enumerate()
                .map(|(slot, stack)| {
                    Held::new(stack).map(|held| held.with_origin(Cell::Storage(slot as u8)))
                })
                .collect(),
        );
    }

    /// Replaces the player container, including its authoritative extent.
    pub(super) fn replace_player(&mut self, slots: &[NetworkItemStack]) {
        self.player = Arc::new(
            slots
                .iter()
                .enumerate()
                .map(|(slot, stack)| {
                    Held::new(stack).map(|held| held.with_origin(Cell::Inventory(slot as u8)))
                })
                .collect(),
        );
    }

    /// The currently retained player container extent.
    pub(super) fn player_len(&self) -> usize {
        self.player.len()
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

    /// Discards the preview owned by a screen without changing actual ingredients.
    pub(super) fn clear_result(&mut self) {
        self.created_output = None;
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
