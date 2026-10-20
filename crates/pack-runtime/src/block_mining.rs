//! Resolves the held item's override of a custom block's mining value.

use protocol::{CustomBlockMining, CustomMiningItem};

/// Selects the first matching item override, or the block's base value.
/// An unsupported tag expression returns `None` rather than predicting an early break.
pub fn custom_mining_seconds(
    mining: &CustomBlockMining,
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
                identifier.is_some()
                    && pack_entity_compiler::ItemTagMolang::parse(expression)?.evaluate(has_tag)?
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
        let tag = |value: &str| value == "test:fast";
        assert_eq!(
            custom_mining_seconds(&mining, Some("test:tool"), 2, &tag),
            Some(0.5)
        );
        assert_eq!(
            custom_mining_seconds(&mining, Some("test:tool"), 3, &tag),
            Some(1.0)
        );
        assert_eq!(custom_mining_seconds(&mining, None, 2, &tag), Some(4.0));
        assert_eq!(
            custom_mining_seconds(&mining, Some("test:other"), 2, &|_| false),
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
        assert_eq!(
            custom_mining_seconds(&mining, Some("test:tool"), 0, &|tag| tag == "test:tools"),
            Some(0.25)
        );
        assert_eq!(
            custom_mining_seconds(&mining, Some("test:other"), 0, &|_| false),
            Some(-1.0)
        );
        assert_eq!(
            custom_mining_seconds(&mining, Some("test:tool"), 0, &|_| true),
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
            assert_eq!(
                custom_mining_seconds(&empty, Some("test:tool"), 0, &|_| true),
                Some(2.0)
            );
        }
    }
}
