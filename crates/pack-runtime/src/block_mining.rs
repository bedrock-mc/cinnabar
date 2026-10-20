//! Resolves the held item's override of a custom block's mining value.

use protocol::{CustomBlockMining, CustomMiningItem};
use std::{collections::HashMap, sync::Arc};

/// Session-owned item tag programs, including failed compilations that must stay unresolved.
#[derive(Debug, Default)]
pub struct CustomMiningFilters(HashMap<Arc<str>, Option<pack_entity_compiler::ItemTagMolang>>);

impl CustomMiningFilters {
    /// Compiles each distinct filter before the session publishes its block registries.
    pub fn new(blocks: &protocol::CustomBlocks) -> Self {
        Self::from_profiles(blocks.blocks.iter().flat_map(|block| {
            std::iter::once(block.mining.as_ref())
                .chain(
                    block
                        .visual
                        .permutations
                        .iter()
                        .filter_map(|permutation| permutation.physical.mining.as_deref()),
                )
                .chain(
                    block
                        .state_physics
                        .iter()
                        .filter_map(|state| state.mining.as_deref()),
                )
        }))
    }

    /// Shares one compilation for repeated expressions across base and effective profiles.
    fn from_profiles<'a>(profiles: impl IntoIterator<Item = &'a CustomBlockMining>) -> Self {
        let mut filters = HashMap::new();
        for profile in profiles {
            for speed in profile.item_specific_speeds.iter() {
                if let CustomMiningItem::Tags(expression) = &speed.item {
                    filters
                        .entry(Arc::clone(expression))
                        .or_insert_with(|| pack_entity_compiler::ItemTagMolang::parse(expression));
                }
            }
        }
        Self(filters)
    }

    /// Evaluates only prepared programs; a missing or unsupported filter stays unresolved.
    fn evaluate(&self, source: &str, has_tag: &dyn Fn(&str) -> bool) -> Option<bool> {
        self.0.get(source)?.as_ref()?.evaluate(has_tag)
    }
}

/// Selects the first matching item override, or the block's base value.
/// An unsupported tag expression returns `None` rather than predicting an early break.
pub fn custom_mining_seconds(
    mining: &CustomBlockMining,
    filters: &CustomMiningFilters,
    identifier: Option<&str>,
    metadata: u32,
    has_tag: &dyn Fn(&str) -> bool,
) -> Option<f32> {
    for speed in mining.item_specific_speeds.iter() {
        let matches = match &speed.item {
            CustomMiningItem::Item {
                name,
                metadata: required,
            } => {
                identifier == Some(name.as_ref())
                    && required.is_none_or(|required| u32::from(required) == metadata)
            }
            CustomMiningItem::Tag(tag) => identifier.is_some() && has_tag(tag),
            CustomMiningItem::Tags(expression) => {
                identifier.is_some() && filters.evaluate(expression, has_tag)?
            }
        };
        if matches {
            return Some(speed.destroy_speed);
        }
    }
    Some(mining.seconds_to_destroy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::CustomMiningSpeed;

    #[test]
    fn custom_mining_unprepared_and_unsupported_filters_stay_unresolved_without_allocating() {
        let mining = CustomBlockMining {
            seconds_to_destroy: 4.0,
            item_specific_speeds: [CustomMiningSpeed {
                item: CustomMiningItem::Tags("return q.any_tag('test:tools');".into()),
                destroy_speed: 0.5,
            }]
            .into(),
        };
        for filters in [
            CustomMiningFilters::default(),
            CustomMiningFilters::from_profiles([&mining]),
        ] {
            let evaluate =
                || custom_mining_seconds(&mining, &filters, Some("test:tool"), 0, &|_| true);
            assert_eq!(evaluate(), None);
            let before = test_alloc::thread_allocations();
            for _ in 0..16 {
                assert_eq!(evaluate(), None);
            }
            assert_eq!(test_alloc::thread_allocations() - before, 0);
        }
    }

    #[test]
    fn custom_mining_warm_tag_evaluation_does_not_allocate() {
        for expression in [
            "q.any_tag('test:tools')",
            "math.abs(q.any_tag('test:tools'))",
            "math.max(q.any_tag('test:tools'), 0)",
            "math.clamp(math.abs(-q.any_tag('test:tools')), 0, 1)",
        ] {
            let mining = CustomBlockMining {
                seconds_to_destroy: 4.0,
                item_specific_speeds: [CustomMiningSpeed {
                    item: CustomMiningItem::Tags(expression.into()),
                    destroy_speed: 0.5,
                }]
                .into(),
            };
            let filters = CustomMiningFilters::from_profiles([&mining]);
            let evaluate =
                || custom_mining_seconds(&mining, &filters, Some("test:tool"), 0, &|_| true);
            assert_eq!(evaluate(), Some(0.5));
            let before = test_alloc::thread_allocations();
            for _ in 0..16 {
                std::hint::black_box(evaluate());
            }
            let allocated = test_alloc::thread_allocations() - before;
            assert_eq!(
                allocated, 0,
                "unchanged item filters must reuse their preparation: {expression}"
            );
        }
    }

    #[test]
    fn custom_mining_item_specific_value_replaces_base_and_first_match_wins() {
        let mining = CustomBlockMining {
            seconds_to_destroy: 4.0,
            item_specific_speeds: [
                CustomMiningSpeed {
                    item: CustomMiningItem::Item {
                        name: "test:tool".into(),
                        metadata: Some(2),
                    },
                    destroy_speed: 0.5,
                },
                CustomMiningSpeed {
                    item: CustomMiningItem::Tag("test:fast".into()),
                    destroy_speed: 1.0,
                },
            ]
            .into(),
        };
        let filters = CustomMiningFilters::from_profiles([&mining]);
        let tag = |value: &str| value == "test:fast";
        assert_eq!(
            custom_mining_seconds(&mining, &filters, Some("test:tool"), 2, &tag),
            Some(0.5)
        );
        assert_eq!(
            custom_mining_seconds(&mining, &filters, Some("test:tool"), 3, &tag),
            Some(1.0)
        );
        assert_eq!(
            custom_mining_seconds(&mining, &filters, None, 2, &tag),
            Some(4.0)
        );
        assert_eq!(
            custom_mining_seconds(&mining, &filters, Some("test:other"), 2, &|_| false),
            Some(4.0)
        );
    }

    #[test]
    fn custom_mining_tag_expressions_and_disabled_item_overrides() {
        let mining = CustomBlockMining {
            seconds_to_destroy: -1.0,
            item_specific_speeds: [CustomMiningSpeed {
                item: CustomMiningItem::Tags(
                    "q.any_tag('test:tools') && !q.all_tags('test:tools', 'test:slow')".into(),
                ),
                destroy_speed: 0.25,
            }]
            .into(),
        };
        let filters = CustomMiningFilters::from_profiles([&mining]);
        assert_eq!(
            custom_mining_seconds(&mining, &filters, Some("test:tool"), 0, &|tag| tag
                == "test:tools"),
            Some(0.25)
        );
        assert_eq!(
            custom_mining_seconds(&mining, &filters, Some("test:other"), 0, &|_| false),
            Some(-1.0)
        );
        assert_eq!(
            custom_mining_seconds(&mining, &filters, Some("test:tool"), 0, &|_| true),
            Some(-1.0)
        );
        for query in ["q.any_tag()", "q.all_tags()"] {
            let empty = CustomBlockMining {
                seconds_to_destroy: 2.0,
                item_specific_speeds: [CustomMiningSpeed {
                    item: CustomMiningItem::Tags(query.into()),
                    destroy_speed: 0.0,
                }]
                .into(),
            };
            let filters = CustomMiningFilters::from_profiles([&empty]);
            assert_eq!(
                custom_mining_seconds(&empty, &filters, Some("test:tool"), 0, &|_| true),
                Some(2.0)
            );
        }
    }
}
