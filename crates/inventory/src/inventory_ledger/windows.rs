//! Admission for windows whose cells are named runs (furnace, brewing stand,
//! horse, crafter) and for server-pushed window properties.

use protocol::{ContainerDataEvent, ContainerIdentity, NetworkItemStack, OpenCells};

use super::cells::{CellSurface, Held};
use super::{Cell, PlayerInventoryLedger};

/// Properties one window retains; a hostile server cannot grow the map.
const MAX_WINDOW_DATA: usize = 16;

impl PlayerInventoryLedger {
    fn legacy_named_window(&self, identity: &ContainerIdentity) -> Option<bool> {
        let id = identity.window_id?;
        if matches!(
            id,
            protocol::PLAYER_INVENTORY_WINDOW_ID
                | protocol::OFFHAND_WINDOW_ID
                | protocol::ARMOR_WINDOW_ID
                | protocol::UI_INVENTORY_WINDOW_ID
                | protocol::DYNAMIC_STORAGE_WINDOW_ID
        ) {
            return None;
        }
        let storage = self.storage.as_ref()?;
        matches!(storage.kind.open_cells(), Some(OpenCells::Named { .. }))
            .then_some(id == storage.window_id)
    }

    /// Ordinary named windows admit complete content by window ID, ignoring packet names.
    pub(super) fn apply_legacy_named_content(
        &mut self,
        identity: ContainerIdentity,
        slots: &[NetworkItemStack],
    ) -> bool {
        let Some(matches) = self.legacy_named_window(&identity) else {
            return false;
        };
        let valid_length = self.storage.as_ref().is_some_and(|storage| {
            matches!(storage.kind.open_cells(), Some(OpenCells::Named { lengths, .. }) if lengths.contains(&slots.len()))
        });
        if !matches || !valid_length {
            self.note_unrouted_container();
            return true;
        }
        for slot in 0..slots.len() {
            self.retire_legacy_write(Cell::Storage(slot as u8));
        }
        let storage = self.storage.as_mut().expect("named window observed");
        storage
            .identity
            .get_or_insert(ContainerIdentity::window(storage.window_id));
        storage.resync_required = false;
        self.confirmed.replace_storage(slots);
        self.surface_refreshed(CellSurface::Storage);
        true
    }

    /// A legacy slot's index is relative to the whole open window.
    pub(super) fn apply_legacy_named_slot(
        &mut self,
        identity: ContainerIdentity,
        slot: u16,
        stack: &NetworkItemStack,
    ) -> bool {
        let Some(matches) = self.legacy_named_window(&identity) else {
            return false;
        };
        if !matches
            || !u8::try_from(slot).is_ok_and(|slot| {
                self.set_authoritative_cell(Cell::Storage(slot), Held::new(stack))
            })
        {
            self.note_unrouted_container();
        }
        true
    }

    /// Whether `identity` is unnamed content for the open named window.
    pub(super) fn bare_named_window_matches(&self, identity: &ContainerIdentity) -> bool {
        identity.slot_type.is_none()
            && identity.dynamic_id.is_none()
            && self.storage.as_ref().is_some_and(|storage| {
                identity.window_id == Some(storage.window_id)
                    && matches!(storage.kind.open_cells(), Some(OpenCells::Named { .. }))
            })
    }

    /// Admits a content payload for the open named window: the whole window
    /// when it has the full length, otherwise the run `name` addresses.
    pub(super) fn apply_window_content(
        &mut self,
        identity: ContainerIdentity,
        name: u8,
        slots: &[NetworkItemStack],
    ) {
        let Some(storage) = self.storage.as_ref() else {
            self.note_unrouted_container();
            return;
        };
        if identity.window_id != Some(storage.window_id) {
            return;
        }
        let Some(OpenCells::Named { lengths, .. }) = storage.kind.open_cells() else {
            self.note_unrouted_container();
            return;
        };
        let max_len = lengths.iter().copied().max().unwrap_or(0);
        let first = if identity.slot_type.is_none() {
            Some(0)
        } else {
            protocol::open_name_first_cell(storage.kind, name)
        };
        let Some(first) = first else {
            self.note_unrouted_container();
            return;
        };
        let whole = slots.len() == max_len;
        if !whole && slots.len() + usize::from(first) > max_len {
            self.note_unrouted_container();
            return;
        }
        let start = if whole { 0 } else { usize::from(first) };
        for slot in start..start + slots.len() {
            self.retire_legacy_write(Cell::Storage(slot as u8));
        }
        let storage = self.storage.as_mut().expect("storage observed above");
        storage.identity.get_or_insert(identity);
        storage.resync_required = false;
        if whole {
            self.confirmed.replace_storage(slots);
        } else {
            self.confirmed
                .write_storage(usize::from(first), slots, max_len);
        }
        self.surface_refreshed(CellSurface::Storage);
    }

    /// Admits one slot update for the open named window by container name.
    pub(super) fn apply_window_slot(
        &mut self,
        identity: ContainerIdentity,
        name: u8,
        slot: u16,
        stack: &NetworkItemStack,
    ) {
        let Some(storage) = self.storage.as_ref() else {
            self.note_unrouted_container();
            return;
        };
        if identity.window_id.is_some_and(|id| id != storage.window_id) {
            return;
        }
        let index = protocol::open_name_first_cell(storage.kind, name)
            .zip(u8::try_from(slot).ok())
            .and_then(|(first, slot)| first.checked_add(slot));
        match index {
            Some(index) if self.set_authoritative_cell(Cell::Storage(index), Held::new(stack)) => {}
            _ => self.note_unrouted_container(),
        }
    }

    /// Records one `ContainerSetData` property for the open window.
    pub(super) fn apply_window_data(&mut self, data: &ContainerDataEvent) {
        let Some(storage) = self.storage.as_mut() else {
            return;
        };
        if data.container.window_id != Some(storage.window_id) {
            return;
        }
        if storage.data.len() >= MAX_WINDOW_DATA && !storage.data.contains_key(&data.property) {
            return;
        }
        storage.data.insert(data.property, data.value);
    }
}
