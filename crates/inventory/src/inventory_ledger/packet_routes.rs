//! Packet-specific UI and dynamic storage admission.

use protocol::{ContainerIdentity, InventoryContentEvent, NetworkItemStack, SlotIdentity};
use protocol_ids::ContainerName;

use super::PlayerInventoryLedger;
use super::admission::ui_cell;
use super::cells::{Cell, CellSurface, Held};

impl PlayerInventoryLedger {
    /// The UI window replaces all 54 backing cells, regardless of its packet name.
    pub(super) fn apply_ui_content(&mut self, content: &InventoryContentEvent) -> bool {
        if content.container.window_id != Some(protocol::UI_INVENTORY_WINDOW_ID) {
            return false;
        }
        if content.slots.len() != protocol::UI_SLOT_COUNT {
            self.note_unrouted_container();
            return true;
        }
        self.set_authoritative_cell(Cell::Cursor, Held::new(&content.slots[0]));
        for (slot, stack) in content.slots.iter().enumerate().skip(1) {
            if let Some(cell) = ui_cell(slot as u8) {
                self.set_authoritative_cell(cell, Held::new(stack));
            }
        }
        self.cursor_resync_required = false;
        self.crafting_resync_required = false;
        self.surface_refreshed(CellSurface::Cursor);
        self.surface_refreshed(CellSurface::Crafting);
        true
    }

    /// UI Slot packets use their raw index, including retained server result previews.
    pub(super) fn apply_ui_slot(
        &mut self,
        identity: SlotIdentity,
        stack: &NetworkItemStack,
    ) -> bool {
        if identity.container.window_id != Some(protocol::UI_INVENTORY_WINDOW_ID) {
            return false;
        }
        if identity.slot == 0 {
            self.set_authoritative_cell(Cell::Cursor, Held::new(stack));
            self.cursor_resync_required = false;
            self.surface_refreshed(CellSurface::Cursor);
        } else if let Some(cell) = u8::try_from(identity.slot).ok().and_then(ui_cell) {
            self.set_authoritative_cell(cell, Held::new(stack));
        } else {
            self.note_unrouted_container();
        }
        true
    }

    /// A dynamic key needs a retained entry or an item with both required storage components.
    pub(super) fn admit_bundle_key(&self, id: u32, item: Option<&NetworkItemStack>) -> bool {
        self.bundles.contains_key(&id)
            || item.filter(|item| !item.is_empty()).is_some_and(|item| {
                self.negotiated_item_entry(item.network_id)
                    .and_then(|entry| entry.components.as_ref())
                    .is_some_and(|components| components.storage_container)
            })
    }
    /// Registry Content addresses a registered model independently of the legacy window ID.
    pub(super) fn apply_registry_content(&mut self, content: &InventoryContentEvent) -> bool {
        if !is_fixed_registry_key(content.container) {
            return false;
        }
        let Some(cells) = self.registered_cells(content.container) else {
            self.note_unrouted_container();
            return true;
        };
        for (&cell, stack) in cells.iter().zip(content.slots.iter()) {
            self.write_registered_cell(cell, stack);
        }
        if content.slots.len() > cells.len() {
            self.note_unrouted_container();
        }
        if content.slots.len() >= cells.len() {
            if cells
                .iter()
                .copied()
                .eq((0..self.confirmed.player_len()).map(|slot| Cell::Inventory(slot as u8)))
            {
                self.player_resync_required = false;
                self.surface_refreshed(CellSurface::Player);
            } else if cells
                .iter()
                .copied()
                .eq((0..self.confirmed.storage_len()).map(|slot| Cell::Storage(slot as u8)))
            {
                if let Some(storage) = &mut self.storage {
                    storage.resync_required = false;
                }
                self.surface_refreshed(CellSurface::Storage);
            }
        }
        true
    }

    /// Registry Slot indices are relative to the registered model, including player segments.
    pub(super) fn apply_registry_slot(
        &mut self,
        identity: SlotIdentity,
        stack: &NetworkItemStack,
    ) -> bool {
        if !is_fixed_registry_key(identity.container) {
            return false;
        }
        let cell = self
            .registered_cells(identity.container)
            .and_then(|cells| cells.get(usize::from(identity.slot)).copied());
        if let Some(cell) = cell {
            self.write_registered_cell(cell, stack);
        } else {
            self.note_unrouted_container();
        }
        true
    }

    /// Resolves fixed registry keys without discarding their optional dynamic identity.
    fn registered_cells(&self, identity: ContainerIdentity) -> Option<Vec<Cell>> {
        if identity.dynamic_id.is_some() {
            return None;
        }
        if self.storage.is_none() && self.personal.is_none() {
            return None;
        }
        let name = identity.slot_type?;
        let cells: Vec<_> = match ContainerName::from_wire(name)? {
            ContainerName::Cursor => vec![Cell::Cursor],
            ContainerName::HotBar => (0..self
                .confirmed
                .player_len()
                .min(usize::from(protocol::HOTBAR_SLOT_COUNT)))
                .map(|slot| Cell::Inventory(slot as u8))
                .collect(),
            ContainerName::Inventory => (usize::from(protocol::HOTBAR_SLOT_COUNT)
                ..self.confirmed.player_len())
                .map(|slot| Cell::Inventory(slot as u8))
                .collect(),
            ContainerName::CombinedHotBarAndInventory => (0..self.confirmed.player_len())
                .map(|slot| Cell::Inventory(slot as u8))
                .collect(),
            _ => {
                let storage = self.storage.as_ref()?;
                if storage.kind.open_cells().is_some() {
                    (0..self.confirmed.storage_len())
                        .filter_map(|slot| {
                            let (container, _) = protocol::open_cell_request(
                                storage.kind,
                                slot as u8,
                                None,
                                storage.identity.and_then(|identity| identity.slot_type),
                            )?;
                            let registered_name = match container {
                                protocol::StackRequestContainer::LevelEntity { .. } => {
                                    protocol::CONTAINER_NAME_LEVEL_ENTITY
                                }
                                protocol::StackRequestContainer::OpenWindow { name, .. } => name,
                                _ => return None,
                            };
                            (registered_name == name).then_some(Cell::Storage(slot as u8))
                        })
                        .collect()
                } else {
                    super::crafting_close::screen_input_cells(Some(storage.kind))
                        .filter(|cell| matches!(cell, Cell::Craft(slot) if protocol::ui_slot_container_name(*slot) == Some(name)))
                        .collect()
                }
            }
        };
        (!cells.is_empty()).then_some(cells)
    }

    /// Publishes one model-relative correction and its cell-specific observation state.
    fn write_registered_cell(&mut self, cell: Cell, stack: &NetworkItemStack) {
        if !self.set_authoritative_cell(cell, Held::new(stack)) {
            self.note_unrouted_container();
            return;
        }
        match cell {
            Cell::Inventory(slot) => {
                self.known[usize::from(slot)] = true;
                self.note_authoritative_write(cell);
            }
            Cell::Cursor => {
                self.cursor_resync_required = false;
                self.surface_refreshed(CellSurface::Cursor);
            }
            Cell::Storage(_) => self.bind_storage_request_identity(),
            _ => {}
        }
    }
}

/// Whether this packet belongs to fixed registry routing rather than dynamic storage.
fn is_fixed_registry_key(identity: ContainerIdentity) -> bool {
    identity.window_id == Some(protocol::DYNAMIC_STORAGE_WINDOW_ID)
        && identity.slot_type != Some(protocol::CONTAINER_NAME_DYNAMIC)
}
