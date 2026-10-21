//! Effective item facts admitted once for each negotiated registry.

use assets::{
    facts::{FILLED_MAP_IDENTIFIER, ItemFact, ItemFacts, ItemTraits, ItemUseAnimationFact},
    item::{
        armour::{ItemArmour, ItemArmourSlot},
        durability::ItemDurability,
        fuel::ItemFuel,
        glint::ItemGlint,
    },
};
use protocol::{ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion};
use std::{collections::BTreeMap, sync::Arc};

/// An exact item identity within one admitted registry snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionItemKind(u16);

/// Session facts keyed by the server's IDs rather than the retail IDs.
#[derive(Clone, Debug, Default)]
pub struct SessionItemFacts {
    first_id: i32,
    indices: Arc<[Option<u16>]>,
    records: Arc<[SessionItemRecord]>,
    enchanted_book_id: Option<i32>,
}

#[derive(Clone, Debug)]
struct SessionItemRecord {
    kind: SessionItemKind,
    fact: ItemFact,
    retail_id: Option<i32>,
    armour_row: Option<u8>,
    banner: bool,
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
        let mut enchanted_book_id = None;
        let mut kinds = BTreeMap::new();
        for entry in entries {
            let offset = usize::try_from(i64::from(entry.network_id) - i64::from(first_id)).ok()?;
            indices[offset] = Some(u16::try_from(records.len()).ok()?);
            let next_kind = SessionItemKind(u16::try_from(kinds.len()).ok()?);
            let kind = *kinds.entry(entry.identifier.as_ref()).or_insert(next_kind);
            let record = resolve(pinned, entry, kind);
            if record.fact.traits.contains(ItemTraits::ENCHANTED_BOOK) {
                enchanted_book_id.get_or_insert(entry.network_id);
            }
            records.push(record);
        }
        Some(Self {
            first_id,
            indices: indices.into(),
            records: records.into(),
            enchanted_book_id,
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
    /// Returns the identity shared by registry aliases of the same item name.
    pub fn kind(&self, network_id: i32) -> Option<SessionItemKind> {
        Some(self.record(network_id)?.kind)
    }

    /// Retains the independent retail identity for exact repair-material comparisons.
    pub fn retail_id(&self, network_id: i32) -> Option<i32> {
        self.record(network_id)?.retail_id
    }
    /// Returns the cached equipment row, including declared body armor.
    pub fn armour_row(&self, network_id: i32) -> Option<u8> {
        self.record(network_id)?.armour_row
    }

    /// Identifies the admitted banner item independently of tags and stack-specific Type data.
    pub(crate) fn is_banner(&self, network_id: i32) -> bool {
        self.record(network_id).is_some_and(|record| record.banner)
    }

    /// Returns the negotiated output identity for enchanting a plain book.
    pub fn enchanted_book_id(&self) -> Option<i32> {
        self.enchanted_book_id
    }
}

/// Combines pinned identity with explicit server components at the admission boundary.
fn resolve(
    pinned: &ItemFacts,
    entry: &ItemRegistryEntry,
    kind: SessionItemKind,
) -> SessionItemRecord {
    if matches!(entry.version, ItemRegistryVersion::Unknown(_))
        || (entry.components.is_none() && !entry.canonical_empty_component_data)
    {
        return SessionItemRecord {
            kind,
            fact: ItemFact::default(),
            retail_id: None,
            armour_row: None,
            banner: false,
        };
    }
    let component_item =
        entry.component_based || matches!(entry.version, ItemRegistryVersion::DataDriven);
    let banner = entry.identifier.as_ref() == assets::facts::BANNER_IDENTIFIER;
    let pinned_fact = pinned.by_identifier(&entry.identifier);
    let retail_id = if component_item {
        None
    } else {
        pinned.network_id(&entry.identifier)
    };
    let mut fact = if component_item {
        ItemFact::default()
    } else {
        pinned_fact.copied().unwrap_or_default()
    };
    // Preserve admission for registered native maps outside the retail census.
    if !component_item
        && pinned_fact.is_none()
        && entry.identifier.as_ref() == FILLED_MAP_IDENTIFIER
    {
        fact.allow_off_hand = Some(true);
        fact.offhand_provisional = true;
    }
    fact.max_stack_size = super::inventory_ledger::registry::entry_capacity_with_base(
        entry,
        pinned_fact.and_then(|fact| fact.max_stack_size),
    );
    pinned.apply_tags(&mut fact, entry.item_tags.iter().map(AsRef::as_ref));
    let mut armour_row = fact.armour_slot.map(armour_slot_row);
    let components = entry.components.as_deref();
    if components.and_then(|components| components.wearable_slot.as_deref())
        == Some("slot.weapon.offhand")
    {
        fact.allow_off_hand = Some(true);
        fact.offhand_provisional = false;
    } else if let Some(components) =
        components.filter(|components| components.allow_off_hand_declared)
    {
        fact.allow_off_hand = components.allow_off_hand;
        fact.offhand_provisional = false;
    }
    let Some(components) = entry.components.as_deref() else {
        return SessionItemRecord {
            kind,
            fact,
            retail_id,
            armour_row,
            banner,
        };
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
        armour_row = fact.armour_slot.map(armour_slot_row).or_else(|| {
            (components.wearable_slot.as_deref() == Some("slot.armor.body")).then_some(4)
        });
        fact.armour = fact
            .armour_slot
            .zip(components.wearable_protection)
            .map_or(ItemArmour::Unresolved, |(slot, protection)| {
                ItemArmour::Wearable { slot, protection }
            });
    }
    if components.durability_declared {
        fact.durability =
            ItemDurability::from_declared_maximum(components.max_durability, component_item);
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
    fact.grip = fact.grip.with_hand_equipped(
        components.hand_equipped,
        components.hand_equipped_declared,
        component_item,
    );
    fact.use_animation = components
        .use_animation
        .as_deref()
        .and_then(ItemUseAnimationFact::from_component);
    fact.use_duration_ticks = components.use_duration_ticks;
    SessionItemRecord {
        kind,
        fact,
        retail_id,
        armour_row,
        banner,
    }
}

/// Maps the carrier's humanoid equipment slots to protocol armor rows.
fn armour_slot_row(slot: ItemArmourSlot) -> u8 {
    match slot {
        ItemArmourSlot::Head => 0,
        ItemArmourSlot::Chest => 1,
        ItemArmourSlot::Legs => 2,
        ItemArmourSlot::Feet => 3,
    }
}

#[cfg(test)]
mod tests;
