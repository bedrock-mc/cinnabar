//! Authoritative inventory event admission.
//!
//! Every wire path — a full-content rewrite, an individual slot update, and
//! an accepted item stack response correction — resolves its container
//! identity through the one canonical projection
//! ([`project_container_cell`]) before any cell is touched, so a Content
//! event, a Slot event, and an accepted response naming the same physical
//! cell converge on exactly one retained cell while distinct surfaces can
//! never collide. Identities that resolve onto no retained ledger cell are
//! odd but well-formed data: a typed counted skip that mutates nothing and
//! never ends the session.

use protocol::{
    CanonicalCell, ContainerIdentity, InventoryAuthority, InventoryContentEvent, InventoryEvent,
    NetworkItemStack, SlotIdentity, project_container_cell,
};

use super::helpers::{bare_storage_window_matches, valid_storage_window_id};
use super::{
    Cell, CellSurface, GENERIC_STORAGE_WINDOW_TYPE, LARGE_STORAGE_SLOT_COUNT,
    PLAYER_INVENTORY_SLOT_COUNT, PlayerInventoryLedger, SMALL_STORAGE_SLOT_COUNT, StorageWindow,
};

impl PlayerInventoryLedger {
    pub fn apply(&mut self, event: &InventoryEvent) {
        match event {
            InventoryEvent::Authority(authority) => {
                self.authority = Some(*authority);
                if *authority != InventoryAuthority::Server {
                    self.pending = None;
                    self.cursor = None;
                    self.cursor_overlay = None;
                    self.player_resync_required = false;
                    self.cursor_resync_required = false;
                    self.storage = None;
                    self.pending_close = None;
                }
            }
            InventoryEvent::Open(open) => self.apply_open(*open),
            InventoryEvent::Close(close) => {
                if self.pending_close.is_some_and(|pending| {
                    close.container.window_id == Some(pending.window_id)
                        && close.window_type == pending.window_type
                }) {
                    self.pending_close = None;
                }
                if self.storage.as_ref().is_some_and(|storage| {
                    close.container.window_id == Some(storage.window_id)
                        && close.window_type == GENERIC_STORAGE_WINDOW_TYPE
                }) {
                    self.close_storage(false);
                }
            }
            InventoryEvent::Content(content) => self.apply_content(content),
            InventoryEvent::Slot(update) => {
                self.apply_slot_update(update.identity, &update.stack);
            }
            InventoryEvent::Response(event) => self.apply_response(event),
            _ => {}
        }
    }

    /// Admits one authoritative content rewrite through the canonical
    /// projection. A content payload addresses its surface from index zero,
    /// so the projected first cell identifies the surface.
    fn apply_content(&mut self, content: &InventoryContentEvent) {
        match project_container_cell(&content.container, 0) {
            Some(CanonicalCell::GenericStorage { .. }) => {
                self.apply_storage_content(content.container, &content.slots);
            }
            Some(CanonicalCell::PlayerInventory(_)) => {
                let complete = content.slots.len() == PLAYER_INVENTORY_SLOT_COUNT;
                let revision = self.take_authority_revision();
                for index in 0..content.slots.len().min(PLAYER_INVENTORY_SLOT_COUNT) {
                    self.slots[index] = content
                        .slots
                        .get(index)
                        .filter(|stack| !stack.is_empty())
                        .cloned();
                    self.slot_overlays[index] = None;
                    self.known[index] = true;
                    self.slot_revisions[index] = revision;
                }
                if complete {
                    self.player_resync_required = false;
                    self.cancel_pending_for_authority(CellSurface::Player, None);
                }
            }
            Some(CanonicalCell::Cursor) => {
                // The cursor holds exactly one cell; anything else is odd
                // remote data addressed to the cursor surface.
                if content.slots.len() == 1 {
                    self.cursor = content
                        .slots
                        .first()
                        .filter(|stack| !stack.is_empty())
                        .cloned();
                    self.cursor_overlay = None;
                    self.cursor_revision = self.take_authority_revision();
                    self.cursor_resync_required = false;
                    self.cancel_pending_for_authority(CellSurface::Cursor, None);
                } else {
                    self.note_unrouted_container();
                }
            }
            // Armor and offhand rewrites resolve canonically but this ledger
            // retains neither surface yet, so they stay counted skips.
            Some(CanonicalCell::Armor(_) | CanonicalCell::Offhand) | None => {
                self.note_unrouted_container();
            }
        }
    }

    /// Admits one authoritative slot update through the canonical projection.
    fn apply_slot_update(&mut self, identity: SlotIdentity, stack: &NetworkItemStack) {
        match project_container_cell(&identity.container, identity.slot) {
            Some(CanonicalCell::PlayerInventory(index)) => {
                let index = usize::from(index);
                self.slots[index] = (!stack.is_empty()).then(|| stack.clone());
                self.slot_overlays[index] = None;
                self.known[index] = true;
                let revision = self.take_authority_revision();
                self.slot_revisions[index] = revision;
            }
            Some(CanonicalCell::Cursor) => {
                self.cursor = (!stack.is_empty()).then(|| stack.clone());
                self.cursor_overlay = None;
                self.cursor_revision = self.take_authority_revision();
                self.cursor_resync_required = false;
            }
            Some(CanonicalCell::GenericStorage { slot, .. }) => {
                self.apply_storage_slot(identity.container, slot, stack);
            }
            // Prior-admission restoration: legacy bare-window traffic (a
            // Slot update whose optional container name is absent on the
            // wire) addressed the open generic-storage window by its raw
            // window id alone. The projection cannot see which windows are
            // open, so this one leg consults the retained window.
            None if bare_storage_window_matches(self.storage.as_ref(), &identity.container) => {
                self.apply_storage_slot(identity.container, identity.slot, stack);
            }
            Some(CanonicalCell::Armor(_) | CanonicalCell::Offhand) | None => {
                self.note_unrouted_container();
            }
        }
    }

    /// Maps one accepted-response correction address onto its retained ledger
    /// cell: the same canonical projection the ingress paths use, bounded by
    /// each surface's exact retention. `None` marks an unrouted identity.
    pub(super) fn retained_response_cell(
        &self,
        container: &ContainerIdentity,
        slot: u16,
    ) -> Option<Cell> {
        match project_container_cell(container, slot)? {
            CanonicalCell::PlayerInventory(index) => Some(Cell::Inventory(index)),
            CanonicalCell::Cursor => Some(Cell::Cursor),
            CanonicalCell::GenericStorage { slot, .. } => {
                let storage = self.storage.as_ref()?;
                if usize::from(slot) >= storage.slots.len() {
                    return None;
                }
                Some(Cell::Storage(u8::try_from(slot).ok()?))
            }
            CanonicalCell::Armor(_) | CanonicalCell::Offhand => None,
        }
    }

    /// Counts one well-formed authoritative event whose container identity
    /// did not resolve onto a retained canonical ledger cell: unknown
    /// container codes, unreviewed surfaces, or indices outside every mapped
    /// surface. Skipped whole — no mutation, session continues.
    pub(super) fn note_unrouted_container(&mut self) {
        self.skipped_unknown_containers = self.skipped_unknown_containers.saturating_add(1);
    }

    fn apply_open(&mut self, open: protocol::ContainerOpenEvent) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.storage_generation.is_some())
        {
            if self.pending_state() == Some(super::InventoryPendingState::AwaitingResponse) {
                self.require_authoritative_recovery();
            } else {
                self.rollback_pending();
            }
        }
        let Some(window_id) = open.container.window_id else {
            return;
        };
        if open.window_type != GENERIC_STORAGE_WINDOW_TYPE || !valid_storage_window_id(window_id) {
            self.queue_close(window_id, open.window_type);
            self.storage = None;
            return;
        }
        if self.pending_close.is_some_and(|close| {
            close.window_id == window_id && close.window_type == open.window_type
        }) {
            self.pending_close = None;
        }
        let generation = self.next_open_generation;
        self.next_open_generation = self.next_open_generation.wrapping_add(1).max(1);
        self.storage = Some(StorageWindow {
            window_id,
            generation,
            identity: None,
            slots: Vec::new(),
            revisions: Vec::new(),
            overlays: Vec::new(),
            resync_required: false,
            closing: false,
        });
    }

    fn apply_storage_content(&mut self, identity: ContainerIdentity, slots: &[NetworkItemStack]) {
        let valid_len = matches!(
            slots.len(),
            SMALL_STORAGE_SLOT_COUNT | LARGE_STORAGE_SLOT_COUNT
        );
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        let window_id = storage.window_id;
        let generation = storage.generation;
        if identity.window_id != Some(window_id) {
            return;
        }
        if storage.identity.is_some_and(|current| current != identity) {
            return;
        }
        if !valid_len {
            self.queue_close(window_id, GENERIC_STORAGE_WINDOW_TYPE);
            self.close_storage(false);
            return;
        }
        let revision = self.take_authority_revision();
        let storage = self.storage.as_mut().expect("storage remains active");
        storage.identity = Some(identity);
        storage.slots = slots
            .iter()
            .map(|stack| (!stack.is_empty()).then(|| stack.clone()))
            .collect();
        storage.revisions = vec![revision; slots.len()];
        storage.overlays = vec![None; slots.len()];
        storage.resync_required = false;
        self.cancel_pending_for_authority(CellSurface::Storage, Some(generation));
    }

    fn apply_storage_slot(
        &mut self,
        identity: ContainerIdentity,
        slot: u16,
        stack: &NetworkItemStack,
    ) {
        let Some(storage) = self.storage.as_ref() else {
            // A storage-surface event with no open window resolved canonically
            // but has no retained cell to land in: counted leniency.
            self.note_unrouted_container();
            return;
        };
        if !super::helpers::storage_slot_identity_matches(storage, identity)
            || usize::from(slot) >= storage.slots.len()
        {
            return;
        }
        let revision = self.take_authority_revision();
        let storage = self.storage.as_mut().expect("storage remains active");
        storage.slots[usize::from(slot)] = (!stack.is_empty()).then(|| stack.clone());
        storage.revisions[usize::from(slot)] = revision;
        storage.overlays[usize::from(slot)] = None;
    }

    pub(super) fn pending_identity_mismatch(&self, identity: ContainerIdentity) -> bool {
        self.pending
            .as_ref()
            .and_then(|pending| pending.storage_identity)
            .is_none_or(|expected| {
                identity.window_id.is_some()
                    || expected.slot_type != identity.slot_type
                    || expected.dynamic_id != identity.dynamic_id
            })
    }
}
