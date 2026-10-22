//! Authoritative inventory event admission.
//!
//! Packet windows and named response containers have separate address rules.
//! Each admitted write resolves onto one retained cell; unknown addresses are
//! counted and skipped without ending the session.

use protocol::{
    CanonicalCell, ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack,
    SlotIdentity, project_container_cell,
};

use super::cells::{ARMOR_CELLS, CellSurface, FIRST_CRAFT_SLOT, Held};
use super::control::ControlWindow;
use super::helpers::valid_storage_window_id;
use super::{
    Cell, LARGE_STORAGE_SLOT_COUNT, PLAYER_INVENTORY_SLOT_COUNT, PlayerInventoryLedger,
    SMALL_STORAGE_SLOT_COUNT, StorageWindow,
};

impl PlayerInventoryLedger {
    pub fn apply(&mut self, event: &InventoryEvent) {
        self.personal_writes = 0;
        for update in event.slot_updates() {
            tracing::debug!(target: "cinnabar::inventory_requests",
                identity = ?update.identity, network_id = update.stack.network_id,
                stack_id = update.stack.stack_network_id, count = update.stack.count,
                "authoritative inventory slot");
        }
        if let InventoryEvent::Content(content) = event {
            tracing::debug!(target: "cinnabar::inventory_requests",
                container = ?content.container, slots = content.slots.len(),
                items = ?content.slots.iter().take(PLAYER_INVENTORY_SLOT_COUNT)
                    .map(|stack| (stack.network_id, stack.stack_network_id, stack.count))
                    .collect::<Vec<_>>(),
                "authoritative inventory content");
        }
        self.admit(event);
        self.refold();
        self.reconcile_personal();
    }

    fn admit(&mut self, event: &InventoryEvent) {
        match event {
            // Recipe execution is not activated by protocol admission alone.
            InventoryEvent::Recipes(_) => {}
            InventoryEvent::Authority(authority) => {
                let changed = self
                    .authority
                    .is_some_and(|previous| previous != *authority);
                if changed {
                    self.confirmed.clear_result();
                    self.personal = None;
                    self.confirmed.set(Cell::Cursor, None);
                    self.storage = None;
                    self.controls.personal.desired_open = false;
                    self.controls.storage.desired_open = false;
                    self.abandoned.clear();
                    self.discard_requests(|_| true);
                }
                self.authority = Some(*authority);
                if changed {
                    self.verify_backing_authority();
                }
            }
            InventoryEvent::Open(open) => self.apply_open(*open),
            InventoryEvent::Trade(trade) => self.apply_trade(trade),
            InventoryEvent::Close(close) => self.apply_close(*close),
            InventoryEvent::RegistryCleanup(removed) => self.apply_bundle_cleanup(removed),
            InventoryEvent::Content(content) => self.apply_content(content),
            InventoryEvent::Slot(update) => {
                self.apply_slot_update(
                    update.identity,
                    &update.stack,
                    update.storage_item.as_ref(),
                );
            }
            InventoryEvent::Transaction(transaction) => {
                self.skipped_unknown_containers = self
                    .skipped_unknown_containers
                    .saturating_add(transaction.skipped_actions as u64);
                if transaction.skipped_actions != 0 {
                    tracing::warn!(target: "cinnabar::inventory_requests",
                        count = transaction.skipped_actions,
                        "skipped unsupported normal transaction actions");
                }
                if transaction.slots.iter().any(|update| {
                    update.identity.container.window_id
                        == Some(protocol::PLAYER_INVENTORY_WINDOW_ID)
                        && usize::from(update.identity.slot) >= self.confirmed.player_len()
                }) {
                    self.skipped_unknown_containers = self
                        .skipped_unknown_containers
                        .saturating_add(transaction.slots.len() as u64);
                    return;
                }
                for update in transaction.slots.iter() {
                    let mut identity = update.identity;
                    if identity.container.window_id == Some(protocol::OFFHAND_WINDOW_ID) {
                        identity.container = ContainerIdentity {
                            window_id: None,
                            slot_type: Some(protocol::CONTAINER_NAME_OFFHAND),
                            dynamic_id: None,
                        };
                    }
                    self.apply_slot_update(identity, &update.stack, None);
                }
            }
            InventoryEvent::Response(event) => self.apply_response(event),
            InventoryEvent::Data(data) => self.apply_window_data(data),
            InventoryEvent::EnchantOptions(options) => {
                if self.storage.is_some() {
                    self.enchant_options = Some(std::sync::Arc::clone(&options.options));
                }
            }
            InventoryEvent::Creative(content) => self.creative = Some(content.clone()),
            _ => {}
        }
    }

    /// Admits one authoritative content rewrite through the canonical
    /// projection. A content payload addresses its surface from index zero,
    /// so the projected first cell identifies the surface.
    fn apply_content(&mut self, content: &InventoryContentEvent) {
        self.trace_storage_sized_content(content);
        if self.apply_ui_content(content) || self.apply_registry_content(content) {
            return;
        }
        if content.container.window_id != Some(protocol::OFFHAND_WINDOW_ID)
            && self.apply_ordinary_content(content.container, &content.slots)
        {
            return;
        }
        if content.container.window_id == Some(protocol::DYNAMIC_STORAGE_WINDOW_ID)
            && content.container.slot_type == Some(protocol::CONTAINER_NAME_DYNAMIC)
        {
            if let Some(id) = content.container.dynamic_id
                && self.admit_bundle_key(id, Some(&content.storage_item))
            {
                self.apply_bundle_content(id, &content.slots);
            } else {
                self.note_unrouted_container();
            }
            return;
        }
        match project_container_cell(&content.container, 0) {
            Some(CanonicalCell::PlayerInventory(_)) => {
                let count = content.slots.len().min(PLAYER_INVENTORY_SLOT_COUNT);
                for index in 0..self.confirmed.player_len().max(count) {
                    self.prepare_authoritative_cell(Cell::Inventory(index as u8));
                    self.note_authoritative_write(Cell::Inventory(index as u8));
                }
                self.confirmed.replace_player(&content.slots[..count]);
                self.known.fill(true);
                self.player_resync_required = false;
                self.surface_refreshed(CellSurface::Player);
            }
            Some(CanonicalCell::Cursor) => {
                // The cursor holds exactly one cell; anything else is odd
                // remote data addressed to the cursor surface.
                if let [stack] = content.slots.as_ref() {
                    self.set_authoritative_cell(Cell::Cursor, Held::new(stack));
                    self.cursor_resync_required = false;
                    self.surface_refreshed(CellSurface::Cursor);
                } else {
                    self.note_unrouted_container();
                }
            }
            Some(CanonicalCell::Armor(_)) => {
                for (slot, stack) in content.slots.iter().take(ARMOR_CELLS).enumerate() {
                    self.set_authoritative_cell(Cell::Armor(slot as u8), Held::new(stack));
                }
                if content.slots.len() >= ARMOR_CELLS - 1 {
                    self.armor_resync_required = false;
                    self.surface_refreshed(CellSurface::Armor);
                }
            }
            Some(CanonicalCell::Offhand) => {
                if let Some(stack) = content.slots.first() {
                    self.set_authoritative_cell(Cell::Offhand, Held::new(stack));
                    self.offhand_resync_required = false;
                    self.surface_refreshed(CellSurface::Offhand);
                } else {
                    self.note_unrouted_container();
                }
            }
            Some(CanonicalCell::UiSlot(slot)) => match content.slots.as_ref() {
                [stack] => {
                    self.set_authoritative_cell(Cell::Craft(slot), Held::new(stack));
                }
                _ => self.note_unrouted_container(),
            },
            Some(
                CanonicalCell::GenericStorage { .. }
                | CanonicalCell::WindowSlot { .. }
                | CanonicalCell::CraftInput(_)
                | CanonicalCell::TableCraftInput(_)
                | CanonicalCell::CreatedOutput,
            )
            | None => {
                self.note_unrouted_container();
            }
        }
    }

    /// Emits a bounded prefix of fixed-field storage-sized content diagnostics
    /// for each ledger session. Enable only this target with
    /// `RUST_LOG=cinnabar::inventory_storage_admission=debug`.
    ///
    /// Item descriptors and payload bytes are deliberately excluded. This is
    /// observation only: routing and admission remain unchanged.
    fn trace_storage_sized_content(&mut self, content: &InventoryContentEvent) {
        if !matches!(
            content.slots.len(),
            SMALL_STORAGE_SLOT_COUNT | LARGE_STORAGE_SLOT_COUNT
        ) || self.storage_content_traces_remaining == 0
        {
            return;
        }
        self.storage_content_traces_remaining -= 1;
        let projection = match project_container_cell(&content.container, 0) {
            Some(CanonicalCell::GenericStorage { .. }) => "generic_storage",
            Some(CanonicalCell::PlayerInventory(_)) => "player_inventory",
            Some(CanonicalCell::Cursor) => "cursor",
            Some(CanonicalCell::Armor(_)) => "armor",
            Some(CanonicalCell::Offhand) => "offhand",
            Some(CanonicalCell::UiSlot(_)) => "ui_slot",
            Some(CanonicalCell::WindowSlot { .. }) => "window_slot",
            Some(
                CanonicalCell::CraftInput(_)
                | CanonicalCell::TableCraftInput(_)
                | CanonicalCell::CreatedOutput,
            )
            | None => "unrouted",
        };
        let (open_window_id, open_generation) =
            self.storage.as_ref().map_or((None, None), |storage| {
                (Some(storage.window_id), Some(storage.generation))
            });
        tracing::debug!(
            target: "cinnabar::inventory_storage_admission",
            content_window_id = ?content.container.window_id,
            content_slot_type = ?content.container.slot_type,
            content_dynamic_id = ?content.container.dynamic_id,
            content_slot_count = content.slots.len(),
            projection,
            open_window_id = ?open_window_id,
            open_generation = ?open_generation,
            "storage-sized inventory content"
        );
    }

    /// Admits one authoritative slot update through the canonical projection.
    fn apply_slot_update(
        &mut self,
        identity: SlotIdentity,
        stack: &NetworkItemStack,
        storage_item: Option<&NetworkItemStack>,
    ) {
        if self.apply_ui_slot(identity, stack) || self.apply_registry_slot(identity, stack) {
            return;
        }
        if self.apply_ordinary_slot(identity.container, identity.slot, stack) {
            return;
        }
        if identity.container.window_id == Some(protocol::OFFHAND_WINDOW_ID) {
            self.note_unrouted_container();
            return;
        }
        if identity.container.window_id == Some(protocol::DYNAMIC_STORAGE_WINDOW_ID)
            && identity.container.slot_type == Some(protocol::CONTAINER_NAME_DYNAMIC)
        {
            if let Some(id) = identity.container.dynamic_id
                && self.admit_bundle_key(id, storage_item)
            {
                self.apply_bundle_slot(id, identity.slot, stack);
            } else {
                self.note_unrouted_container();
            }
            return;
        }
        let Some(cell) =
            project_container_cell(&identity.container, identity.slot).and_then(fixed_cell)
        else {
            self.note_unrouted_container();
            return;
        };
        match cell {
            Cell::Inventory(index) => {
                if !self.confirmed.contains(Cell::Inventory(index)) {
                    self.note_unrouted_container();
                    return;
                }
                self.set_authoritative_cell(Cell::Inventory(index), Held::new(stack));
                self.known[usize::from(index)] = true;
                self.note_authoritative_write(Cell::Inventory(index));
            }
            // A single-cell surface is completely restated by one slot update.
            Cell::Cursor => {
                self.set_authoritative_cell(Cell::Cursor, Held::new(stack));
                self.cursor_resync_required = false;
                self.surface_refreshed(CellSurface::Cursor);
            }
            Cell::Offhand => {
                self.set_authoritative_cell(Cell::Offhand, Held::new(stack));
                self.offhand_resync_required = false;
                self.surface_refreshed(CellSurface::Offhand);
            }
            _ => {
                if !self.set_authoritative_cell(cell, Held::new(stack)) {
                    self.note_unrouted_container();
                }
            }
        }
    }

    /// Maps a request or response address onto a retained cell. Named open-window
    /// addresses use full window indices, bounded by the named segment.
    /// `None` marks an unrouted identity.
    pub(super) fn retained_response_cell(
        &self,
        container: &ContainerIdentity,
        slot: u16,
    ) -> Option<Cell> {
        match project_container_cell(container, slot)? {
            CanonicalCell::PlayerInventory(index) => self
                .confirmed
                .contains(Cell::Inventory(index))
                .then_some(Cell::Inventory(index)),
            CanonicalCell::Cursor => Some(Cell::Cursor),
            CanonicalCell::GenericStorage { slot, .. } => {
                self.storage.as_ref()?;
                let cell = Cell::Storage(u8::try_from(slot).ok()?);
                self.confirmed.contains(cell).then_some(cell)
            }
            CanonicalCell::WindowSlot { name, slot } => {
                let storage = self.storage.as_ref()?;
                let slot = u8::try_from(slot).ok()?;
                let (protocol::StackRequestContainer::OpenWindow { name: expected, .. }, _) =
                    protocol::open_cell_request(storage.kind, slot, None, None)?
                else {
                    return None;
                };
                let cell = Cell::Storage(slot);
                (name == expected && self.confirmed.contains(cell)).then_some(cell)
            }
            canonical => fixed_cell(canonical),
        }
    }

    /// Counts one well-formed authoritative event whose container identity
    /// did not resolve onto a retained canonical ledger cell: unknown
    /// container codes, unreviewed surfaces, or indices outside every mapped
    /// surface. Skipped whole — no mutation, session continues.
    pub(super) fn note_unrouted_container(&mut self) {
        self.skipped_unknown_containers = self.skipped_unknown_containers.saturating_add(1);
    }

    pub(super) fn apply_open(&mut self, open: protocol::ContainerOpenEvent) {
        if open.window_type == super::PERSONAL_INVENTORY_WINDOW_TYPE {
            self.apply_personal_open(open);
            return;
        }
        let Some(window_id) = open.container.window_id else {
            self.note_unrouted_container();
            return;
        };
        let Some(kind) = protocol::WindowKind::from_window_type(open.window_type)
            .filter(|_| valid_storage_window_id(window_id))
        else {
            self.note_unrouted_container();
            return;
        };
        self.settle_open_request();
        if matches!(self.personal, Some(super::PersonalWindow::Open { .. })) {
            return;
        }
        // Servers may repeat an open without sending the already-known chest contents again.
        if self.storage.as_ref().is_some_and(|storage| {
            !storage.closing
                && storage.kind == protocol::WindowKind::Storage
                && storage.window_id != protocol::DYNAMIC_STORAGE_WINDOW_ID
                && open.container.window_id == Some(storage.window_id)
                && open.window_type == storage.window_type
                && open.position == storage.position
                && open.runtime_entity_id == storage.actor_unique_id
                && storage
                    .identity
                    .is_none_or(|identity| identity.dynamic_id.is_none())
        }) {
            self.pending_open_warnings =
                (self.pending_open_warnings + 1).min(super::MAX_PENDING_REQUESTS);
            return;
        }
        self.retire_replaced_windows(None);
        self.confirmed.clear_result();
        self.controls.personal.desired_open = false;
        self.controls.personal.confirmed = None;
        self.enchant_options = None;
        let generation = self.next_open_generation;
        self.next_open_generation = self.next_open_generation.wrapping_add(1).max(1);
        self.controls.storage.confirmed = Some(ControlWindow {
            generation,
            window_id,
            window_type: open.window_type,
        });
        self.controls.storage.desired_open = true;
        self.storage = Some(StorageWindow {
            trade: None,
            window_id,
            window_type: open.window_type,
            kind,
            position: open.position,
            actor_unique_id: open.runtime_entity_id,
            data: std::collections::BTreeMap::new(),
            generation,
            identity: None,
            block_container: None,
            superseded_identity: None,
            resync_required: false,
            closing: false,
        });
        self.confirmed.clear_storage();
        // A named window shows its fixed cells before any content arrives.
        if let Some(protocol::OpenCells::Named { lengths, .. }) = kind.open_cells() {
            self.confirmed
                .ensure_storage(lengths.iter().copied().min().unwrap_or(0));
        }
    }
}

/// Maps a fixed armor, offhand or crafting canonical cell onto its ledger cell.
fn fixed_cell(canonical: CanonicalCell) -> Option<Cell> {
    Some(match canonical {
        CanonicalCell::PlayerInventory(slot) => Cell::Inventory(slot),
        CanonicalCell::Cursor => Cell::Cursor,
        CanonicalCell::Armor(slot) => Cell::Armor(slot),
        CanonicalCell::Offhand => Cell::Offhand,
        CanonicalCell::CraftInput(index) => Cell::Craft(FIRST_CRAFT_SLOT + index),
        CanonicalCell::TableCraftInput(index) => Cell::Craft(FIRST_CRAFT_SLOT + 4 + index),
        CanonicalCell::CreatedOutput => Cell::CreatedOutput,
        CanonicalCell::UiSlot(slot) => Cell::Craft(slot),
        CanonicalCell::GenericStorage { .. } | CanonicalCell::WindowSlot { .. } => return None,
    })
}

/// The ledger cell for one UI inventory slot a screen uses.
pub(super) fn ui_cell(slot: u8) -> Option<Cell> {
    protocol::ui_slot_container_name(slot)?;
    Some(if slot == protocol::CREATED_OUTPUT_SLOT {
        Cell::CreatedOutput
    } else {
        Cell::Craft(slot)
    })
}
