//! Mining components retained from server block definitions.

use super::{Arc, Nbt};

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

/// Reads network values and accepts the authored numeric spelling used by older servers.
pub(super) fn component(component: Option<&Nbt>) -> (Option<Arc<CustomBlockMining>>, usize) {
    let Some(component) = component else {
        return (None, 0);
    };
    let mut skipped = 0;
    let seconds = match component {
        Nbt::Byte(0) => -1.0,
        Nbt::Byte(1) => 0.0,
        Nbt::Compound(_) => component
            .field("value")
            .or_else(|| component.field("seconds_to_destroy"))
            .and_then(Nbt::number)
            .unwrap_or(0.0) as f32,
        _ => component.number().unwrap_or(0.0) as f32,
    };
    if !seconds.is_finite() {
        return (None, 1);
    }
    let speeds = component
        .list("item_specific_speeds")
        .iter()
        .filter_map(|entry| {
            let item = entry.field("item").and_then(item);
            let speed = entry
                .field("destroy_speed")
                .and_then(Nbt::number)
                .unwrap_or(0.0) as f32;
            match item {
                Some(item) if speed.is_finite() => Some(CustomMiningSpeed {
                    item,
                    destroy_speed: speed,
                }),
                _ => {
                    skipped += 1;
                    None
                }
            }
        })
        .collect();
    (
        Some(Arc::new(CustomBlockMining {
            seconds_to_destroy: seconds,
            item_specific_speeds: speeds,
        })),
        skipped,
    )
}

/// Keeps wire descriptor precedence and the wildcard metadata distinct from an exact value.
fn item(value: &Nbt) -> Option<CustomMiningItem> {
    if let Some(expression) = value
        .field("Tags")
        .or_else(|| value.field("tags"))
        .and_then(Nbt::as_str)
    {
        return Some(CustomMiningItem::Tags(expression.into()));
    }
    if let Some(tag) = value.field("ItemTag").and_then(Nbt::as_str) {
        return Some(CustomMiningItem::Tag(tag.into()));
    }
    let name = value
        .as_str()
        .or_else(|| value.field("Name").and_then(Nbt::as_str))?;
    if name.is_empty() {
        return None;
    }
    let metadata = match value.field("Aux") {
        Some(Nbt::Short(value)) if *value != i64::from(crate::RECIPE_ANY_AUX) => Some(
            u16::from_ne_bytes(i16::try_from(*value).ok()?.to_ne_bytes()),
        ),
        _ => None,
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
    fn custom_mining_keeps_network_and_authored_values() {
        for (key, value) in [("value", 2.0), ("seconds_to_destroy", 0.5)] {
            let (mining, skipped) = component(Some(&compound(vec![(key, Nbt::Float(value))])));
            assert_eq!(mining.unwrap().seconds_to_destroy, value as f32);
            assert_eq!(skipped, 0);
        }
        let (mining, _) = component(Some(&Nbt::Float(3.0)));
        assert_eq!(mining.unwrap().seconds_to_destroy, 3.0);
    }

    #[test]
    fn custom_mining_default_and_disabled_are_resolved() {
        assert_eq!(
            component(None).0.unwrap_or_default().seconds_to_destroy,
            0.0
        );
        for value in [
            Nbt::Byte(0),
            Nbt::Byte(-1),
            Nbt::Float(-1.0),
            compound(vec![("value", Nbt::Float(-4.0))]),
        ] {
            assert!(component(Some(&value)).0.unwrap().seconds_to_destroy < 0.0);
        }
        assert_eq!(
            component(Some(&Nbt::Byte(1))).0.unwrap().seconds_to_destroy,
            0.0
        );
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
                entry(compound(vec![(
                    "Tags",
                    Nbt::String("q.any_tag('test:tools')".into()),
                )])),
                entry(compound(vec![])),
            ]),
        )]);
        let (mining, skipped) = component(Some(&value));
        let mining = mining.unwrap();
        assert_eq!(skipped, 1);
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
    fn custom_mining_skips_nonfinite_values() {
        assert_eq!(component(Some(&Nbt::Float(f64::NAN))), (None, 1));
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
