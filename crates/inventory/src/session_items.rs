//! Effective item facts admitted once for each negotiated registry.

use assets::{
    facts::{ItemFact, ItemFacts, ItemUseAnimationFact},
    item::{
        armour::{ItemArmour, ItemArmourSlot},
        durability::ItemDurability,
        fuel::ItemFuel,
        glint::ItemGlint,
        grip::ItemGrip,
    },
};
use protocol::{ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion};
use std::sync::Arc;

/// Session facts keyed by the server's IDs rather than the retail IDs.
#[derive(Clone, Debug, Default)]
pub struct SessionItemFacts {
    first_id: i32,
    indices: Arc<[Option<u16>]>,
    records: Arc<[SessionItemRecord]>,
}

#[derive(Clone, Debug)]
struct SessionItemRecord {
    fact: ItemFact,
    retail_id: Option<i32>,
}

impl SessionItemFacts {
    /// Builds one immutable registry admission; duplicate IDs or an unbounded span reject it.
    pub fn from_registry(pinned: &ItemFacts, event: &ItemRegistryEvent) -> Option<Self> {
        if event.entries.len() > protocol::MAX_ITEM_REGISTRY_ENTRIES {
            return None;
        }
        let mut entries: Vec<_> = event.entries.iter().collect();
        entries.sort_unstable_by_key(|entry| entry.network_id);
        if entries
            .windows(2)
            .any(|pair| pair[0].network_id == pair[1].network_id)
        {
            return None;
        }
        let Some(first) = entries.first() else {
            return Some(Self::default());
        };
        let first_id = first.network_id;
        let span = usize::try_from(i64::from(entries.last()?.network_id) - i64::from(first_id) + 1)
            .ok()?;
        if span > usize::from(u16::MAX) + 1 {
            return None;
        }
        let mut indices = vec![None; span];
        let mut records = Vec::with_capacity(entries.len());
        for entry in entries {
            let offset = usize::try_from(i64::from(entry.network_id) - i64::from(first_id)).ok()?;
            indices[offset] = Some(u16::try_from(records.len()).ok()?);
            records.push(resolve(pinned, entry));
        }
        Some(Self {
            first_id,
            indices: indices.into(),
            records: records.into(),
        })
    }

    /// Resolves a bounded signed ID with indexed reads, preserving sparse gaps.
    fn record(&self, network_id: i32) -> Option<&SessionItemRecord> {
        let offset = usize::try_from(i64::from(network_id) - i64::from(self.first_id)).ok()?;
        let index = (*self.indices.get(offset)?)?;
        self.records.get(usize::from(index))
    }

    /// Unknown IDs return one unresolved record without a name lookup or allocation.
    pub fn get(&self, network_id: i32) -> &ItemFact {
        static UNRESOLVED: std::sync::OnceLock<ItemFact> = std::sync::OnceLock::new();
        self.record(network_id)
            .map(|record| &record.fact)
            .unwrap_or_else(|| UNRESOLVED.get_or_init(ItemFact::default))
    }
    /// Retains the independent retail identity for exact repair-material comparisons.
    pub fn retail_id(&self, network_id: i32) -> Option<i32> {
        self.record(network_id)?.retail_id
    }
}

/// Combines pinned identity with explicit server components at the admission boundary.
fn resolve(pinned: &ItemFacts, entry: &ItemRegistryEntry) -> SessionItemRecord {
    if matches!(entry.version, ItemRegistryVersion::Unknown(_))
        || (entry.components.is_none() && !entry.canonical_empty_component_data)
    {
        return SessionItemRecord {
            fact: ItemFact::default(),
            retail_id: None,
        };
    }
    let component_item =
        entry.component_based || matches!(entry.version, ItemRegistryVersion::DataDriven);
    let retail_id = if component_item {
        None
    } else {
        pinned.network_id(&entry.identifier)
    };
    let mut fact = if component_item {
        ItemFact::default()
    } else {
        pinned
            .by_identifier(&entry.identifier)
            .copied()
            .unwrap_or_default()
    };
    fact.max_stack_size = super::inventory_ledger::registry::entry_capacity_with_base(
        entry,
        pinned
            .by_identifier(&entry.identifier)
            .and_then(|fact| fact.max_stack_size),
    );
    if component_item {
        pinned.apply_tags(&mut fact, entry.item_tags.iter().map(AsRef::as_ref));
    }
    let Some(components) = entry.components.as_deref() else {
        return SessionItemRecord { fact, retail_id };
    };
    if component_item {
        if !components.wearable_declared {
            fact.armour = ItemArmour::NotArmour;
        }
        if !components.durability_declared {
            fact.durability = ItemDurability::NotDamageable;
        }
    }
    if components.wearable_declared {
        fact.armour_slot = match components.wearable_slot.as_deref() {
            Some("slot.armor.head") => Some(ItemArmourSlot::Head),
            Some("slot.armor.chest") => Some(ItemArmourSlot::Chest),
            Some("slot.armor.legs") => Some(ItemArmourSlot::Legs),
            Some("slot.armor.feet") => Some(ItemArmourSlot::Feet),
            _ => None,
        };
        fact.armour = fact
            .armour_slot
            .zip(components.wearable_protection)
            .map_or(ItemArmour::Unresolved, |(slot, protection)| {
                ItemArmour::Wearable { slot, protection }
            });
    }
    if components.durability_declared {
        fact.durability = components
            .max_durability
            .filter(|maximum| *maximum > 0)
            .map_or(ItemDurability::Unresolved, |maximum| {
                ItemDurability::Maximum { maximum }
            });
    }
    if components.fuel_declared {
        fact.fuel = components
            .fuel_duration_ticks
            .map_or(ItemFuel::Unresolved, |burn_ticks| ItemFuel::Session {
                burn_ticks,
            });
    }
    if components.glint_declared {
        fact.glint = components
            .glint
            .map_or(ItemGlint::Unresolved, |enabled| ItemGlint::Session {
                enabled,
            });
    }
    if components.hand_equipped_declared {
        fact.grip = components
            .hand_equipped
            .map_or(ItemGrip::Unresolved, |upright| ItemGrip::Session {
                upright,
                mirrored_art: false,
            });
    }
    if components.allow_off_hand_declared {
        fact.allow_off_hand = components.allow_off_hand;
        fact.offhand_provisional = false;
    }
    fact.use_animation = components
        .use_animation
        .as_deref()
        .and_then(ItemUseAnimationFact::from_component);
    fact.use_duration_ticks = components.use_duration_ticks;
    SessionItemRecord { fact, retail_id }
}

#[cfg(test)]
mod tests;
