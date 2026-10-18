use protocol::{ItemRegistryEvent, NetworkItemStack, RecipeRegistrySnapshot};
use std::{mem::size_of, num::NonZeroU64, sync::Arc};

use super::budget::{Credits, Permit};

pub(super) const REGISTRY_DOMAIN: u8 = 1;
pub(super) const CATALOG_DOMAIN: u8 = 1 << 1;
pub(super) const AUTHORITY_DOMAIN: u8 = 1 << 2;
pub(super) const UNLOCKS_DOMAIN: u8 = 1 << 3;
/// Rule observations form an independent authority domain during queue loss.
pub(super) const CRAFTING_RULES_DOMAIN: u8 = 1 << 4;
/// Full loss includes every domain bit, including domains introduced later.
pub(super) const ALL_DOMAINS: u8 = u8::MAX;

/// Classifies pending authority before observation can allocate or discard its payload.
pub(super) fn event_domain(event: &crate::InventoryAuthorityEvent) -> u8 {
    use crate::InventoryAuthorityEvent;
    match event {
        InventoryAuthorityEvent::Registry(_) => REGISTRY_DOMAIN,
        InventoryAuthorityEvent::Inventory(protocol::InventoryEvent::Recipes(_)) => CATALOG_DOMAIN,
        InventoryAuthorityEvent::Inventory(protocol::InventoryEvent::Authority(_)) => {
            AUTHORITY_DOMAIN
        }
        InventoryAuthorityEvent::Inventory(protocol::InventoryEvent::RecipeUnlocks(_)) => {
            UNLOCKS_DOMAIN
        }
        InventoryAuthorityEvent::Inventory(protocol::InventoryEvent::CraftingRules(_)) => {
            CRAFTING_RULES_DOMAIN
        }
        _ => 0,
    }
}

pub(super) const MAX_RECORDS: usize = 64;

#[derive(Debug)]
pub(super) struct RegistryOwner {
    pub(super) snapshot: RecipeRegistrySnapshot,
    _permit: Permit,
}

impl RegistryOwner {
    pub(super) fn with_credits(
        event: &ItemRegistryEvent,
        generation: NonZeroU64,
        credits: &Arc<Credits>,
    ) -> Option<Arc<Self>> {
        if event.entries.len() > protocol::MAX_ITEM_REGISTRY_ENTRIES {
            return None;
        }
        let mut bytes = size_of::<Self>().checked_add(128)?;
        for entry in event.entries.iter() {
            bytes = bytes
                .checked_add(size_of::<protocol::ItemRegistryEntry>())?
                .checked_add(size_of::<usize>())?
                .checked_add(entry.identifier.len())?
                .checked_add(32)?
                .checked_add(entry.item_tags.len().checked_mul(size_of::<Arc<str>>())?)?
                .checked_add(32)?;
            if let Some(components) = &entry.components {
                bytes = bytes.checked_add(components.retained_bytes())?;
            }
            for tag in entry.item_tags.iter() {
                bytes = bytes.checked_add(tag.len())?.checked_add(32)?;
            }
        }
        // Credit precedes numeric index construction and retention of entry/name Arcs.
        let permit = credits.reserve(bytes)?;
        let snapshot = RecipeRegistrySnapshot::new(generation, Arc::clone(&event.entries)).ok()?;
        Some(Arc::new(Self {
            snapshot,
            _permit: permit,
        }))
    }
}

#[derive(Debug)]
pub(super) struct StackOwner {
    pub(super) stack: NetworkItemStack,
    _permit: Permit,
}

impl StackOwner {
    fn charge(stack: &NetworkItemStack) -> Option<usize> {
        size_of::<Self>()
            .checked_add(stack.extra_data.len())?
            .checked_add(64)
    }

    /// Reserves every admitted cell before cloning, publishing no partial batch on refusal.
    pub(super) fn updates_with_credits<const N: usize>(
        updates: [Option<Option<&NetworkItemStack>>; N],
        credits: &Arc<Credits>,
    ) -> Option<[Option<Arc<Self>>; N]> {
        let empty = NetworkItemStack::empty();
        let stacks = updates.map(|update| update.map(|stack| stack.unwrap_or(&empty)));
        let mut permits: [Option<Permit>; N] = std::array::from_fn(|_| None);
        for (index, stack) in stacks.iter().enumerate() {
            if let Some(stack) = stack {
                permits[index] = Some(credits.reserve(Self::charge(stack)?)?);
            }
        }
        Some(std::array::from_fn(|index| {
            let stack = stacks[index]?;
            Some(Arc::new(Self {
                stack: stack.clone(),
                _permit: permits[index]
                    .take()
                    .expect("all admitted cell credits reserved"),
            }))
        }))
    }
}

#[derive(Debug, Clone)]
pub(super) enum Observation {
    /// One FIFO transaction can replace several crafting cells and the cursor.
    Cells([Option<Arc<StackOwner>>; crate::inventory_ledger::PERSONAL_UPDATE_COUNT]),
    Registry(Option<Arc<RegistryOwner>>),
    Recipes(protocol::RecipeUpdate),
    RecipeUnlocks(protocol::RecipeUnlockUpdate),
    CraftingRules(protocol::CraftingRules),
    Authority(protocol::InventoryAuthority),
}

#[derive(Debug, Clone)]
pub(super) struct Record {
    pub(super) sequence: u64,
    pub(super) domain: u8,
    pub(super) observation: Observation,
}

/// Copies of a runtime share queue allocation; a replacement gets its own credit.
#[derive(Debug)]
pub(super) struct Queue {
    pub(super) records: Vec<Record>,
    _permit: Permit,
}

impl Queue {
    pub(super) fn replace(
        records: impl Iterator<Item = Record>,
        count: usize,
        credits: &Arc<Credits>,
    ) -> Option<Arc<Self>> {
        if count > MAX_RECORDS {
            return None;
        }
        let bytes = count
            .checked_mul(size_of::<Record>())?
            .checked_add(size_of::<Self>() + 64)?;
        let permit = credits.reserve(bytes)?;
        let mut retained = Vec::new();
        retained.try_reserve_exact(count).ok()?;
        for record in records {
            if retained.len() == count {
                return None;
            }
            retained.push(record);
        }
        if retained.len() != count {
            return None;
        }
        Some(Arc::new(Self {
            records: retained,
            _permit: permit,
        }))
    }
}
