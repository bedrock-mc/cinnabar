//! Bounded immutable presentation facts keyed by retained extra-data identity.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use super::{
    ItemDisplay, item_charged_projectile, item_custom_color, item_display, item_extra_damage,
    item_extra_unbreakable, item_has_enchantment_list,
};

const MAX_FACT_ENTRIES: usize = 1_024;
const MAX_FACT_BYTES: usize = 4 * 1024 * 1024;

/// NBT-derived facts; registry traits and dynamic authority remain separate.
#[derive(Debug, Default)]
pub struct ItemStackFacts {
    pub display: ItemDisplay,
    pub damage: Option<u32>,
    pub unbreakable: bool,
    pub has_enchantment_list: bool,
    pub charged_projectile: Option<Arc<str>>,
    pub custom_color: Option<u32>,
}

type Key = (usize, usize, bool);

#[derive(Debug)]
struct Entry {
    // Retaining the source prevents its address being reused while it is cached.
    _source: Arc<[u8]>,
    facts: Arc<ItemStackFacts>,
    bytes: usize,
    touched: u64,
}

#[derive(Debug, Default)]
struct Cache {
    entries: HashMap<Key, Entry>,
    bytes: usize,
    clock: u64,
}

/// A bounded session cache shared by the frame's item presentation consumers.
#[derive(Debug, Clone, Default)]
pub struct ItemStackFactsCache(Arc<Mutex<Cache>>);

impl ItemStackFactsCache {
    /// Resolves one immutable extra-data allocation, decoding projectiles only for crossbows.
    pub fn get(&self, source: &Arc<[u8]>, crossbow: bool) -> Arc<ItemStackFacts> {
        if super::decode_extra_nbt(source).is_none() {
            static EMPTY: OnceLock<Arc<ItemStackFacts>> = OnceLock::new();
            return Arc::clone(EMPTY.get_or_init(Default::default));
        }
        let key = (source.as_ptr().addr(), source.len(), crossbow);
        let mut cache = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.clock = cache.clock.wrapping_add(1);
        let touched = cache.clock;
        if let Some(entry) = cache.entries.get_mut(&key) {
            entry.touched = touched;
            return Arc::clone(&entry.facts);
        }
        let facts = Arc::new(ItemStackFacts {
            display: item_display(source),
            damage: item_extra_damage(source),
            unbreakable: item_extra_unbreakable(source),
            has_enchantment_list: item_has_enchantment_list(source),
            charged_projectile: crossbow.then(|| item_charged_projectile(source)).flatten(),
            custom_color: item_custom_color(source),
        });
        let bytes = source.len()
            + facts.display.name.as_ref().map_or(0, |name| name.len())
            + facts
                .display
                .lore
                .iter()
                .map(|line| line.len())
                .sum::<usize>()
            + facts.display.enchantments.len() * std::mem::size_of::<(i16, u8)>()
            + facts
                .charged_projectile
                .as_ref()
                .map_or(0, |name| name.len());
        if bytes > MAX_FACT_BYTES {
            return facts;
        }
        while cache.entries.len() >= MAX_FACT_ENTRIES || cache.bytes + bytes > MAX_FACT_BYTES {
            let Some(oldest) = cache
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.touched)
                .map(|(key, _)| *key)
            else {
                break;
            };
            if let Some(entry) = cache.entries.remove(&oldest) {
                cache.bytes -= entry.bytes;
            }
        }
        cache.bytes += bytes;
        cache.entries.insert(
            key,
            Entry {
                _source: Arc::clone(source),
                facts: Arc::clone(&facts),
                bytes,
                touched,
            },
        );
        facts
    }
}

#[cfg(test)]
mod tests;
