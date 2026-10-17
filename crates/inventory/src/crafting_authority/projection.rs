use protocol::{ItemRegistryEvent, NetworkItemStack, RecipeRegistrySnapshot};
use std::{mem::size_of, num::NonZeroU64, sync::Arc};

use super::budget::{Credits, Permit};

/// Resolves packet-context personal inputs and cursor before named response aliases.
pub(super) fn personal_cell_index(identity: protocol::SlotIdentity) -> Option<usize> {
    if identity.container.window_id == Some(protocol::UI_INVENTORY_WINDOW_ID) {
        return if identity.slot == 0 {
            Some(4)
        } else {
            protocol::personal_craft_slot_index(&identity.container, identity.slot).map(usize::from)
        };
    }
    match protocol::project_container_cell(&identity.container, identity.slot) {
        Some(protocol::CanonicalCell::CraftInput(index)) => Some(usize::from(index)),
        Some(protocol::CanonicalCell::Cursor) => Some(4),
        _ => None,
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

    /// Reserves the whole observation before cloning any stack, publishing no partial batch.
    pub(super) fn many_with_credits<const N: usize>(
        stacks: [&NetworkItemStack; N],
        credits: &Arc<Credits>,
    ) -> Option<[Arc<Self>; N]> {
        let mut charges = [0; N];
        for (charge, stack) in charges.iter_mut().zip(stacks) {
            *charge = Self::charge(stack)?;
        }
        charges
            .iter()
            .try_fold(0usize, |sum, charge| sum.checked_add(*charge))?;
        let mut permits = std::array::from_fn::<_, N, _>(|index| credits.reserve(charges[index]));
        if permits.iter().any(Option::is_none) {
            return None;
        }
        Some(std::array::from_fn(|index| {
            Arc::new(Self {
                stack: stacks[index].clone(),
                _permit: permits[index]
                    .take()
                    .expect("all observation credits reserved"),
            })
        }))
    }

    pub(super) fn with_credits(
        stack: &NetworkItemStack,
        credits: &Arc<Credits>,
    ) -> Option<Arc<Self>> {
        let bytes = Self::charge(stack)?;
        let permit = credits.reserve(bytes)?;
        Some(Arc::new(Self {
            stack: stack.clone(),
            _permit: permit,
        }))
    }
}

#[derive(Debug, Clone)]
pub(super) enum Observation {
    Cell {
        index: usize,
        stack: Arc<StackOwner>,
    },
    Cursor(Arc<StackOwner>),
    /// One FIFO transaction can replace several crafting cells and the cursor.
    Cells([Option<Arc<StackOwner>>; 5]),
    Registry(Option<Arc<RegistryOwner>>),
    Recipes(protocol::RecipeUpdate),
    Authority(protocol::InventoryAuthority),
}

impl Observation {
    pub(super) fn domain(&self) -> u8 {
        match self {
            Self::Registry(_) => 1,
            Self::Recipes(_) => 2,
            Self::Authority(_) => 4,
            Self::Cell { .. } | Self::Cursor(_) | Self::Cells(_) => 0,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Record {
    pub(super) sequence: u64,
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
