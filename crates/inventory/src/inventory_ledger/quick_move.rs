//! Whole-stack shift-click: every eligible destination in one request.

use assets::facts::ContainerRoles;
use protocol::{NetworkItemStack, WindowKind};

use super::cells::{Cell, Held};
use super::equipment::equipment_admission;
use super::gesture::{
    InventoryTarget, StackRequestActionKind, Submission, counted_merge, counted_transfer,
};
use super::item_roles;
use super::registry::{OccupiedStackRelation, entry_capacity};
use super::{InventoryGestureError, PlayerInventoryLedger};

/// A quick-move input decision and the item facts it was made from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct QuickMoveRoute {
    window: WindowKind,
    stack: NetworkItemStack,
    /// Role-selected inputs; other windows use their fixed cells or player fallback.
    input: Option<QuickMoveInput>,
    /// At least one confirmed window destination admits the original stack.
    admitted: bool,
}

/// Input destinations selected from the current item and recipe facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QuickMoveInput {
    Single(Cell),
    StorageSlots(&'static [u8]),
}

impl QuickMoveInput {
    /// Lists eligible cells before filtering for confirmed container content.
    fn cells(self) -> impl Iterator<Item = Cell> {
        let (single, slots): (Option<Cell>, &'static [u8]) = match self {
            Self::Single(cell) => (Some(cell), &[]),
            Self::StorageSlots(slots) => (None, slots),
        };
        single
            .into_iter()
            .chain(slots.iter().copied().map(Cell::Storage))
    }
}

impl PlayerInventoryLedger {
    pub(crate) fn begin_move_to_furnace_input(
        &mut self,
        source: InventoryTarget,
    ) -> Result<i32, InventoryGestureError> {
        if !self.confirmed.contains(Cell::Storage(0)) {
            return Err(InventoryGestureError::InvalidRequest);
        }
        self.begin_quick_move_into(source, Some(&[Cell::Storage(0)]), None, |_| true)
    }

    /// Moves a hovered stack across every cell that accepts it: compatible
    /// partial stacks first, then empty cells, until nothing is left.
    pub fn begin_quick_move(
        &mut self,
        target: InventoryTarget,
    ) -> Result<i32, InventoryGestureError> {
        self.begin_quick_move_with_storage_filter(target, |_| true)
    }

    /// Moves a stack only into permitted storage cells, falling back to player
    /// inventory when no window destination is permitted. Other surfaces keep
    /// their usual routing.
    pub fn begin_quick_move_with_storage_filter(
        &mut self,
        target: InventoryTarget,
        permits_storage: impl Fn(u8) -> bool,
    ) -> Result<i32, InventoryGestureError> {
        self.begin_quick_move_into(target, None, None, permits_storage)
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
            |_| true,
        )
    }

    pub(crate) fn begin_return_furnace_cell(
        &mut self,
        source: u8,
    ) -> Result<i32, InventoryGestureError> {
        let destinations: Vec<_> = (0..protocol::PLAYER_INVENTORY_SLOTS)
            .filter(|slot| {
                self.known[usize::from(*slot)] && self.confirmed.contains(Cell::Inventory(*slot))
            })
            .map(Cell::Inventory)
            .collect();
        self.begin_quick_move_into(
            InventoryTarget::Storage(source),
            Some(&destinations),
            None,
            |_| true,
        )
    }

    /// Builds one request across explicit cells or filtered quick-move destinations.
    pub(super) fn begin_quick_move_into(
        &mut self,
        target: InventoryTarget,
        destinations: Option<&[Cell]>,
        amount: Option<u16>,
        permits_storage: impl Fn(u8) -> bool,
    ) -> Result<i32, InventoryGestureError> {
        let source = target.cell();
        let personal_generation = self.gesture_preflight(!matches!(source, Cell::Storage(_)))?;
        self.check_surfaces([source])?;
        let from = self
            .movable(source)?
            .ok_or(InventoryGestureError::EmptyGesture)?;
        let address = self.window_address();
        let candidates = destinations.map_or_else(
            || self.quick_move_candidates(source, &from, permits_storage),
            <[Cell]>::to_vec,
        );
        let quick_move_route = destinations
            .is_none()
            .then(|| self.quick_move_route(source, &from.stack))
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
            if self.awaiting_identity(into) {
                continue;
            }
            let OccupiedStackRelation::Compatible { capacity } =
                self.occupied_stack_relation(&from, into)
            else {
                continue;
            };
            let limit = self
                .placement_capacity(*cell, &from.stack)
                .map_or(capacity, |limit| limit.min(capacity));
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
            let limit = self
                .placement_capacity(*cell, &from.stack)
                .unwrap_or(u16::MAX);
            let amount = remaining
                .min(limit)
                .min(item_capacity.unwrap_or(remaining).max(1));
            if amount == 0 {
                continue;
            }
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
            pending.quick_move_route = quick_move_route;
        }
        Ok(request_id)
    }

    /// The window a quick move out of the player inventory targets, if one is open.
    fn quick_move_window(&self) -> Option<WindowKind> {
        self.storage.as_ref().and_then(|storage| {
            (storage.identity.is_some() || storage.kind.is_ui_backed()).then_some(storage.kind)
        })
    }

    /// Retains input routing facts for a quick move out of the player inventory.
    fn quick_move_route(&self, source: Cell, stack: &NetworkItemStack) -> Option<QuickMoveRoute> {
        let Cell::Inventory(_) = source else {
            return None;
        };
        Some(self.quick_move_route_for_window(self.quick_move_window()?, stack))
    }

    /// Retains both routing preferences and stack-aware admission for later fact refreshes.
    fn quick_move_route_for_window(
        &self,
        window: WindowKind,
        stack: &NetworkItemStack,
    ) -> QuickMoveRoute {
        QuickMoveRoute {
            window,
            stack: stack.clone(),
            input: self.quick_move_input_route(window, stack.network_id, stack.metadata),
            admitted: self
                .window_destinations(window, stack.network_id, stack.metadata)
                .iter()
                .any(|cell| self.placement_capacity(*cell, stack) != Some(0)),
        }
    }

    /// Resolves role-selected inputs, leaving fixed layouts and player fallback to the caller.
    fn quick_move_input_route(
        &self,
        window: WindowKind,
        network_id: i32,
        metadata: u32,
    ) -> Option<QuickMoveInput> {
        if crate::furnace_recipes::kind(window).is_some() {
            self.furnace_slot(window, network_id, metadata)
                .map(Cell::Storage)
                .map(QuickMoveInput::Single)
        } else {
            let fact = self.item_facts.get(network_id);
            match window {
                WindowKind::Horse => item_roles::horse_slot(fact)
                    .map(Cell::Storage)
                    .map(QuickMoveInput::Single),
                WindowKind::Loom => {
                    item_roles::loom_slot(fact, self.item_facts.is_banner(network_id))
                        .map(Cell::Craft)
                        .map(QuickMoveInput::Single)
                }
                WindowKind::Smithing => item_roles::smithing_slot(fact)
                    .map(Cell::Craft)
                    .map(QuickMoveInput::Single),
                WindowKind::Brewing => Some(QuickMoveInput::StorageSlots(
                    item_roles::brewing_slots(fact),
                )),
                WindowKind::Enchanting => Some(QuickMoveInput::Single(Cell::Craft(
                    if fact.roles.contains(ContainerRoles::ENCHANTING_LAPIS) {
                        15
                    } else {
                        14
                    },
                ))),
                WindowKind::Beacon => fact
                    .roles
                    .contains(ContainerRoles::BEACON_PAYMENT)
                    .then_some(QuickMoveInput::Single(Cell::Craft(27))),
                _ => None,
            }
        }
    }

    /// The furnace slot a player stack quick-moves into; `None` keeps it in the inventory.
    fn furnace_slot(&self, window: WindowKind, network_id: i32, metadata: u32) -> Option<u8> {
        if self.furnace_input_accepts(window, network_id, metadata) == Some(true) {
            return Some(0);
        }
        let fuel = self.item_fuel(network_id);
        (fuel.eligible() == Some(true)).then(|| u8::from(fuel.ingredient_first() != Some(true)))
    }

    /// Retires quick moves whose recipes or admitted item facts change their destinations.
    pub(super) fn retire_rerouted_quick_moves(&mut self) {
        let rerouted: Vec<i32> = self
            .queue
            .iter()
            .filter(|pending| {
                pending.quick_move_route.as_ref().is_some_and(|route| {
                    self.quick_move_route_for_window(route.window, &route.stack) != *route
                })
            })
            .map(|pending| pending.request_id)
            .collect();
        if !rerouted.is_empty() {
            self.abandon_requests(|pending| rerouted.contains(&pending.request_id));
        }
    }

    /// Candidate destinations, in order, for a quick move out of `source`.
    fn quick_move_candidates(
        &self,
        source: Cell,
        from: &Held,
        permits_storage: impl Fn(u8) -> bool,
    ) -> Vec<Cell> {
        let stack = &from.stack;
        let player = |range: std::ops::Range<u8>| -> Vec<Cell> {
            range
                .filter(|slot| {
                    self.known[usize::from(*slot)]
                        && self.confirmed.contains(Cell::Inventory(*slot))
                })
                .map(Cell::Inventory)
                .collect()
        };
        let Cell::Inventory(slot) = source else {
            if source == Cell::Offhand
                || (matches!(source, Cell::Storage(_))
                    && self.quick_move_window() == Some(WindowKind::Storage))
            {
                return player(0..protocol::PLAYER_INVENTORY_SLOTS);
            }
            let mut cells = player(protocol::HOTBAR_SLOT_COUNT..protocol::PLAYER_INVENTORY_SLOTS);
            cells.extend(player(0..protocol::HOTBAR_SLOT_COUNT));
            return cells;
        };
        let mut cells = match self.quick_move_window() {
            Some(kind) => self.window_destinations(kind, stack.network_id, stack.metadata),
            None => self.equipment_destinations(from),
        };
        cells.retain(|cell| {
            !matches!(cell, Cell::Storage(slot) if !permits_storage(*slot))
                && self.placement_capacity(*cell, stack) != Some(0)
        });
        if !cells.is_empty() {
            return cells;
        }
        if slot < protocol::HOTBAR_SLOT_COUNT {
            player(protocol::HOTBAR_SLOT_COUNT..protocol::PLAYER_INVENTORY_SLOTS)
        } else {
            player(0..protocol::HOTBAR_SLOT_COUNT)
        }
    }

    /// The first equipment cell that admits an empty placement or a compatible merge.
    fn equipment_destinations(&self, from: &Held) -> Vec<Cell> {
        let stack = &from.stack;
        let admission = equipment_admission(&self.item_facts, stack.network_id);
        admission
            .armor
            .map(Cell::Armor)
            .into_iter()
            .chain((admission.offhand == Some(true)).then_some(Cell::Offhand))
            .find(|cell| {
                self.confirmed.contains(*cell)
                    && self.view().get(*cell).is_none_or(|held| {
                        if self.awaiting_identity(held) {
                            return false;
                        }
                        match self.occupied_stack_relation(from, held) {
                            OccupiedStackRelation::Compatible { capacity } => {
                                let capacity = self
                                    .placement_capacity(*cell, &from.stack)
                                    .unwrap_or(capacity)
                                    .min(capacity);
                                held.stack.count < capacity
                            }
                            _ => false,
                        }
                    })
            })
            .into_iter()
            .collect()
    }

    /// The window cells a negotiated stack shift-clicks into.
    fn window_destinations(&self, kind: WindowKind, network_id: i32, metadata: u32) -> Vec<Cell> {
        let input = self.quick_move_input_route(kind, network_id, metadata);
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
            WindowKind::Horse if input.is_none() => storage(2..u8::MAX),
            WindowKind::Anvil => ui(&[1, 2]),
            WindowKind::Grindstone => ui(&[16, 17]),
            WindowKind::Stonecutter => ui(&[3]),
            WindowKind::Cartography => ui(&[item_roles::cartography_slot(
                self.negotiated_item_entry(network_id)
                    .map_or("", |entry| &entry.identifier),
            )]),
            _ => input
                .into_iter()
                .flat_map(QuickMoveInput::cells)
                .filter(|cell| self.confirmed.contains(*cell))
                .collect(),
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
            ledger.window_destinations(WindowKind::Furnace, 10_001, 0),
            vec![Cell::Storage(1)]
        );
        assert!(
            ledger
                .window_destinations(WindowKind::Furnace, 10_002, 0)
                .is_empty()
        );
        assert!(
            ledger
                .window_destinations(WindowKind::Furnace, 10_003, 0)
                .is_empty()
        );
        ledger.fuel_facts.insert(
            10_004,
            assets::item::fuel::builtin_item_fuel("minecraft:oak_log"),
        );
        assert_eq!(
            ledger.window_destinations(WindowKind::Furnace, 10_004, 0),
            vec![Cell::Storage(0)]
        );
        ledger.fuel_facts.insert(
            10_005,
            assets::item::fuel::builtin_item_fuel("minecraft:crimson_planks"),
        );
        assert!(
            ledger
                .window_destinations(WindowKind::Furnace, 10_005, 0)
                .is_empty()
        );
    }
}
