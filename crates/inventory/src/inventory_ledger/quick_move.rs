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

/// A furnace quick-move decision and the item facts it was made from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FurnaceRoute {
    window: WindowKind,
    network_id: i32,
    metadata: u32,
    /// The furnace slot chosen; `None` returned the stack within the player inventory.
    slot: Option<u8>,
}

/// Cells that hold one item however large the item's stack size is.
fn slot_limit(cell: Cell) -> Option<u16> {
    matches!(cell, Cell::Craft(14 | 27)).then_some(1)
}

impl PlayerInventoryLedger {
    pub(crate) fn begin_move_to_furnace_input(
        &mut self,
        source: InventoryTarget,
    ) -> Result<i32, InventoryGestureError> {
        if !self.confirmed.contains(Cell::Storage(0)) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        self.begin_quick_move_into(source, Some(&[Cell::Storage(0)]), None)
    }

    /// Moves a hovered stack across every cell that accepts it: compatible
    /// partial stacks first, then empty cells, until nothing is left.
    pub fn begin_quick_move(
        &mut self,
        target: InventoryTarget,
    ) -> Result<i32, InventoryGestureError> {
        self.begin_quick_move_into(target, None, None)
    }

    pub(crate) fn begin_restore_furnace_source(
        &mut self,
        source: u8,
        destination: u8,
        amount: u16,
    ) -> Result<i32, InventoryGestureError> {
        self.begin_quick_move_into(
            InventoryTarget::Storage(source),
            Some(&[Cell::Inventory(destination)]),
            Some(amount),
        )
    }

    pub(crate) fn begin_return_furnace_cell(
        &mut self,
        source: u8,
    ) -> Result<i32, InventoryGestureError> {
        let destinations: Vec<_> = (0..protocol::PLAYER_INVENTORY_SLOTS)
            .filter(|slot| self.known[usize::from(*slot)])
            .map(Cell::Inventory)
            .collect();
        self.begin_quick_move_into(InventoryTarget::Storage(source), Some(&destinations), None)
    }

    pub(super) fn begin_quick_move_into(
        &mut self,
        target: InventoryTarget,
        destinations: Option<&[Cell]>,
        amount: Option<u16>,
    ) -> Result<i32, InventoryGestureError> {
        let source = target.cell();
        let personal_generation = self.gesture_preflight(!matches!(source, Cell::Storage(_)))?;
        self.check_surfaces([source])?;
        let from = self
            .movable(source)?
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let address = self.window_address();
        let candidates = destinations.map_or_else(
            || self.quick_move_candidates(source, &from.stack),
            <[Cell]>::to_vec,
        );
        let furnace_route = destinations
            .is_none()
            .then(|| self.furnace_route(source, &from.stack))
            .flatten();
        let item_capacity = self
            .negotiated_item_entry(from.stack.network_id)
            .and_then(entry_capacity)
            .map(u16::from);
        let mut remaining = amount.map_or(from.stack.count, |amount| amount.min(from.stack.count));
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
        let request_id = self.submit(Submission {
            actions,
            groups,
            personal_generation,
            requires_distinct_stack_ids: distinct,
            registry_bound_merge: registry_bound,
        })?;
        if let Some(pending) = self
            .queue
            .iter_mut()
            .find(|pending| pending.request_id == request_id)
        {
            pending.furnace_route = furnace_route;
        }
        Ok(request_id)
    }

    /// The window a quick move out of the player inventory targets, if one is open.
    fn quick_move_window(&self) -> Option<WindowKind> {
        self.storage.as_ref().and_then(|storage| {
            (storage.identity.is_some() || storage.kind.is_ui_backed()).then_some(storage.kind)
        })
    }

    /// The furnace routing a quick move of `stack` out of the player inventory reads.
    fn furnace_route(&self, source: Cell, stack: &NetworkItemStack) -> Option<FurnaceRoute> {
        let Cell::Inventory(_) = source else {
            return None;
        };
        let window = self
            .quick_move_window()
            .filter(|kind| crate::furnace_recipes::kind(*kind).is_some())?;
        Some(FurnaceRoute {
            window,
            network_id: stack.network_id,
            metadata: stack.metadata,
            slot: self.furnace_slot(window, stack.network_id, stack.metadata),
        })
    }

    /// The furnace slot a player stack quick-moves into; `None` keeps it in the inventory.
    fn furnace_slot(&self, window: WindowKind, network_id: i32, metadata: u32) -> Option<u8> {
        if self.furnace_input_accepts(window, network_id, metadata) == Some(true) {
            return Some(0);
        }
        let fuel = self.item_fuel(network_id);
        (fuel.eligible() == Some(true)).then(|| u8::from(fuel.ingredient_first() != Some(true)))
    }

    /// Retires quick moves whose recipe or fuel facts now route them to another slot.
    pub(super) fn retire_rerouted_furnace_moves(&mut self) {
        let rerouted: Vec<i32> = self
            .queue
            .iter()
            .filter(|pending| {
                pending.furnace_route.is_some_and(|route| {
                    self.furnace_slot(route.window, route.network_id, route.metadata) != route.slot
                })
            })
            .map(|pending| pending.request_id)
            .collect();
        if !rerouted.is_empty() {
            self.abandon_requests(|pending| rerouted.contains(&pending.request_id));
        }
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
            let mut cells = player(protocol::HOTBAR_SLOT_COUNT..protocol::PLAYER_INVENTORY_SLOTS);
            cells.extend(player(0..protocol::HOTBAR_SLOT_COUNT));
            return cells;
        };
        let cells = match self.quick_move_window() {
            Some(kind) => self.window_destinations(kind, id, stack.network_id, stack.metadata),
            None => self.equipment_destinations(id),
        };
        if !cells.is_empty() {
            return cells;
        }
        if slot < protocol::HOTBAR_SLOT_COUNT {
            player(protocol::HOTBAR_SLOT_COUNT..protocol::PLAYER_INVENTORY_SLOTS)
        } else {
            player(0..protocol::HOTBAR_SLOT_COUNT)
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
    fn window_destinations(
        &self,
        kind: WindowKind,
        id: &str,
        network_id: i32,
        metadata: u32,
    ) -> Vec<Cell> {
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
            WindowKind::Furnace | WindowKind::BlastFurnace | WindowKind::Smoker => self
                .furnace_slot(kind, network_id, metadata)
                .map_or_else(Vec::new, |slot| storage(slot..slot + 1)),
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

#[cfg(test)]
mod fuel_tests {
    use super::*;
    use assets::item::fuel::ItemFuel;

    #[test]
    fn furnace_destinations_follow_numeric_fuel_facts_and_defer_unknown_items() {
        let mut ledger = PlayerInventoryLedger::default();
        ledger.apply(&protocol::InventoryEvent::Authority(
            protocol::InventoryAuthority::Server,
        ));
        ledger.apply(&protocol::InventoryEvent::Open(
            protocol::ContainerOpenEvent {
                container: protocol::ContainerIdentity::window(3),
                window_type: protocol::WINDOW_TYPE_FURNACE,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            },
        ));
        ledger.apply(&protocol::InventoryEvent::Content(
            protocol::InventoryContentEvent {
                container: protocol::ContainerIdentity::window(3),
                slots: vec![NetworkItemStack::default(); 3].into(),
                storage_item: NetworkItemStack::default(),
            },
        ));
        ledger
            .fuel_facts
            .insert(10_001, ItemFuel::Session { burn_ticks: 40 });
        ledger
            .fuel_facts
            .insert(10_002, ItemFuel::Session { burn_ticks: 0 });
        assert_eq!(
            ledger.window_destinations(WindowKind::Furnace, "custom:shared_icon", 10_001, 0),
            vec![Cell::Storage(1)]
        );
        assert!(
            ledger
                .window_destinations(WindowKind::Furnace, "custom:shared_icon", 10_002, 0)
                .is_empty()
        );
        assert!(
            ledger
                .window_destinations(WindowKind::Furnace, "custom:oak_planks", 10_003, 0)
                .is_empty()
        );
        ledger.fuel_facts.insert(
            10_004,
            assets::item::fuel::builtin_item_fuel("minecraft:oak_log"),
        );
        assert_eq!(
            ledger.window_destinations(WindowKind::Furnace, "unrelated:icon", 10_004, 0),
            vec![Cell::Storage(0)]
        );
        ledger.fuel_facts.insert(
            10_005,
            assets::item::fuel::builtin_item_fuel("minecraft:crimson_planks"),
        );
        assert!(
            ledger
                .window_destinations(WindowKind::Furnace, "minecraft:coal", 10_005, 0)
                .is_empty()
        );
    }
}
