//! `minecraft:digger` destroy speeds that a server-defined item declares.

use std::sync::Arc;

use crate::nbt_tree::Nbt;

/// One block selector, in the forms a block descriptor accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiggerBlock {
    /// Matches every state of one block identifier.
    Name(Arc<str>),
    /// Matches one identifier only in the listed states, which the client does not compare yet.
    NameWithStates(Arc<str>),
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
                    DiggerBlock::Name(text)
                    | DiggerBlock::NameWithStates(text)
                    | DiggerBlock::Tags(text) => text.len(),
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

/// A tag query takes precedence over a name; states narrow a name.
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
    if let Some(tags) = text(block.field("tags")) {
        return Some(DiggerBlock::Tags(tags));
    }
    let name = text(block.field("name"))?;
    let has_states = match block.field("states") {
        Some(Nbt::Compound(states)) => !states.is_empty(),
        Some(Nbt::String(states)) => !states.is_empty(),
        _ => false,
    };
    Some(if has_states {
        DiggerBlock::NameWithStates(name)
    } else {
        DiggerBlock::Name(name)
    })
}
