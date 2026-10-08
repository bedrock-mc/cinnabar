use protocol::{InventoryEvent, ItemRegistryEvent};

use crate::{InventorySession, MAX_PENDING_INVENTORY_EVENTS};

/// A rejected event never enters the retained inventory FIFO.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InventoryIngressError {
    #[error("wrong inventory session: expected {expected}, received {actual}")]
    WrongSession { expected: u64, actual: u64 },
    #[error("stale inventory sequence: previous {previous}, received {actual}")]
    StaleFifoSequence { previous: u64, actual: u64 },
    #[error("inventory event queue is full ({maximum})")]
    InventoryQueueFull { maximum: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequencedInventoryEvent {
    pub session_generation: u64,
    pub fifo_sequence: u64,
    pub event: InventoryAuthorityEvent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryAuthorityEvent {
    Inventory(InventoryEvent),
    Registry(ItemRegistryEvent),
}

impl InventorySession {
    /// Queue one event without changing its FIFO position.
    pub fn enqueue_inventory_event(
        &mut self,
        session_generation: u64,
        fifo_sequence: u64,
        event: InventoryEvent,
    ) -> Result<(), InventoryIngressError> {
        self.enqueue_inventory_authority_event(
            session_generation,
            fifo_sequence,
            InventoryAuthorityEvent::Inventory(event),
        )
    }

    /// Queue one registry replacement in the inventory FIFO.
    pub fn enqueue_item_registry_event(
        &mut self,
        session_generation: u64,
        fifo_sequence: u64,
        event: ItemRegistryEvent,
    ) -> Result<(), InventoryIngressError> {
        self.enqueue_inventory_authority_event(
            session_generation,
            fifo_sequence,
            InventoryAuthorityEvent::Registry(event),
        )
    }

    /// Validate session, order and capacity before admitting an event.
    fn enqueue_inventory_authority_event(
        &mut self,
        session_generation: u64,
        fifo_sequence: u64,
        event: InventoryAuthorityEvent,
    ) -> Result<(), InventoryIngressError> {
        if session_generation != self.session_id {
            return Err(InventoryIngressError::WrongSession {
                expected: self.session_id,
                actual: session_generation,
            });
        }
        if let Some(previous) = self.last_inventory_sequence
            && fifo_sequence <= previous
        {
            return Err(InventoryIngressError::StaleFifoSequence {
                previous,
                actual: fifo_sequence,
            });
        }
        if self.pending_inventory.len() >= MAX_PENDING_INVENTORY_EVENTS {
            return Err(InventoryIngressError::InventoryQueueFull {
                maximum: MAX_PENDING_INVENTORY_EVENTS,
            });
        }
        self.crafting_authority.note_ingress(fifo_sequence, &event);
        self.pending_inventory.push_back(SequencedInventoryEvent {
            session_generation,
            fifo_sequence,
            event,
        });
        self.last_inventory_sequence = Some(fifo_sequence);
        Ok(())
    }

    /// Remove an event without applying it, retiring the crafting authority it bypasses.
    pub fn pop_inventory_event(&mut self) -> Option<SequencedInventoryEvent> {
        let event = self.pending_inventory.pop_front()?;
        self.crafting_authority.bypass(
            self.last_inventory_sequence
                .unwrap_or(event.fifo_sequence)
                .max(event.fifo_sequence),
        );
        Some(event)
    }

    /// Bind crafting observations to the committed world frontier.
    pub fn synchronize_crafting_frontier(
        &mut self,
        session: u64,
        identity: Option<(u64, u64, Option<u64>)>,
    ) {
        self.crafting_authority
            .synchronize(if session == self.session_id {
                identity
            } else {
                None
            });
    }

    /// Seed the passive crafting projection with startup registry and negotiation.
    pub fn publish_crafting_bootstrap(
        &mut self,
        registry: Option<&ItemRegistryEvent>,
        authority: protocol::InventoryAuthority,
    ) {
        self.crafting_authority.bootstrap(registry, authority);
    }

    /// Apply exactly one FIFO event before the caller updates its UI and evidence projections.
    pub fn apply_next(&mut self) -> Option<SequencedInventoryEvent> {
        let sequenced = self.pending_inventory.pop_front()?;
        self.crafting_authority.observe(
            sequenced.session_generation,
            sequenced.fifo_sequence,
            &sequenced.event,
        );
        match &sequenced.event {
            InventoryAuthorityEvent::Registry(registry) => {
                self.inventory_ledger.apply_registry(registry)
            }
            InventoryAuthorityEvent::Inventory(event) => {
                self.inventory_ledger.apply(event);
                if let InventoryEvent::SelectedSlot(selected) = event
                    && selected.select_slot
                    && selected.slot < protocol::HOTBAR_SLOT_COUNT
                {
                    self.server_selected_slot = Some(selected.slot);
                    self.local_selected_slot = None;
                    self.pending_hotbar_selection = None;
                }
            }
        }
        Some(sequenced)
    }

    /// Advance crafting once after all FIFO events and their presentation effects have drained.
    pub fn finish_drain(&mut self) {
        self.crafting_authority.advance();
    }

    /// Apply a complete FIFO without application presentation effects.
    pub fn drain_pending_inventory(&mut self) {
        while self.apply_next().is_some() {}
        self.finish_drain();
    }

    /// Borrowed display values only, with immutable credit owners retained by this runtime.
    /// Inactive crafting authority never allocates a request or sends a packet.
    pub fn crafting_preview(&self) -> Option<super::CraftingPreview<'_>> {
        self.crafting_authority.preview()
    }

    /// The recipe the presented crafting grid forms against the committed
    /// catalog; `Unavailable` without a catalog or known grid identities.
    #[must_use]
    pub fn crafting_match(&self) -> crate::CraftGridMatch {
        let ledger = self.ledger();
        let (Some(catalog), Some(cells)) = (
            self.crafting_authority.catalog(),
            ledger.crafting_grid_cells(),
        ) else {
            return crate::CraftGridMatch::Unavailable;
        };
        if cells.iter().all(Option::is_none) {
            return crate::CraftGridMatch::NoMatch;
        }
        let items: Vec<_> = cells
            .iter()
            .map(|cell| {
                cell.as_ref()
                    .map(super::inventory_ledger::CraftGridCell::item)
            })
            .collect();
        crate::match_crafting_grid(catalog, ledger.crafting_grid().width(), &items)
    }

    /// Crafts the grid's unique recipe once into the cursor.
    pub fn begin_crafting(
        &mut self,
    ) -> Result<i32, super::inventory_ledger::InventoryGestureError> {
        self.begin_crafting_into(super::inventory_ledger::CraftSink::Cursor)
    }

    /// Crafts the grid's unique recipe once into `sink`.
    pub fn begin_crafting_into(
        &mut self,
        sink: super::inventory_ledger::CraftSink,
    ) -> Result<i32, super::inventory_ledger::InventoryGestureError> {
        let matched = self.crafting_match();
        tracing::debug!(target: "bedrock_client::inventory_requests",
            catalog_available = self.crafting_authority.catalog().is_some(),
            recipes = self.crafting_authority.catalog().map(|catalog| catalog.crafting_handles().len()),
            grid = ?self.ledger().crafting_grid_cells(),
            result = match &matched {
                crate::CraftGridMatch::Unavailable => "unavailable",
                crate::CraftGridMatch::NoMatch => "no_match",
                crate::CraftGridMatch::Ambiguous => "ambiguous",
                crate::CraftGridMatch::Unique(_) => "unique",
            }, "manual crafting requested");
        let crate::CraftGridMatch::Unique(recipe) = matched else {
            return Err(super::inventory_ledger::InventoryGestureError::InvalidRequest);
        };
        self.ledger_mut().begin_craft_into(&recipe, 1, sink)
    }

    /// Crafts the grid's unique recipe as many times as it fits, shift-click style.
    pub fn begin_crafting_all(
        &mut self,
    ) -> Result<i32, super::inventory_ledger::InventoryGestureError> {
        let crate::CraftGridMatch::Unique(recipe) = self.crafting_match() else {
            return Err(super::inventory_ledger::InventoryGestureError::InvalidRequest);
        };
        self.ledger_mut().begin_craft_all(&recipe)
    }
}
