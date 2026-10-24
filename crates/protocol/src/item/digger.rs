//! `minecraft:digger` destroy speeds that a server-defined item declares.

use std::sync::Arc;

use crate::{CustomStateValue, nbt_tree::Nbt};

/// Named values that narrow a block descriptor; absent values leave it unresolved.
pub type DiggerStates = Arc<[(Arc<str>, CustomStateValue)]>;

/// One block selector, in the forms a block descriptor accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiggerBlock {
    /// Matches every state of one block identifier.
    Name(Arc<str>),
    /// Matches one identifier and all listed states; malformed states remain unresolved.
    NameWithStates {
        name: Arc<str>,
        states: Option<DiggerStates>,
    },
    /// A Molang tag query evaluated against the block's tags.
    Tags(Arc<str>),
}

/// One ordered destroy-speed entry; the first matching entry decides the speed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiggerSpeed {
    pub block: DiggerBlock,
    pub speed: i32,
}

/// The ordered speeds and whether an Efficiency enchantment adds to a matched speed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemDigger {
    pub use_efficiency: bool,
    pub destroy_speeds: Arc<[DiggerSpeed]>,
}

impl ItemDigger {
    /// Counts owned text and entries for the item registry's admission budget.
    pub(super) fn retained_bytes(&self) -> usize {
        self.destroy_speeds
            .iter()
            .fold(std::mem::size_of::<Self>(), |bytes, entry| {
                let text = match &entry.block {
                    DiggerBlock::Name(text) | DiggerBlock::Tags(text) => text.len(),
                    DiggerBlock::NameWithStates { name, states } => {
                        name.len()
                            + states
                                .as_deref()
                                .unwrap_or_default()
                                .iter()
                                .map(|(name, value)| {
                                    std::mem::size_of::<(Arc<str>, CustomStateValue)>()
                                        + name.len()
                                        + match value {
                                            CustomStateValue::String(text) => text.len(),
                                            _ => 0,
                                        }
                                })
                                .sum::<usize>()
                    }
                };
                bytes
                    .saturating_add(std::mem::size_of::<DiggerSpeed>())
                    .saturating_add(text)
            })
    }
}

/// Reads a digger declaration, skipping entries without a usable block selector or integer speed.
pub(super) fn parse_digger(component: Option<&Nbt>) -> Option<ItemDigger> {
    let component = component.filter(|component| matches!(component, Nbt::Compound(_)))?;
    let destroy_speeds = component
        .list("destroy_speeds")
        .iter()
        .filter_map(|entry| {
            Some(DiggerSpeed {
                block: digger_block(entry.field("block")?)?,
                speed: entry.field("speed")?.int32()?,
            })
        })
        .collect();
    Some(ItemDigger {
        use_efficiency: component
            .field("use_efficiency")
            .and_then(Nbt::number)
            .is_some_and(|value| value != 0.0),
        destroy_speeds,
    })
}

/// A serialized name takes precedence over tags; nonempty states narrow the name.
fn digger_block(block: &Nbt) -> Option<DiggerBlock> {
    let text = |value: Option<&Nbt>| {
        value
            .and_then(Nbt::as_str)
            .filter(|text| !text.is_empty())
            .map(Arc::from)
    };
    if let Some(name) = text(Some(block)) {
        return Some(DiggerBlock::Name(name));
    }
    let Some(raw_name) = block.field("name") else {
        return text(block.field("tags")).map(DiggerBlock::Tags);
    };
    let name = text(Some(raw_name))?;
    let has_states = match block.field("states") {
        Some(Nbt::Compound(states)) => !states.is_empty(),
        Some(Nbt::List(states)) => !states.is_empty(),
        Some(Nbt::String(states)) => !states.is_empty(),
        _ => false,
    };
    Some(if has_states {
        DiggerBlock::NameWithStates {
            name,
            states: parse_states(block.field("states")?),
        }
    } else {
        DiggerBlock::Name(name)
    })
}

/// Reads serialized typed states without broadening an unsupported restriction.
fn parse_states(states: &Nbt) -> Option<DiggerStates> {
    let Nbt::List(states) = states else {
        return None;
    };
    states
        .iter()
        .map(|state| {
            let name = state.field("state")?.as_str()?.into();
            let value = match state.field("type")?.int32()? {
                1 => CustomStateValue::String(state.field("value")?.as_str()?.into()),
                2 => CustomStateValue::Int(i64::from(state.field("value")?.int32()?)),
                _ => return None,
            };
            Some((name, value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a descriptor with a name and one serialized typed state.
    fn restricted_block() -> Nbt {
        Nbt::Compound(vec![
            ("name".into(), Nbt::String("minecraft:oak_log".into())),
            (
                "states".into(),
                Nbt::List(vec![Nbt::Compound(vec![
                    ("state".into(), Nbt::String("pillar_axis".into())),
                    ("type".into(), Nbt::Int(1)),
                    ("value".into(), Nbt::String("x".into())),
                ])]),
            ),
        ])
    }

    #[test]
    fn serialized_state_list_does_not_become_an_unrestricted_name() {
        assert_eq!(
            digger_block(&restricted_block()),
            Some(DiggerBlock::NameWithStates {
                name: "minecraft:oak_log".into(),
                states: Some(Arc::from([(
                    "pillar_axis".into(),
                    CustomStateValue::String("x".into())
                )]))
            })
        );
    }

    #[test]
    fn serialized_name_takes_precedence_over_tags() {
        let block = Nbt::Compound(vec![
            ("name".into(), Nbt::String("minecraft:stone".into())),
            ("tags".into(), Nbt::String("q.any_tag('wood')".into())),
        ]);
        assert_eq!(
            digger_block(&block),
            Some(DiggerBlock::Name("minecraft:stone".into()))
        );
    }

    #[test]
    fn integer_states_retain_their_value_and_empty_lists_do_not_restrict() {
        let make = |states| {
            Nbt::Compound(vec![
                ("name".into(), Nbt::String("test:block".into())),
                ("states".into(), states),
            ])
        };
        let block = make(Nbt::List(vec![Nbt::Compound(vec![
            ("state".into(), Nbt::String("test:enabled".into())),
            ("type".into(), Nbt::Int(2)),
            ("value".into(), Nbt::Int(1)),
        ])]));
        assert_eq!(
            digger_block(&block),
            Some(DiggerBlock::NameWithStates {
                name: "test:block".into(),
                states: Some(Arc::from([(
                    "test:enabled".into(),
                    CustomStateValue::Int(1)
                )])),
            })
        );
        assert_eq!(
            digger_block(&make(Nbt::List(vec![]))),
            Some(DiggerBlock::Name("test:block".into()))
        );
        assert_eq!(
            digger_block(&make(Nbt::List(vec![Nbt::Other]))),
            Some(DiggerBlock::NameWithStates {
                name: "test:block".into(),
                states: None
            })
        );
    }
}
