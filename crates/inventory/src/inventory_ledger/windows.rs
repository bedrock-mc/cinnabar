//! Admission for windows whose cells are named runs (furnace, brewing stand,
//! horse, crafter) and for server-pushed window properties.

use protocol::{ContainerDataEvent, ContainerIdentity, NetworkItemStack};

use super::cells::{CellSurface, Held};
use super::{Cell, PlayerInventoryLedger};

/// Properties one window retains; a hostile server cannot grow the map.
const MAX_WINDOW_DATA: usize = 16;

impl PlayerInventoryLedger {
    /// Separates fixed and dynamic addresses before matching an ordinary window.
    fn ordinary_window(&self, identity: &ContainerIdentity) -> Option<bool> {
        let id = identity.window_id?;
        if matches!(
            id,
            protocol::PLAYER_INVENTORY_WINDOW_ID
                | protocol::ARMOR_WINDOW_ID
                | protocol::UI_INVENTORY_WINDOW_ID
                | protocol::DYNAMIC_STORAGE_WINDOW_ID
        ) {
            return None;
        }
        let storage = self.storage.as_ref()?;
        Some(id == storage.window_id)
    }

    /// Ordinary windows admit complete content by window ID, ignoring packet names.
    pub(super) fn apply_ordinary_content(
        &mut self,
        identity: ContainerIdentity,
        slots: &[NetworkItemStack],
    ) -> bool {
        let Some(matches) = self.ordinary_window(&identity) else {
            return false;
        };
        let kind = self
            .storage
            .as_ref()
            .expect("ordinary window observed")
            .kind;
        if kind.is_ui_backed() {
            if !matches
                || super::screen_actions::input_layout(kind)
                    .is_none_or(|layout| slots.len() != layout.ordinary_extent)
            {
                self.note_unrouted_container();
                return true;
            }
            for (slot, stack) in slots.iter().enumerate() {
                if let Some(cell) = ordinary_ui_cell(kind, slot) {
                    self.set_authoritative_cell(cell, Held::new(stack));
                }
            }
            return true;
        }
        let valid_length = self
            .storage
            .as_ref()
            .is_some_and(|storage| storage.kind.content_lengths().contains(&slots.len()));
        if !matches || !valid_length {
            self.note_unrouted_container();
            return true;
        }
        for slot in 0..self.confirmed.storage_len().max(slots.len()) {
            self.prepare_authoritative_cell(Cell::Storage(slot as u8));
        }
        self.bind_storage_request_identity();
        let storage = self.storage.as_mut().expect("ordinary window observed");
        storage.resync_required = false;
        self.confirmed.replace_storage(slots);
        self.surface_refreshed(CellSurface::Storage);
        true
    }

    /// A legacy slot's index is relative to the whole open window.
    pub(super) fn apply_ordinary_slot(
        &mut self,
        identity: ContainerIdentity,
        slot: u16,
        stack: &NetworkItemStack,
    ) -> bool {
        let Some(matches) = self.ordinary_window(&identity) else {
            return false;
        };
        let kind = self
            .storage
            .as_ref()
            .expect("ordinary window observed")
            .kind;
        let cell = if kind.is_ui_backed() {
            ordinary_ui_cell(kind, usize::from(slot))
        } else {
            u8::try_from(slot).ok().map(Cell::Storage)
        };
        if !matches || !cell.is_some_and(|cell| self.set_authoritative_cell(cell, Held::new(stack)))
        {
            self.note_unrouted_container();
        }
        true
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

/// Maps incoming manager indices to writable UI inputs; preview cells do not write authority.
fn ordinary_ui_cell(kind: protocol::WindowKind, index: usize) -> Option<Cell> {
    let layout = super::screen_actions::input_layout(kind)?;
    (index < layout.ordinary_inputs).then(|| Cell::Craft(layout.slots[index]))
}
