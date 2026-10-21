//! Mining components retained from server block definitions.

use super::{Arc, Nbt};

/// The mining component carried by a block's component set.
pub(super) const COMPONENT: &str = "minecraft:destructible_by_mining";

/// Whether omitted component data belongs entirely to mining rather than physical placement.
pub(super) fn only_component(components: Option<&Nbt>) -> bool {
    matches!(components, Some(Nbt::Compound(fields)) if !fields.is_empty()
        && fields.iter().all(|(name, _)| name == COMPONENT))
}

/// A custom block's base destroy value and ordered item overrides.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomBlockMining {
    /// Zero breaks immediately; negative values disable mining.
    pub seconds_to_destroy: f32,
    pub item_specific_speeds: Arc<[CustomMiningSpeed]>,
}

/// An item override replaces the base destroy value, rather than multiplying it.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomMiningSpeed {
    pub item: CustomMiningItem,
    pub destroy_speed: f32,
}

/// Item descriptors carried by a mining speed entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomMiningItem {
    Item {
        name: Arc<str>,
        metadata: Option<u16>,
    },
    Tag(Arc<str>),
    Tags(Arc<str>),
}

/// Reads the component as the client's network intake does: only a compound is a component,
/// `value` and `destroy_speed` count only as float tags, and other values read as zero.
/// Returns the mining values and the count of malformed item entries, which never match.
pub(super) fn component(component: Option<&Nbt>) -> (Option<Arc<CustomBlockMining>>, usize) {
    let Some(component @ Nbt::Compound(_)) = component else {
        return (None, 0);
    };
    let mut skipped = 0;
    let speeds = component
        .list("item_specific_speeds")
        .iter()
        .filter(|entry| matches!(entry, Nbt::Compound(_)))
        .filter_map(|entry| {
            let Some(item) = entry.field("item").and_then(item) else {
                skipped += 1;
                return None;
            };
            Some(CustomMiningSpeed {
                item,
                destroy_speed: float(entry.field("destroy_speed")),
            })
        })
        .collect();
    (
        Some(Arc::new(CustomBlockMining {
            seconds_to_destroy: float(component.field("value")),
            item_specific_speeds: speeds,
        })),
        skipped,
    )
}

/// A float tag's value, zero for any other tag; a non-finite value never completes a destroy,
/// which the indestructible sentinel reproduces.
fn float(value: Option<&Nbt>) -> f32 {
    match value {
        Some(Nbt::Float(value)) if value.is_finite() => *value as f32,
        Some(Nbt::Float(_)) => -1.0,
        _ => 0.0,
    }
}

/// Reads an item descriptor in the client's precedence: a Molang `Tags` expression when it
/// carries a `MolangVersion`, then an `ItemTag`, then a `Name` with optional `Aux`.
fn item(value: &Nbt) -> Option<CustomMiningItem> {
    if value.field("MolangVersion").is_some()
        && let Some(expression) = value.field("Tags").and_then(Nbt::as_str)
    {
        return Some(CustomMiningItem::Tags(expression.into()));
    }
    if let Some(tag) = value.field("ItemTag").and_then(Nbt::as_str) {
        return Some(CustomMiningItem::Tag(tag.into()));
    }
    let name = value.field("Name").and_then(Nbt::as_str)?;
    if name.is_empty() {
        return None;
    }
    let metadata = match value.field("Aux") {
        None => None,
        Some(Nbt::Short(value)) if *value == i64::from(crate::RECIPE_ANY_AUX) => None,
        Some(Nbt::Short(value)) => Some(u16::from_ne_bytes(
            i16::try_from(*value).ok()?.to_ne_bytes(),
        )),
        Some(_) => Some(0),
    };
    Some(CustomMiningItem::Item {
        name: name.into(),
        metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one network component without depending on a live server.
    fn compound(fields: Vec<(&str, Nbt)>) -> Nbt {
        Nbt::Compound(
            fields
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }

    #[test]
    fn custom_mining_reads_only_float_values() {
        let (mining, skipped) = component(Some(&compound(vec![("value", Nbt::Float(2.0))])));
        assert_eq!(mining.unwrap().seconds_to_destroy, 2.0);
        assert_eq!(skipped, 0);
        for other in [
            compound(vec![("value", Nbt::Int(3))]),
            compound(vec![("seconds_to_destroy", Nbt::Float(0.5))]),
        ] {
            assert_eq!(component(Some(&other)).0.unwrap().seconds_to_destroy, 0.0);
        }
    }

    #[test]
    fn custom_mining_scalar_components_are_absent() {
        assert!(component(None).0.is_none());
        for scalar in [Nbt::Byte(0), Nbt::Byte(1), Nbt::Float(3.0)] {
            assert_eq!(component(Some(&scalar)), (None, 0));
        }
        let disabled = compound(vec![("value", Nbt::Float(-4.0))]);
        assert!(component(Some(&disabled)).0.unwrap().seconds_to_destroy < 0.0);
    }

    #[test]
    fn custom_mining_preserves_item_order_metadata_and_tags() {
        let entry = |item| compound(vec![("item", item), ("destroy_speed", Nbt::Float(0.25))]);
        let value = compound(vec![(
            "item_specific_speeds",
            Nbt::List(vec![
                entry(compound(vec![
                    ("Name", Nbt::String("test:tool".into())),
                    ("Aux", Nbt::Short(7)),
                ])),
                entry(compound(vec![(
                    "ItemTag",
                    Nbt::String("test:tools".into()),
                )])),
                entry(compound(vec![
                    ("Tags", Nbt::String("q.any_tag('test:tools')".into())),
                    ("MolangVersion", Nbt::Int(12)),
                ])),
                entry(compound(vec![])),
                Nbt::String("test:not_a_compound".into()),
            ]),
        )]);
        let (mining, skipped) = component(Some(&value));
        let mining = mining.unwrap();
        assert_eq!(skipped, 1, "only the malformed compound entry is counted");
        assert_eq!(mining.item_specific_speeds.len(), 3);
        assert_eq!(
            mining.item_specific_speeds[0].item,
            CustomMiningItem::Item {
                name: "test:tool".into(),
                metadata: Some(7)
            }
        );
        assert_eq!(
            mining.item_specific_speeds[1].item,
            CustomMiningItem::Tag("test:tools".into())
        );
        assert!(matches!(
            &mining.item_specific_speeds[2].item,
            CustomMiningItem::Tags(_)
        ));
    }

    #[test]
    fn custom_mining_tags_without_a_molang_version_fall_back_to_the_name() {
        let descriptor = compound(vec![
            ("Tags", Nbt::String("q.any_tag('test:tools')".into())),
            ("Name", Nbt::String("test:tool".into())),
            ("Aux", Nbt::Int(3)),
        ]);
        assert_eq!(
            item(&descriptor),
            Some(CustomMiningItem::Item {
                name: "test:tool".into(),
                metadata: Some(0)
            })
        );
    }

    #[test]
    fn custom_mining_nonfinite_values_never_complete() {
        let (mining, skipped) = component(Some(&compound(vec![("value", Nbt::Float(f64::NAN))])));
        assert!(mining.unwrap().seconds_to_destroy < 0.0);
        assert_eq!(skipped, 0);
    }

    #[test]
    fn custom_mining_item_metadata_preserves_signed_short_bits() {
        assert_eq!(
            item(&compound(vec![
                ("Name", Nbt::String("test:tool".into())),
                ("Aux", Nbt::Short(-1))
            ])),
            Some(CustomMiningItem::Item {
                name: "test:tool".into(),
                metadata: Some(u16::MAX)
            })
        );
    }
}
