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

/// Session-owned block tag programs for every digger the item registry declares.
#[derive(Debug, Default)]
pub struct DiggerFilters(HashMap<Arc<str>, Option<pack_entity_compiler::ItemTagMolang>>);

impl DiggerFilters {
    /// Compiles each distinct tag query once, when the session's item registry is installed.
    pub fn new<'a>(entries: impl IntoIterator<Item = &'a ItemRegistryEntry>) -> Self {
        let mut filters = HashMap::new();
        let diggers = entries
            .into_iter()
            .filter_map(|entry| entry.components.as_ref()?.digger.as_deref());
        for digger in diggers {
            for entry in digger.destroy_speeds.iter() {
                if let DiggerBlock::Tags(expression) = &entry.block {
                    filters
                        .entry(Arc::clone(expression))
                        .or_insert_with(|| pack_entity_compiler::ItemTagMolang::parse(expression));
                }
            }
        }
        Self(filters)
    }

    /// Evaluates only prepared programs; a missing or unsupported query stays unresolved.
    fn evaluate(&self, source: &str, has_tag: &dyn Fn(&str) -> bool) -> Option<bool> {
        self.0.get(source)?.as_ref()?.evaluate(has_tag)
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

    fn filters(digger: &ItemDigger) -> DiggerFilters {
        let entry = ItemRegistryEntry {
            components: Some(Arc::new(protocol::ItemComponents {
                digger: Some(Arc::new(digger.clone())),
                ..Default::default()
            })),
            ..protocol::vanilla_item_registry()[0].clone()
        };
        DiggerFilters::new([&entry])
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
        let digger = digger(&[(DiggerBlock::Tags("q.any_tag('stone')".into()), 12)]);
        assert_eq!(
            digger_match(
                &digger,
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
