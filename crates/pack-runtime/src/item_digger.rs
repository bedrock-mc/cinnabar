//! Matches a server item's `minecraft:digger` speeds against the block being mined.

use protocol::{DiggerBlock, ItemDigger, ItemRegistryEntry};
use std::{collections::HashMap, sync::Arc};

/// The digger entry that decides one destroy, or why none can be predicted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiggerMatch {
    /// The first matching entry's speed; a match also allows the block's drops.
    Speed(i32),
    /// No entry matches, so the item mines like an empty hand.
    None,
    /// A tag query could not be prepared, or a state-restricted entry names this block.
    Unresolved,
}

/// Block tag programs owned by one active item registry.
///
/// Replacement registries share programs for live queries and release obsolete ones.
#[derive(Debug, Default, Clone)]
pub struct DiggerFilters(HashMap<Arc<str>, Arc<Option<pack_entity_compiler::ItemTagMolang>>>);

impl DiggerFilters {
    /// Compiles each distinct query in an item registry on the caller's preparation worker.
    pub fn new<'a>(entries: impl IntoIterator<Item = &'a ItemRegistryEntry>) -> Self {
        Self::default().replaced(entries)
    }

    /// Prepares exactly the replacement registry's queries, reusing programs still in use.
    /// Must run on a preparation worker because new queries require parsing.
    pub fn replaced<'a>(&self, entries: impl IntoIterator<Item = &'a ItemRegistryEntry>) -> Self {
        let mut replacement = Self::default();
        let diggers = entries
            .into_iter()
            .filter_map(|entry| entry.components.as_ref()?.digger.as_deref());
        for digger in diggers {
            for entry in digger.destroy_speeds.iter() {
                let DiggerBlock::Tags(expression) = &entry.block else {
                    continue;
                };
                replacement
                    .0
                    .entry(Arc::clone(expression))
                    .or_insert_with(|| {
                        self.0.get(expression).cloned().unwrap_or_else(|| {
                            Arc::new(pack_entity_compiler::ItemTagMolang::parse(expression))
                        })
                    });
            }
        }
        replacement
    }

    /// Evaluates only prepared programs; a missing or unsupported query stays unresolved.
    fn evaluate(&self, source: &str, has_tag: &dyn Fn(&str) -> bool) -> Option<bool> {
        self.0.get(source)?.as_ref().as_ref()?.evaluate(has_tag)
    }
}

/// Selects the first entry that matches the block's identifier or tags, in declaration order.
pub fn digger_match(
    digger: &ItemDigger,
    filters: &DiggerFilters,
    block: &str,
    has_tag: &dyn Fn(&str) -> bool,
) -> DiggerMatch {
    for entry in digger.destroy_speeds.iter() {
        let matched = match &entry.block {
            DiggerBlock::Name(name) => name.as_ref() == block,
            DiggerBlock::NameWithStates(name) if name.as_ref() == block => {
                return DiggerMatch::Unresolved;
            }
            DiggerBlock::NameWithStates(_) => false,
            DiggerBlock::Tags(expression) => match filters.evaluate(expression, has_tag) {
                Some(matched) => matched,
                None => return DiggerMatch::Unresolved,
            },
        };
        if matched {
            return DiggerMatch::Speed(entry.speed);
        }
    }
    DiggerMatch::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::DiggerSpeed;

    fn digger(entries: &[(DiggerBlock, i32)]) -> ItemDigger {
        ItemDigger {
            use_efficiency: false,
            destroy_speeds: entries
                .iter()
                .map(|(block, speed)| DiggerSpeed {
                    block: block.clone(),
                    speed: *speed,
                })
                .collect(),
        }
    }

    fn registry_entry(digger: &ItemDigger) -> ItemRegistryEntry {
        ItemRegistryEntry {
            components: Some(Arc::new(protocol::ItemComponents {
                digger: Some(Arc::new(digger.clone())),
                ..Default::default()
            })),
            ..protocol::vanilla_item_registry()[0].clone()
        }
    }

    fn filters(digger: &ItemDigger) -> DiggerFilters {
        DiggerFilters::new([&registry_entry(digger)])
    }

    /// Reports exactly the listed tags.
    fn tags(list: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |tag| list.contains(&tag)
    }

    #[test]
    fn first_matching_name_or_tag_entry_decides_the_speed() {
        let digger = digger(&[
            (DiggerBlock::Name("minecraft:red_concrete".into()), 30),
            (
                DiggerBlock::Tags(
                    "q.any_tag('stone_pick_diggable', 'minecraft:is_pickaxe_item_destructible')"
                        .into(),
                ),
                8,
            ),
            (DiggerBlock::Name("minecraft:stone".into()), 2),
        ]);
        let filters = filters(&digger);
        let pickaxe = tags(&["minecraft:is_pickaxe_item_destructible"]);
        assert_eq!(
            digger_match(&digger, &filters, "minecraft:red_concrete", &pickaxe),
            DiggerMatch::Speed(30)
        );
        assert_eq!(
            digger_match(&digger, &filters, "minecraft:stone", &pickaxe),
            DiggerMatch::Speed(8)
        );
        assert_eq!(
            digger_match(&digger, &filters, "minecraft:dirt", &tags(&[])),
            DiggerMatch::None
        );
    }

    #[test]
    fn unprepared_queries_and_state_restricted_names_stay_unresolved() {
        let tagged = digger(&[(DiggerBlock::Tags("q.any_tag('stone')".into()), 12)]);
        assert_eq!(
            digger_match(
                &tagged,
                &DiggerFilters::default(),
                "minecraft:stone",
                &tags(&["stone"])
            ),
            DiggerMatch::Unresolved
        );
        let states = digger(&[(
            DiggerBlock::NameWithStates("minecraft:stone_slab".into()),
            8,
        )]);
        let filters = filters(&states);
        assert_eq!(
            digger_match(&states, &filters, "minecraft:stone_slab", &tags(&[])),
            DiggerMatch::Unresolved
        );
        assert_eq!(
            digger_match(&states, &filters, "minecraft:dirt", &tags(&[])),
            DiggerMatch::None
        );
    }

    #[test]
    fn replacement_retires_removed_queries_and_shares_live_programs() {
        let first = digger(&[(DiggerBlock::Tags("q.any_tag('stone')".into()), 12)]);
        let later = digger(&[(DiggerBlock::Tags("q.any_tag('dirt')".into()), 4)]);
        let shared = registry_entry(&first);
        let added = registry_entry(&later);
        let initial = filters(&first);
        let both = initial.replaced([&shared, &added]);
        assert!(Arc::ptr_eq(
            initial.0.get("q.any_tag('stone')").unwrap(),
            both.0.get("q.any_tag('stone')").unwrap(),
        ));
        let replacement = both.replaced([&added]);
        assert_eq!(
            digger_match(&later, &replacement, "minecraft:dirt", &tags(&["dirt"])),
            DiggerMatch::Speed(4)
        );
        assert_eq!(
            digger_match(&first, &replacement, "minecraft:stone", &tags(&["stone"])),
            DiggerMatch::Unresolved
        );
        assert_eq!(replacement.0.len(), 1);
        let empty = replacement.replaced([]);
        assert!(empty.0.is_empty());
    }

    #[test]
    fn warm_tag_matching_does_not_allocate() {
        let digger = digger(&[(DiggerBlock::Tags("q.any_tag('stone')".into()), 12)]);
        let filters = filters(&digger);
        let stone = tags(&["stone"]);
        let before = test_alloc::thread_allocations();
        for _ in 0..16 {
            std::hint::black_box(digger_match(&digger, &filters, "minecraft:stone", &stone));
        }
        assert_eq!(test_alloc::thread_allocations() - before, 0);
    }
}
