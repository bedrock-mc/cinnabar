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
    CanonicalCell, ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack,
    SlotIdentity, project_container_cell,
};

use super::cells::{ARMOR_CELLS, CellSurface, FIRST_CRAFT_SLOT, Held};
use super::helpers::{bare_storage_window_matches, valid_raw_window_id, valid_storage_window_id};
use super::{
    Cell, LARGE_STORAGE_SLOT_COUNT, PLAYER_INVENTORY_SLOT_COUNT, PendingCloseOwner,
    PlayerInventoryLedger, SMALL_STORAGE_SLOT_COUNT, StorageWindow,
};

impl PlayerInventoryLedger {
    pub fn apply(&mut self, event: &InventoryEvent) {
        for update in event.slot_updates() {
            tracing::debug!(target: "bedrock_client::inventory_requests",
                identity = ?update.identity, network_id = update.stack.network_id,
                stack_id = update.stack.stack_network_id, count = update.stack.count,
                "authoritative inventory slot");
        }
        if let InventoryEvent::Content(content) = event {
            tracing::debug!(target: "bedrock_client::inventory_requests",
                container = ?content.container, slots = content.slots.len(),
                items = ?content.slots.iter().take(PLAYER_INVENTORY_SLOT_COUNT)
                    .map(|stack| (stack.network_id, stack.stack_network_id, stack.count))
                    .collect::<Vec<_>>(),
                "authoritative inventory content");
        }
        self.admit(event);
        self.refold();
        self.reconcile_crafting_close();
    }

    fn admit(&mut self, event: &InventoryEvent) {
        match event {
            // Recipe execution is not activated by protocol admission alone.
            InventoryEvent::Recipes(_) => {}
            InventoryEvent::Authority(authority) => {
                if self
                    .authority
                    .replace(*authority)
                    .is_some_and(|previous| previous != *authority)
                {
                    self.queue.clear();
                    self.personal = None;
                    self.confirmed.set(Cell::Cursor, None);
                    self.player_resync_required = false;
                    self.cursor_resync_required = false;
                    self.storage = None;
                    self.pending_closes.clear();
                }
            }
            InventoryEvent::Open(open) => self.apply_open(*open),
            InventoryEvent::Close(close) => {
                let personal_close = self.personal.as_ref().and_then(|personal| match personal {
                    super::PersonalWindow::Open {
                        window_id,
                        window_type,
                        ..
                    } if close.server_initiated
                        && close.container.window_id == Some(*window_id)
                        && close.window_type == *window_type =>
                    {
                        Some(false)
                    }
                    super::PersonalWindow::Closing {
                        window_id,
                        window_type,
                        deadline_millis,
                        ..
                    } => {
                        // Vanilla routes a response to the screen manager
                        // without inspecting its container id or type. Those
                        // payload fields are not acknowledgement correlation.
                        if !close.server_initiated && deadline_millis.is_some() {
                            Some(true)
                        } else if close.server_initiated
                            && close.container.window_id == Some(*window_id)
                            && close.window_type == *window_type
                        {
                            Some(false)
                        } else {
                            None
                        }
                    }
                    _ => None,
                });
                // A stale response cannot consume the close before it has
                // reached the transport, even when its payload happens to match.
                let unsent_personal_close = matches!(
                    self.personal,
                    Some(super::PersonalWindow::Closing {
                        deadline_millis: None,
                        ..
                    })
                );
                if (close.server_initiated || !unsent_personal_close)
                    && let Some(window_id) = close.container.window_id
                {
                    self.remove_pending_close(window_id, close.window_type);
                }
                if let Some(retain_confirmed_cursor) = personal_close {
                    self.finish_personal_close(retain_confirmed_cursor);
                }
                if self.storage.as_ref().is_some_and(|storage| {
                    close.container.window_id == Some(storage.window_id)
                        && close.window_type == storage.window_type
                }) {
                    self.close_storage();
                }
            }
            InventoryEvent::Content(content) => self.apply_content(content),
            InventoryEvent::Slot(update) => {
                self.apply_slot_update(update.identity, &update.stack);
            }
            InventoryEvent::Transaction(transaction) => {
                self.skipped_unknown_containers = self
                    .skipped_unknown_containers
                    .saturating_add(transaction.skipped_actions as u64);
                if transaction.skipped_actions != 0 {
                    tracing::warn!(target: "bedrock_client::inventory_requests",
                        count = transaction.skipped_actions,
                        "skipped unsupported normal transaction actions");
                }
                for update in event.slot_updates() {
                    self.apply_slot_update(update.identity, &update.stack);
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
        if self.apply_legacy_named_content(content.container, &content.slots) {
            return;
        }
        let resolved;
        let identity = self.storage_wire_identity(content.container);
        let content = if identity != content.container {
            resolved = InventoryContentEvent {
                container: identity,
                ..content.clone()
            };
            &resolved
        } else {
            content
        };
        if content.container.slot_type == Some(protocol::CONTAINER_NAME_DYNAMIC)
            && let Some(dynamic_id) = content.container.dynamic_id
        {
            self.apply_bundle_content(dynamic_id, &content.slots);
            return;
        }
        self.trace_storage_sized_content(content);
        match project_container_cell(&content.container, 0) {
            Some(CanonicalCell::GenericStorage { .. }) => {
                self.apply_storage_content(content.container, &content.slots);
            }
            Some(CanonicalCell::PlayerInventory(_)) => {
                let complete = content.slots.len() == PLAYER_INVENTORY_SLOT_COUNT;
                for (index, stack) in content
                    .slots
                    .iter()
                    .take(PLAYER_INVENTORY_SLOT_COUNT)
                    .enumerate()
                {
                    self.set_authoritative_cell(Cell::Inventory(index as u8), Held::new(stack));
                    self.known[index] = true;
                    self.note_authoritative_write(Cell::Inventory(index as u8));
                }
                if complete {
                    self.player_resync_required = false;
                    self.surface_refreshed(CellSurface::Player);
                }
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
                if let [stack] = content.slots.as_ref() {
                    self.set_authoritative_cell(Cell::Offhand, Held::new(stack));
                    self.offhand_resync_required = false;
                    self.surface_refreshed(CellSurface::Offhand);
                } else {
                    self.note_unrouted_container();
                }
            }
            Some(CanonicalCell::WindowSlot { name, .. }) => {
                self.apply_window_content(content.container, name, &content.slots);
            }
            Some(CanonicalCell::UiSlot(slot)) => match content.slots.as_ref() {
                [stack] => {
                    self.set_authoritative_cell(Cell::Craft(slot), Held::new(stack));
                }
                _ => self.note_unrouted_container(),
            },
            None if protocol::is_personal_ui_inventory(&content.container)
                && content.slots.len() == UI_INVENTORY_SLOT_COUNT =>
            {
                for slot in 0..protocol::UI_SLOT_COUNT as u8 {
                    if let Some(cell) = ui_cell(slot) {
                        let stack = &content.slots[usize::from(slot)];
                        self.set_authoritative_cell(cell, Held::new(stack));
                    }
                }
                self.crafting_resync_required = false;
                self.surface_refreshed(CellSurface::Crafting);
            }
            None if self.bare_named_window_matches(&content.container) => {
                self.apply_window_content(content.container, 0, &content.slots);
            }
            Some(
                CanonicalCell::CraftInput(_)
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
    /// `RUST_LOG=bedrock_client::inventory_storage_admission=debug`.
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
            target: "bedrock_client::inventory_storage_admission",
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
    fn apply_slot_update(&mut self, identity: SlotIdentity, stack: &NetworkItemStack) {
        if self.apply_legacy_named_slot(identity.container, identity.slot, stack) {
            return;
        }
        let identity = SlotIdentity {
            container: if identity.container.slot_type.is_some() {
                self.storage_wire_identity(identity.container)
            } else {
                identity.container
            },
            ..identity
        };
        if identity.container.slot_type == Some(protocol::CONTAINER_NAME_DYNAMIC)
            && let Some(dynamic_id) = identity.container.dynamic_id
        {
            self.apply_bundle_slot(dynamic_id, identity.slot, stack);
            return;
        }
        match project_container_cell(&identity.container, identity.slot) {
            Some(CanonicalCell::PlayerInventory(index)) => {
                self.set_authoritative_cell(Cell::Inventory(index), Held::new(stack));
                self.known[usize::from(index)] = true;
                self.note_authoritative_write(Cell::Inventory(index));
            }
            // A single-cell surface is completely restated by one slot update.
            Some(CanonicalCell::Cursor) => {
                self.set_authoritative_cell(Cell::Cursor, Held::new(stack));
                self.cursor_resync_required = false;
                self.surface_refreshed(CellSurface::Cursor);
            }
            Some(CanonicalCell::Offhand) => {
                self.set_authoritative_cell(Cell::Offhand, Held::new(stack));
                self.offhand_resync_required = false;
                self.surface_refreshed(CellSurface::Offhand);
            }
            Some(CanonicalCell::GenericStorage { slot, .. })
                if self.storage.as_ref().is_some_and(|storage| {
                    matches!(
                        storage.kind.open_cells(),
                        Some(protocol::OpenCells::Named { .. })
                    )
                }) =>
            {
                self.apply_window_slot(
                    identity.container,
                    protocol::CONTAINER_NAME_LEVEL_ENTITY,
                    slot,
                    stack,
                );
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
            None if protocol::is_personal_ui_inventory(&identity.container)
                && u8::try_from(identity.slot).ok().and_then(ui_cell).is_some() =>
            {
                let cell = ui_cell(identity.slot as u8).expect("checked by the guard");
                self.set_authoritative_cell(cell, Held::new(stack));
            }
            Some(CanonicalCell::UiSlot(slot)) => {
                self.set_authoritative_cell(Cell::Craft(slot), Held::new(stack));
            }
            Some(CanonicalCell::WindowSlot { name, slot }) => {
                self.apply_window_slot(identity.container, name, slot, stack);
            }
            Some(canonical) => match fixed_cell(canonical) {
                Some(cell) => {
                    self.set_authoritative_cell(cell, Held::new(stack));
                }
                None => self.note_unrouted_container(),
            },
            None => self.note_unrouted_container(),
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
                self.storage.as_ref()?;
                let cell = Cell::Storage(u8::try_from(slot).ok()?);
                self.confirmed.contains(cell).then_some(cell)
            }
            CanonicalCell::WindowSlot { name, slot } => {
                let storage = self.storage.as_ref()?;
                let first = protocol::open_name_first_cell(storage.kind, name)?;
                let cell = Cell::Storage(first.checked_add(u8::try_from(slot).ok()?)?);
                self.confirmed.contains(cell).then_some(cell)
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

    fn apply_open(&mut self, open: protocol::ContainerOpenEvent) {
        if open.window_type == super::PERSONAL_INVENTORY_WINDOW_TYPE {
            self.apply_personal_open(open);
            return;
        }
        if self.personal.is_some() {
            if let Some(window_id) = open.container.window_id {
                self.queue_close(window_id, open.window_type, PendingCloseOwner::Cleanup);
            }
            return;
        }
        self.abandon_requests(|pending| pending.storage_generation.is_some());
        let Some(window_id) = open.container.window_id else {
            return;
        };
        let Some(kind) = protocol::WindowKind::from_window_type(open.window_type)
            .filter(|_| valid_storage_window_id(window_id))
        else {
            self.queue_close(window_id, open.window_type, PendingCloseOwner::Cleanup);
            self.storage = None;
            self.confirmed.clear_storage();
            return;
        };
        self.enchant_options = None;
        self.remove_pending_close(window_id, open.window_type);
        let generation = self.next_open_generation;
        self.next_open_generation = self.next_open_generation.wrapping_add(1).max(1);
        self.storage = Some(StorageWindow {
            window_id,
            window_type: open.window_type,
            kind,
            position: open.position,
            actor_unique_id: open.runtime_entity_id,
            data: std::collections::BTreeMap::new(),
            generation,
            identity: None,
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

    fn apply_personal_open(&mut self, open: protocol::ContainerOpenEvent) {
        let Some(window_id) = open.container.window_id else {
            self.note_unrouted_container();
            return;
        };
        let Some(super::PersonalWindow::Opening {
            generation,
            admitted: true,
            desired_open,
            ..
        }) = self.personal
        else {
            self.queue_close(window_id, open.window_type, PendingCloseOwner::Cleanup);
            self.note_unrouted_container();
            return;
        };
        if !valid_raw_window_id(window_id) {
            self.queue_close(window_id, open.window_type, PendingCloseOwner::Cleanup);
            self.personal = None;
            self.personal_lifecycle_failed = true;
            self.note_unrouted_container();
            return;
        }
        if desired_open {
            self.personal = Some(super::PersonalWindow::Open {
                generation,
                window_id,
                window_type: open.window_type,
            });
        } else {
            let returning = self.close_return_needed();
            self.queue_close(
                window_id,
                open.window_type,
                PendingCloseOwner::Personal(generation),
            );
            self.personal = Some(super::PersonalWindow::Closing {
                generation,
                window_id,
                window_type: open.window_type,
                // The acknowledgement completed the Open wait. Start the
                // distinct Close wait only after its packet is admitted.
                deadline_millis: None,
            });
            if returning {
                self.retain_close_returns(PendingCloseOwner::Personal(generation));
            }
        }
    }

    fn apply_storage_content(&mut self, identity: ContainerIdentity, slots: &[NetworkItemStack]) {
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        let window_id = storage.window_id;
        let window_type = storage.window_type;
        // Screens that keep their cells in the UI inventory never land here.
        let lengths = match storage.kind.open_cells() {
            Some(protocol::OpenCells::Generic(lengths)) => lengths,
            // A horse's chest rides the level-entity name after its equipment.
            Some(protocol::OpenCells::Named { .. }) => {
                self.apply_window_content(identity, protocol::CONTAINER_NAME_LEVEL_ENTITY, slots);
                return;
            }
            None => return,
        };
        if identity.window_id != Some(window_id) {
            return;
        }
        let valid_len = lengths.contains(&slots.len());
        if storage.identity.is_some_and(|current| current != identity) {
            return;
        }
        if !valid_len {
            self.queue_close(window_id, window_type, PendingCloseOwner::Storage);
            self.close_storage();
            return;
        }
        for slot in 0..self.confirmed.storage_len().max(slots.len()) {
            self.retire_legacy_write(Cell::Storage(slot as u8));
        }
        let storage = self.storage.as_mut().expect("storage remains active");
        storage.identity = Some(identity);
        storage.resync_required = false;
        self.confirmed.replace_storage(slots);
        self.surface_refreshed(CellSurface::Storage);
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
        if !super::helpers::storage_slot_identity_matches(storage, identity) {
            return;
        }
        if let Ok(slot) = u8::try_from(slot) {
            self.set_authoritative_cell(Cell::Storage(slot), Held::new(stack));
        }
    }
}

/// The personal UI inventory's full content length.
const UI_INVENTORY_SLOT_COUNT: usize = 54;

/// Maps a fixed armor, offhand or crafting canonical cell onto its ledger cell.
fn fixed_cell(canonical: CanonicalCell) -> Option<Cell> {
    Some(match canonical {
        CanonicalCell::Armor(slot) => Cell::Armor(slot),
        CanonicalCell::Offhand => Cell::Offhand,
        CanonicalCell::CraftInput(index) => Cell::Craft(FIRST_CRAFT_SLOT + index),
        CanonicalCell::TableCraftInput(index) => Cell::Craft(FIRST_CRAFT_SLOT + 4 + index),
        CanonicalCell::CreatedOutput => Cell::CreatedOutput,
        CanonicalCell::UiSlot(slot) => Cell::Craft(slot),
        CanonicalCell::PlayerInventory(_)
        | CanonicalCell::Cursor
        | CanonicalCell::GenericStorage { .. }
        | CanonicalCell::WindowSlot { .. } => return None,
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
