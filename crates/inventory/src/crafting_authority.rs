//! Inactive, committed crafting projection. This owns no request, journal or UI.
mod budget;
mod observation;
mod policy;
pub(crate) use policy::BookDiscoveryKey;
mod projection;

use projection::{
    ALL_DOMAINS, AUTHORITY_DOMAIN, CATALOG_DOMAIN, CRAFTING_RULES_DOMAIN, Observation, Queue,
    REGISTRY_DOMAIN, Record, RegistryOwner, StackOwner, UNLOCKS_DOMAIN, event_domain,
};
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
    _catalog: Arc<RecipeCatalog>,
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
    /// Latest accepted sequence per domain, including events in the outer FIFO.
    pending_domains: [u64; u8::BITS as usize],
    consumed: u64,
    authority_loss: u64,
    generation: u64,
    exhausted: bool,
    authority: Option<InventoryAuthority>,
    registry: Option<Arc<RegistryOwner>>,
    /// Shared so presentation copies never deep-clone the recipe lists.
    catalog: Arc<RecipeCatalog>,
    catalog_lost: bool,
    unlocks: protocol::RecipeUnlockState,
    unlock_component: Option<bool>,
    rules: protocol::CraftingRules,
    grid: [Option<Arc<StackOwner>>; crate::inventory_ledger::PERSONAL_INPUT_COUNT],
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
            pending_domains: [0; u8::BITS as usize],
            consumed: 0,
            authority_loss: 0,
            generation: 0,
            exhausted: false,
            authority: None,
            registry: None,
            catalog: Arc::new(catalog),
            catalog_lost: true,
            unlocks: Default::default(),
            unlock_component: Some(false),
            rules: protocol::CraftingRules::default().resolved(),
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
            None => {
                self.exhausted = true;
                self.queue = None;
                self.retire_domains(ALL_DOMAINS, self.observed);
            }
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
                domains |= record.domain;
            }
        }
        self.barrier = barrier;
        for (bit, sequence) in self.pending_domains.iter().enumerate() {
            if *sequence > self.consumed && *sequence <= barrier {
                domains |= 1 << bit;
            }
        }
        self.clear_cells();
        self.retire_domains(if self.exhausted { ALL_DOMAINS } else { domains }, barrier);
    }

    /// Retires selected facts without advancing the cell revision or changing snapshots.
    fn retire_domains(&mut self, domains: u8, barrier: u64) {
        if domains & REGISTRY_DOMAIN != 0 {
            self.registry = None;
        }
        if domains & CATALOG_DOMAIN != 0 {
            Arc::make_mut(&mut self.catalog).begin_session(self.session);
            self.catalog_lost = true;
        }
        if domains & UNLOCKS_DOMAIN != 0 {
            self.unlocks.clear();
            self.unlock_component = None;
        }
        if domains & CRAFTING_RULES_DOMAIN != 0 {
            self.rules = protocol::CraftingRules::default();
        }
        if domains & AUTHORITY_DOMAIN != 0 {
            self.authority = None;
            self.authority_loss = self.authority_loss.max(barrier);
        }
    }

    pub(super) fn bypass(&mut self, sequence: u64) {
        self.lose(sequence, ALL_DOMAINS);
    }

    /// Tracks admitted facts before observation, so loss also covers unstaged events.
    pub(super) fn note_ingress(&mut self, sequence: u64, event: &InventoryAuthorityEvent) {
        self.observed = self.observed.max(sequence);
        let domain = event_domain(event);
        for (bit, latest) in self.pending_domains.iter_mut().enumerate() {
            if domain & (1 << bit) != 0 {
                *latest = (*latest).max(sequence);
            }
        }
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
        (self.catalog.is_available() && !self.catalog_lost).then_some(&*self.catalog)
    }

    /// Retains the committed recipe allocation for numeric inventory admission.
    pub(super) fn catalog_snapshot(&self) -> Option<Arc<RecipeCatalog>> {
        self.catalog().map(|_| Arc::clone(&self.catalog))
    }

    /// The committed name authority; recipe and unlock observations share one FIFO.
    pub(super) fn recipe_unlocks(&self) -> &protocol::RecipeUnlockState {
        &self.unlocks
    }

    /// Complete rules at the committed frontier; partial recovery cannot restore missing fields.
    pub(super) fn crafting_rules(&self) -> Option<protocol::CraftingRules> {
        self.rules.is_complete().then_some(self.rules)
    }

    /// Publishes startup rules before any session observations can replace them.
    pub(super) fn bootstrap_rules(&mut self, rules: protocol::CraftingRules) {
        if self.accepts_bootstrap() {
            self.rules = rules.resolved();
            self.changed_cells();
        }
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
            self.lose(self.observed, ALL_DOMAINS);
            return;
        };
        if stream == 0 || epoch > through || self.stream.is_some_and(|old| old != stream) {
            self.lose(through, ALL_DOMAINS);
        }
        if stream == 0
            || epoch > through
            || (self.stream == Some(stream) && (epoch < self.epoch || through < self.consumed))
        {
            self.through = None;
            self.lose(through, ALL_DOMAINS);
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

    /// Seeds startup authority and absent discovery without granting cell facts.
    pub(super) fn bootstrap(
        &mut self,
        registry: Option<&protocol::ItemRegistryEvent>,
        authority: InventoryAuthority,
    ) {
        if !self.accepts_bootstrap() {
            return;
        }
        self.registry = registry.and_then(|event| self.registry_owner(event));
        self.authority = Some(authority);
        self.unlock_component = Some(false);
        observation::bootstrap(self.session, self.registry.is_some());
    }

    /// Bootstrap facts are admissible only before the stream or an authority loss begins.
    fn accepts_bootstrap(&self) -> bool {
        self.stream.is_none()
            && self.observed == 0
            && self.consumed == 0
            && self.barrier == 0
            && self.authority_loss == 0
            && self.epoch == 0
    }

    /// Routes fixture events through the same authority owner used by real ingress.
    #[cfg(test)]
    pub(super) fn observe(&mut self, session: u64, sequence: u64, event: &InventoryAuthorityEvent) {
        let mut ledger = crate::PlayerInventoryLedger::default();
        if let InventoryAuthorityEvent::Inventory(event) = event {
            ledger.apply(event);
        }
        self.observe_admitted(session, sequence, event, ledger.personal_updates());
    }

    /// Retains only authoritative personal writes admitted by the ledger at this FIFO receipt.
    pub(super) fn observe_admitted(
        &mut self,
        session: u64,
        sequence: u64,
        event: &InventoryAuthorityEvent,
        writes: crate::inventory_ledger::PersonalUpdates<'_>,
    ) {
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
                    self.lose(sequence, REGISTRY_DOMAIN);
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
            InventoryAuthorityEvent::Inventory(InventoryEvent::RecipeUnlocks(update)) => {
                Observation::RecipeUnlocks(update.clone())
            }
            InventoryAuthorityEvent::Inventory(InventoryEvent::CraftingRules(rules)) => {
                Observation::CraftingRules(*rules)
            }
            _ if writes.iter().any(Option::is_some) => {
                let Some(cells) = StackOwner::updates_with_credits(writes, &self.credits) else {
                    self.lose(sequence, 0);
                    return;
                };
                Observation::Cells(cells)
            }
            _ => return,
        };
        let domain = event_domain(event);
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
                domain,
                observation,
            })),
            count,
            &self.credits,
        );
        match replacement {
            Some(queue) => {
                self.queue = Some(queue);
                let mask = writes.iter().enumerate().fold(0, |mask, (index, cell)| {
                    mask | if cell.is_some() { 1 << index } else { 0 }
                });
                observation::stage(session, sequence, event, mask);
            }
            None => self.lose(sequence, domain),
        }
    }

    pub(super) fn advance(&mut self) {
        if self.exhausted {
            return;
        }
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
                        let applied = Arc::make_mut(&mut self.catalog).apply(
                            self.session,
                            record.sequence,
                            update,
                        );
                        self.catalog_lost = !self.catalog.is_available();
                        if applied {
                            observation::recipe(record.sequence, self.catalog.is_available());
                        }
                    }
                }
                Observation::RecipeUnlocks(update) => {
                    self.unlock_component = if update.is_unavailable() {
                        self.unlock_component.filter(|present| *present)
                    } else {
                        Some(true)
                    };
                    self.unlocks.apply(&self.catalog, update);
                    self.changed_cells();
                }
                Observation::CraftingRules(update) => {
                    let previous = self.rules;
                    self.rules.apply(*update);
                    if self.rules != previous {
                        self.changed_cells();
                    }
                }
                Observation::Authority(authority) if record.sequence > self.authority_loss => {
                    self.authority = Some(*authority)
                }
                Observation::Authority(_) => {}
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
                Observation::Cells(_) => {}
            }
            observation::discard(record.sequence);
            if self.exhausted {
                self.lose(self.observed, ALL_DOMAINS);
                return;
            }
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
                    queue.records[split..].iter().fold(0, |d, r| d | r.domain),
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
            if stack.is_empty() {
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
        let value = if self.manual_recipes_available() {
            crate::matching::match_manual_grid_where(
                &self.catalog,
                self.session,
                &registry.snapshot,
                &grid,
                |recipe| self.manual_recipe_allowed(recipe),
            )
        } else {
            ManualCraftMatch::Unavailable
        };
        self.preview = Some(Arc::new(PreviewOwner {
            value,
            _registry: Arc::clone(registry),
            _catalog: Arc::clone(&self.catalog),
            _permit: permit,
        }));
        self.cache_key = Some(key);
    }
}

#[cfg(test)]
mod tests;
