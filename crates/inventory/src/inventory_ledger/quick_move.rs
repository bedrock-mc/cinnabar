//! Whole-stack shift-click: every eligible destination in one request.

use protocol::{NetworkItemStack, WindowKind};

use super::cells::Cell;
use super::gesture::{
    InventoryTarget, StackRequestActionKind, Submission, counted_merge, counted_transfer,
    has_meaningful_overlay,
};
use super::item_roles;
use super::registry::{OccupiedStackRelation, entry_capacity};
use super::{InventoryGestureError, PlayerInventoryLedger};

/// Cells that hold one item however large the item's stack size is.
fn slot_limit(cell: Cell) -> Option<u16> {
    matches!(cell, Cell::Craft(14 | 27)).then_some(1)
}

impl PlayerInventoryLedger {
    /// Moves a hovered stack across every cell that accepts it: compatible
    /// partial stacks first, then empty cells, until nothing is left.
    pub fn begin_quick_move(
        &mut self,
        target: InventoryTarget,
    ) -> Result<i32, InventoryGestureError> {
        let source = target.cell();
        let personal_generation = self.gesture_preflight(!matches!(source, Cell::Storage(_)))?;
        self.check_surfaces([source])?;
        let from = self
            .movable(source)?
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let address = self.window_address();
        let candidates = self.quick_move_candidates(source, &from.stack);
        let item_capacity = self
            .negotiated_item_entry(from.stack.network_id)
            .and_then(entry_capacity)
            .map(u16::from);
        let mut remaining = from.stack.count;
        let mut actions = Vec::new();
        let mut groups = Vec::new();
        let (mut distinct, mut registry_bound) = (false, false);
        for cell in &candidates {
            if remaining == 0 {
                break;
            }
            let Some(into) = self.view().get(*cell) else {
                continue;
            };
            if self.awaiting_identity(into)
                || has_meaningful_overlay(from.overlay.as_ref())
                || has_meaningful_overlay(into.overlay.as_ref())
            {
                continue;
            }
            let OccupiedStackRelation::Compatible { capacity } =
                self.occupied_stack_relation(&from.stack, &into.stack)
            else {
                continue;
            };
            let limit = slot_limit(*cell).map_or(capacity, |limit| limit.min(capacity));
            let amount = remaining.min(limit.saturating_sub(into.stack.count));
            if amount == 0 {
                continue;
            }
            let built = counted_merge(
                StackRequestActionKind::Place,
                source,
                *cell,
                &from.stack,
                &into.stack,
                address,
                amount,
                capacity,
            )?;
            remaining -= amount;
            distinct |= built.requires_distinct_stack_ids;
            registry_bound |= built.registry_bound_merge;
            actions.push(built.action);
            groups.push(built.group);
        }
        for cell in &candidates {
            if remaining == 0 {
                break;
            }
            if self.view().get(*cell).is_some() {
                continue;
            }
            let limit = slot_limit(*cell).unwrap_or(u16::MAX);
            let amount = remaining
                .min(limit)
                .min(item_capacity.unwrap_or(remaining).max(1));
            let built = counted_transfer(
                StackRequestActionKind::Place,
                source,
                *cell,
                &from.stack,
                address,
                Some(amount),
            )?;
            remaining -= amount;
            distinct |= built.requires_distinct_stack_ids;
            actions.push(built.action);
            groups.push(built.group);
        }
        if actions.is_empty() {
            return Err(InventoryGestureError::InvalidRequest);
        }
        self.submit(Submission {
            actions,
            groups,
            personal_generation,
            requires_distinct_stack_ids: distinct,
            registry_bound_merge: registry_bound,
        })
    }

    /// Candidate destinations, in order, for a quick move out of `source`.
    fn quick_move_candidates(&self, source: Cell, stack: &NetworkItemStack) -> Vec<Cell> {
        let id = self
            .negotiated_item_entry(stack.network_id)
            .map_or("", |entry| &*entry.identifier);
        let player = |range: std::ops::Range<u8>| -> Vec<Cell> {
            range
                .filter(|slot| self.known[usize::from(*slot)])
                .map(Cell::Inventory)
                .collect()
        };
        let Cell::Inventory(slot) = source else {
            let mut cells = player(9..36);
            cells.extend(player(0..9));
            return cells;
        };
        let window = self.storage.as_ref().and_then(|storage| {
            (storage.identity.is_some() || storage.kind.is_ui_backed()).then_some(storage.kind)
        });
        let cells = match window {
            Some(kind) => self.window_destinations(kind, id),
            None => self.equipment_destinations(id),
        };
        if !cells.is_empty() {
            return cells;
        }
        if slot < 9 {
            player(9..36)
        } else {
            player(0..9)
        }
    }

    /// The empty armor or offhand cell an item equips into on the personal screen.
    fn equipment_destinations(&self, id: &str) -> Vec<Cell> {
        let cell = match item_roles::armor_row(id) {
            Some(row) => Cell::Armor(row),
            None if item_roles::bare(id) == "shield" => Cell::Offhand,
            None => return Vec::new(),
        };
        if self.view().get(cell).is_none() && self.confirmed.contains(cell) {
            vec![cell]
        } else {
            Vec::new()
        }
    }

    /// The window cells a stack of `id` shift-clicks into.
    fn window_destinations(&self, kind: WindowKind, id: &str) -> Vec<Cell> {
        let storage = |range: std::ops::Range<u8>| -> Vec<Cell> {
            range
                .map(Cell::Storage)
                .filter(|cell| self.confirmed.contains(*cell))
                .collect()
        };
        let ui = |slots: &[u8]| -> Vec<Cell> {
            slots
                .iter()
                .map(|slot| Cell::Craft(*slot))
                .filter(|cell| self.confirmed.contains(*cell))
                .collect()
        };
        match kind {
            WindowKind::Storage
            | WindowKind::Dispenser
            | WindowKind::Dropper
            | WindowKind::Hopper
            | WindowKind::Crafter => storage(0..u8::MAX),
            WindowKind::Furnace | WindowKind::BlastFurnace | WindowKind::Smoker => {
                let slot = item_roles::furnace_slot(id);
                storage(slot..slot + 1)
            }
            WindowKind::Brewing => item_roles::brewing_slots(id)
                .iter()
                .map(|slot| Cell::Storage(*slot))
                .filter(|cell| self.confirmed.contains(*cell))
                .collect(),
            WindowKind::Horse => match item_roles::horse_slot(id) {
                Some(slot) => storage(slot..slot + 1),
                None => storage(2..u8::MAX),
            },
            WindowKind::Anvil => ui(&[1, 2]),
            WindowKind::Enchanting => ui(&[if item_roles::is_lapis(id) { 15 } else { 14 }]),
            WindowKind::Beacon if item_roles::is_beacon_payment(id) => ui(&[27]),
            WindowKind::Loom => item_roles::loom_slot(id).map_or_else(Vec::new, |slot| ui(&[slot])),
            WindowKind::Grindstone => ui(&[16, 17]),
            WindowKind::Stonecutter => ui(&[3]),
            WindowKind::Cartography => ui(&[item_roles::cartography_slot(id)]),
            WindowKind::Smithing => ui(&[item_roles::smithing_slot(id)]),
            WindowKind::Beacon | WindowKind::Workbench | WindowKind::Lectern => Vec::new(),
        }
    }
}
