//! Resolves the held item's override of a custom block's mining value.

use pack_entity_compiler::BlockMolang;
use protocol::{CustomBlock, CustomBlockMining, CustomBlocks, CustomMiningItem};
use std::{collections::HashMap, sync::Arc};

type MiningCondition<'a> = (Option<BlockMolang>, &'a Arc<CustomBlockMining>);

/// Resolves every admitted state's mining component; admission already bounds the state count.
pub(crate) fn resolve(blocks: &mut CustomBlocks) {
    for block in Arc::make_mut(&mut blocks.blocks) {
        if block.visual.materialization_disabled {
            continue;
        }
        let conditions: Vec<_> = block
            .visual
            .permutations
            .iter()
            .filter_map(|permutation| {
                let mining = permutation.physical.mining.as_ref()?;
                Some((BlockMolang::parse(&permutation.condition), mining))
            })
            .collect();
        if conditions.is_empty() {
            continue;
        }
        let mut unevaluated = false;
        block.state_mining = (0..block.state_count)
            .map(|state| evaluate_state(block, state, &conditions, &mut unevaluated))
            .collect();
        blocks.skipped += usize::from(unevaluated);
    }
}

/// Applies ordered overrides whose condition holds; one that cannot be evaluated reads as zero,
/// as the client treats a failed or empty condition.
fn evaluate_state(
    block: &CustomBlock,
    state: u32,
    conditions: &[MiningCondition<'_>],
    unevaluated: &mut bool,
) -> Arc<CustomBlockMining> {
    let values = block.state_values(state).unwrap_or_default();
    let mut mining = &block.mining;
    for (condition, override_mining) in conditions {
        let value = condition
            .as_ref()
            .and_then(|condition| crate::block_condition::evaluate(condition, block, &values));
        *unevaluated |= value.is_none();
        if value.is_some_and(BlockMolang::holds) {
            mining = override_mining;
        }
    }
    Arc::clone(mining)
}

/// Session-owned item tag programs; a failed compilation matches no item.
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
                .chain(block.state_mining.iter().map(AsRef::as_ref))
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

    /// Evaluates prepared programs; a missing or unsupported filter matches no item.
    fn matches(&self, source: &str, has_tag: &dyn Fn(&str) -> bool) -> bool {
        self.0
            .get(source)
            .and_then(Option::as_ref)
            .and_then(|filter| filter.evaluate(has_tag))
            .unwrap_or(false)
    }
}

/// Selects the first matching item override's value, or the block's base value.
pub fn custom_mining_seconds(
    mining: &CustomBlockMining,
    filters: &CustomMiningFilters,
    identifier: Option<&str>,
    metadata: u32,
    has_tag: &dyn Fn(&str) -> bool,
) -> f32 {
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
                identifier.is_some() && filters.matches(expression, has_tag)
            }
        };
        if matches {
            return speed.destroy_speed;
        }
    }
    mining.seconds_to_destroy
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::CustomMiningSpeed;

    #[test]
    fn custom_mining_unprepared_and_unsupported_filters_match_no_item_without_allocating() {
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
            assert_eq!(evaluate(), 4.0);
            let before = test_alloc::thread_allocations();
            for _ in 0..16 {
                assert_eq!(evaluate(), 4.0);
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
            assert_eq!(evaluate(), 0.5);
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
            0.5
        );
        assert_eq!(
            custom_mining_seconds(&mining, &filters, Some("test:tool"), 3, &tag),
            1.0
        );
        assert_eq!(custom_mining_seconds(&mining, &filters, None, 2, &tag), 4.0);
        assert_eq!(
            custom_mining_seconds(&mining, &filters, Some("test:other"), 2, &|_| false),
            4.0
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
            0.25
        );
        assert_eq!(
            custom_mining_seconds(&mining, &filters, Some("test:other"), 0, &|_| false),
            -1.0
        );
        assert_eq!(
            custom_mining_seconds(&mining, &filters, Some("test:tool"), 0, &|_| true),
            -1.0
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
                2.0
            );
        }
    }
}
