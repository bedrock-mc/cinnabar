//! Inactive, committed crafting projection. This owns no request, journal or UI.
mod budget;
mod observation;
mod projection;

use projection::{Observation, Queue, Record, RegistryOwner, StackOwner, transaction_cell_index};
use protocol::{InventoryAuthority, InventoryEvent, RecipeCatalog, VerifiedNetworkItemStack};
use std::{num::NonZeroU64, sync::Arc};

use crate::{InventoryAuthorityEvent, ManualCraftCell, ManualCraftMatch};

/// Borrowed display values. Copies retain the borrow, never a detached owning Arc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CraftingPreview<'a> {
    Unavailable,
    NoMatch,
    Ambiguous,
    Unique {
        identifier: &'a str,
        count: u16,
        metadata: u32,
        block_runtime_id: u32,
    },
}

#[derive(Debug)]
struct PreviewOwner {
    value: ManualCraftMatch,
    _registry: Arc<RegistryOwner>,
    _catalog: RecipeCatalog,
    _permit: budget::Permit,
}

#[derive(Debug, Clone)]
pub(super) struct CraftingAuthority {
    credits: Arc<budget::Credits>,
    session: u64,
    stream: Option<u64>,
    through: Option<u64>,
    epoch: u64,
    barrier: u64,
    observed: u64,
    consumed: u64,
    authority_loss: u64,
    generation: u64,
    exhausted: bool,
    authority: Option<InventoryAuthority>,
    registry: Option<Arc<RegistryOwner>>,
    catalog: RecipeCatalog,
    catalog_lost: bool,
    grid: [Option<Arc<StackOwner>>; 4],
    cursor: Option<Arc<StackOwner>>,
    queue: Option<Arc<Queue>>,
    revision: u64,
    cache_key: Option<(u64, u64, u64, u64)>,
    preview: Option<Arc<PreviewOwner>>,
}

impl CraftingAuthority {
    pub(super) fn new(session: u64) -> Self {
        Self::with_credits(session, budget::Credits::shared())
    }

    fn with_credits(session: u64, credits: Arc<budget::Credits>) -> Self {
        let mut catalog = RecipeCatalog::default();
        catalog.begin_session(session);
        Self {
            credits,
            session,
            stream: None,
            through: None,
            epoch: 0,
            barrier: 0,
            observed: 0,
            consumed: 0,
            authority_loss: 0,
            generation: 0,
            exhausted: false,
            authority: None,
            registry: None,
            catalog,
            catalog_lost: true,
            grid: std::array::from_fn(|_| None),
            cursor: None,
            queue: None,
            revision: 0,
            cache_key: None,
            preview: None,
        }
    }

    fn clear_cells(&mut self) {
        observation::clear_cells();
        self.grid = std::array::from_fn(|_| None);
        self.cursor = None;
        self.changed_cells();
    }

    // One checked revision covers both grid and cursor display availability.
    fn changed_cells(&mut self) {
        self.preview = None;
        self.cache_key = None;
        match self.revision.checked_add(1) {
            Some(next) => self.revision = next,
            None => self.exhausted = true,
        }
    }

    fn lose(&mut self, incoming: u64, domain: u8) {
        observation::loss();
        let mut domains = domain;
        let mut barrier = self
            .barrier
            .max(incoming)
            .max(self.observed)
            .max(self.consumed)
            .max(self.through.unwrap_or(0));
        if let Some(queue) = self.queue.take() {
            for record in &queue.records {
                barrier = barrier.max(record.sequence);
                domains |= record.observation.domain();
            }
        }
        self.barrier = barrier;
        self.clear_cells();
        if domains & 1 != 0 {
            self.registry = None;
        }
        if domains & 2 != 0 {
            self.catalog.begin_session(self.session);
            self.catalog_lost = true;
        }
        if domains & 4 != 0 {
            self.authority = None;
            self.authority_loss = self.authority_loss.max(barrier);
        }
    }

    pub(super) fn bypass(&mut self, sequence: u64) {
        self.lose(sequence, 7);
    }

    pub(super) fn note_ingress(&mut self, sequence: u64, event: &InventoryAuthorityEvent) {
        self.observed = self.observed.max(sequence);
        if matches!(
            event,
            InventoryAuthorityEvent::Inventory(InventoryEvent::Authority(
                InventoryAuthority::Client
            ))
        ) {
            self.authority_loss = self.authority_loss.max(sequence);
            self.authority = None;
            self.clear_cells();
        }
    }

    /// The committed recipe catalog while it is available.
    pub(super) fn catalog(&self) -> Option<&RecipeCatalog> {
        (self.catalog.is_available() && !self.catalog_lost).then_some(&self.catalog)
    }

    pub(super) fn preview(&self) -> Option<CraftingPreview<'_>> {
        self.preview.as_ref().map(|owner| match &owner.value {
            ManualCraftMatch::Unavailable => CraftingPreview::Unavailable,
            ManualCraftMatch::NoMatch => CraftingPreview::NoMatch,
            ManualCraftMatch::Ambiguous => CraftingPreview::Ambiguous,
            ManualCraftMatch::Unique(value) => CraftingPreview::Unique {
                identifier: &value.identifier,
                count: value.count,
                metadata: value.metadata,
                block_runtime_id: value.block_runtime_id,
            },
        })
    }

    pub(super) fn synchronize(&mut self, identity: Option<(u64, u64, Option<u64>)>) {
        observation::synchronize(self.session, identity);
        let Some((stream, epoch, Some(through))) = identity else {
            self.through = None;
            self.lose(self.observed, 7);
            return;
        };
        if stream == 0 || epoch > through || self.stream.is_some_and(|old| old != stream) {
            self.lose(through, 7);
        }
        if stream == 0
            || epoch > through
            || (self.stream == Some(stream) && (epoch < self.epoch || through < self.consumed))
        {
            self.through = None;
            self.lose(through, 7);
            return;
        }
        self.stream = Some(stream);
        self.through = Some(through);
        if epoch != self.epoch {
            self.epoch = epoch;
            self.clear_cells();
        }
    }

    fn registry_owner(
        &mut self,
        event: &protocol::ItemRegistryEvent,
    ) -> Option<Arc<RegistryOwner>> {
        let Some(generation) = self.generation.checked_add(1) else {
            self.exhausted = true;
            return None;
        };
        self.generation = generation;
        RegistryOwner::with_credits(event, NonZeroU64::new(generation)?, &self.credits)
    }

    fn replace_registry(&mut self, next: Option<Arc<RegistryOwner>>) {
        // A retained numeric cell is bound at its FIFO position. A replacement
        // must not reinterpret it as a differently named item, even at the same
        // numeric ID. Empty cells have no registry binding to invalidate.
        let previous = self.registry.as_ref();
        let keeps_binding = |stack: &StackOwner| {
            stack.stack.is_empty()
                || previous
                    .and_then(|old| old.snapshot.get(stack.stack.network_id))
                    .zip(
                        next.as_ref()
                            .and_then(|new| new.snapshot.get(stack.stack.network_id)),
                    )
                    .is_some_and(|(old, new)| old.identifier == new.identifier)
        };
        let mut retired = false;
        for cell in self
            .grid
            .iter_mut()
            .chain(std::iter::once(&mut self.cursor))
        {
            if cell.as_ref().is_some_and(|stack| !keeps_binding(stack)) {
                *cell = None;
                retired = true;
            }
        }
        self.registry = next;
        self.cache_key = None;
        if retired {
            self.changed_cells();
            observation::forget_absent_cells(self);
        }
    }

    /// Only the admitted session bootstrap may call this; it grants no cell facts.
    pub(super) fn bootstrap(
        &mut self,
        registry: Option<&protocol::ItemRegistryEvent>,
        authority: InventoryAuthority,
    ) {
        if self.observed != 0
            || self.consumed != 0
            || self.barrier != 0
            || self.authority_loss != 0
            || self.epoch != 0
        {
            return;
        }
        self.registry = registry.and_then(|event| self.registry_owner(event));
        self.authority = Some(authority);
        observation::bootstrap(self.session, self.registry.is_some());
    }

    pub(super) fn observe(&mut self, session: u64, sequence: u64, event: &InventoryAuthorityEvent) {
        if session != self.session || self.exhausted {
            return;
        }
        self.note_ingress(sequence, event);
        if sequence <= self.barrier {
            return;
        }
        let observation = match event {
            InventoryAuthorityEvent::Registry(registry) => {
                let Some(owner) = self.registry_owner(registry) else {
                    self.lose(sequence, 1);
                    return;
                };
                Observation::Registry(Some(owner))
            }
            InventoryAuthorityEvent::Inventory(InventoryEvent::Authority(authority)) => {
                Observation::Authority(*authority)
            }
            InventoryAuthorityEvent::Inventory(InventoryEvent::Recipes(update)) => {
                Observation::Recipes(update.clone())
            }
            InventoryAuthorityEvent::Inventory(InventoryEvent::Slot(update)) => {
                let index = match protocol::project_container_cell(
                    &update.identity.container,
                    update.identity.slot,
                ) {
                    Some(protocol::CanonicalCell::CraftInput(index)) => Some(usize::from(index)),
                    Some(protocol::CanonicalCell::Cursor) => None,
                    _ => match protocol::personal_craft_slot_index(
                        &update.identity.container,
                        update.identity.slot,
                    ) {
                        Some(index) => Some(usize::from(index)),
                        None => return,
                    },
                };
                let Some(stack) = StackOwner::with_credits(&update.stack, &self.credits) else {
                    self.lose(sequence, 0);
                    return;
                };
                if let Some(index) = index {
                    Observation::Cell { index, stack }
                } else {
                    Observation::Cursor(stack)
                }
            }
            InventoryAuthorityEvent::Inventory(event @ InventoryEvent::Transaction(_)) => {
                let mut cells = std::array::from_fn(|_| None);
                for update in event.slot_updates() {
                    let Some(index) = transaction_cell_index(update.identity) else {
                        continue;
                    };
                    let Some(stack) = StackOwner::with_credits(&update.stack, &self.credits) else {
                        self.lose(sequence, 0);
                        return;
                    };
                    cells[index] = Some(stack);
                }
                if cells.iter().all(Option::is_none) {
                    return;
                }
                Observation::Cells(cells)
            }
            InventoryAuthorityEvent::Inventory(InventoryEvent::Content(content))
                if protocol::personal_craft_content_indices(
                    &content.container,
                    content.slots.len(),
                )
                .is_some() =>
            {
                let indices = protocol::personal_craft_content_indices(
                    &content.container,
                    content.slots.len(),
                )
                .expect("guarded content shape");
                let Some(grid) = StackOwner::grid_with_credits(
                    indices.map(|index| &content.slots[index]),
                    &self.credits,
                ) else {
                    self.lose(sequence, 0);
                    return;
                };
                Observation::Grid(grid)
            }
            InventoryAuthorityEvent::Inventory(InventoryEvent::Content(content))
                if content.slots.len() == 1
                    && content.container.slot_type == Some(protocol::CONTAINER_NAME_CURSOR) =>
            {
                let Some(stack) = StackOwner::with_credits(&content.slots[0], &self.credits) else {
                    self.lose(sequence, 0);
                    return;
                };
                Observation::Cursor(stack)
            }
            _ => return,
        };
        let domain = observation.domain();
        let old = self
            .queue
            .as_ref()
            .map_or(&[][..], |queue| queue.records.as_slice());
        let Some(count) = old.len().checked_add(1) else {
            self.lose(sequence, domain);
            return;
        };
        let replacement = Queue::replace(
            old.iter().cloned().chain(std::iter::once(Record {
                sequence,
                observation,
            })),
            count,
            &self.credits,
        );
        match replacement {
            Some(queue) => {
                self.queue = Some(queue);
                observation::stage(session, sequence, event);
            }
            None => self.lose(sequence, domain),
        }
    }

    pub(super) fn advance(&mut self) {
        let Some(through) = self.through else {
            return;
        };
        let Some(queue) = self.queue.take() else {
            self.match_if_changed();
            return;
        };
        let split = queue
            .records
            .partition_point(|record| record.sequence <= through);
        if split == 0 {
            self.queue = Some(queue);
            self.match_if_changed();
            return;
        }
        for record in &queue.records[..split] {
            self.consumed = self.consumed.max(record.sequence);
            if record.sequence <= self.barrier {
                observation::discard(record.sequence);
                continue;
            }
            match &record.observation {
                Observation::Registry(registry) => {
                    self.replace_registry(registry.clone());
                    observation::registry(record.sequence);
                }
                Observation::Recipes(update) => {
                    if !self.catalog_lost || update.clears_catalog() {
                        let applied = self.catalog.apply(self.session, record.sequence, update);
                        self.catalog_lost = !self.catalog.is_available();
                        if applied {
                            observation::recipe(record.sequence, self.catalog.is_available());
                        }
                    }
                }
                Observation::Authority(authority) if record.sequence > self.authority_loss => {
                    self.authority = Some(*authority)
                }
                Observation::Authority(_) => {}
                Observation::Cell { index, stack }
                    if record.sequence > self.epoch.max(self.authority_loss) =>
                {
                    self.grid[*index] = Some(Arc::clone(stack));
                    self.changed_cells();
                    observation::cells(self.session, record.sequence, self.epoch, 1 << *index);
                }
                Observation::Grid(grid)
                    if record.sequence > self.epoch.max(self.authority_loss) =>
                {
                    self.grid = grid.each_ref().map(|stack| Some(Arc::clone(stack)));
                    self.changed_cells();
                    observation::cells(self.session, record.sequence, self.epoch, 15);
                }
                Observation::Cursor(stack)
                    if record.sequence > self.epoch.max(self.authority_loss) =>
                {
                    self.cursor = Some(Arc::clone(stack));
                    self.changed_cells();
                    observation::cells(self.session, record.sequence, self.epoch, 16);
                }
                Observation::Cells(cells)
                    if record.sequence > self.epoch.max(self.authority_loss) =>
                {
                    let mut mask = 0;
                    for (index, stack) in cells.iter().enumerate() {
                        if let Some(stack) = stack {
                            if index < self.grid.len() {
                                self.grid[index] = Some(Arc::clone(stack));
                            } else {
                                self.cursor = Some(Arc::clone(stack));
                            }
                            mask |= 1 << index;
                        }
                    }
                    self.changed_cells();
                    observation::cells(self.session, record.sequence, self.epoch, mask);
                }
                Observation::Grid(_)
                | Observation::Cell { .. }
                | Observation::Cursor(_)
                | Observation::Cells(_) => {}
            }
            observation::discard(record.sequence);
        }
        if split < queue.records.len() {
            self.queue = Queue::replace(
                queue.records[split..].iter().cloned(),
                queue.records.len() - split,
                &self.credits,
            );
            if self.queue.is_none() {
                self.lose(
                    self.observed,
                    queue.records[split..]
                        .iter()
                        .fold(0, |d, r| d | r.observation.domain()),
                );
            }
        }
        self.match_if_changed();
    }

    fn match_if_changed(&mut self) {
        if self.exhausted
            || self.authority != Some(InventoryAuthority::Server)
            || self
                .cursor
                .as_ref()
                .is_none_or(|cursor| !cursor.stack.is_empty())
            || self.grid.iter().any(Option::is_none)
            || self.catalog_lost
        {
            self.preview = None;
            self.cache_key = None;
            return;
        }
        let Some(registry) = self.registry.as_ref() else {
            self.preview = None;
            return;
        };
        let key = (
            self.epoch,
            self.catalog.revision(),
            registry.snapshot.revision().get(),
            self.revision,
        );
        if self.cache_key == Some(key) {
            return;
        }
        let Some(permit) = self
            .credits
            .reserve(std::mem::size_of::<PreviewOwner>() + 128)
        else {
            self.lose(self.observed, 0);
            return;
        };
        let verified = self.grid.each_ref().map(|cell| {
            let stack = &cell.as_ref().expect("known grid gate").stack;
            // This inactive plain-item slice never materializes arbitrary NBT
            // while matching. Ordinary inventory retains its existing validator.
            if stack.is_empty()
                || !(stack.extra_data.is_empty() || stack.extra_data.as_ref() == [0; 10])
            {
                return None;
            }
            VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).ok()
        });
        let grid = std::array::from_fn(|index| {
            if self.grid[index]
                .as_ref()
                .expect("known grid gate")
                .stack
                .is_empty()
            {
                ManualCraftCell::Empty
            } else {
                verified[index]
                    .as_ref()
                    .map_or(ManualCraftCell::Unknown, ManualCraftCell::Present)
            }
        });
        let value =
            crate::match_manual_grid(&self.catalog, self.session, &registry.snapshot, &grid);
        self.preview = Some(Arc::new(PreviewOwner {
            value,
            _registry: Arc::clone(registry),
            _catalog: self.catalog.clone(),
            _permit: permit,
        }));
        self.cache_key = Some(key);
    }
}

#[cfg(test)]
mod tests;
