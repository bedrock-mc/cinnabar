//! The grindstone result: a lone item loses every enchantment except curses, and two copies of a
//! damageable item merge their durability plus a twentieth of the maximum as well. The result's
//! prior-work penalty depends only on how many curses the inputs carried.

use protocol_ids::Enchantment;

/// Prior-work penalty of a result by the number of curses the inputs carried.
const REPAIR_COST_BY_CURSES: [i32; 3] = [0, 1, 3];

/// One grindstone input, as read from its stack.
#[derive(Clone, Debug, Default)]
pub struct GrindstoneItem<'a> {
    /// Compared only with inputs from the same registry snapshot.
    pub kind: Option<crate::SessionItemKind>,
    pub count: u16,
    pub damage: u32,
    /// Durability, for damageable items.
    pub max_damage: Option<u32>,
    pub enchantments: &'a [(i16, i16)],
    pub repair_cost: i32,
    /// The stack carries user data.
    pub has_user_data: bool,
    /// Trimmed armor gives no result on its own.
    pub trimmed: bool,
    pub enchanted_book: bool,
}

/// What the grindstone makes from its inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrindstoneOutcome {
    /// The input the result copies: 0 for the top slot, 1 for the bottom one.
    pub source: usize,
    /// The result's new damage, when two copies merge.
    pub damage: Option<u32>,
    /// The curses the result keeps.
    pub enchantments: Vec<(i16, i16)>,
    pub repair_cost: i32,
    /// An enchanted book without curses becomes a fresh plain book.
    pub plain_book: bool,
}

fn is_curse(id: i16) -> bool {
    [Enchantment::Curseofbinding, Enchantment::Curseofvanishing]
        .iter()
        .any(|curse| i16::from(curse.wire_value()) == id)
}

/// The grindstone result for the top and bottom inputs, or `None` when they make nothing.
#[must_use]
pub fn evaluate_grindstone(
    top: Option<&GrindstoneItem<'_>>,
    bottom: Option<&GrindstoneItem<'_>>,
) -> Option<GrindstoneOutcome> {
    let top = top.filter(|item| item.count > 0);
    let bottom = bottom.filter(|item| item.count > 0);
    if top.or(bottom).is_none() || [top, bottom].iter().flatten().any(|item| item.count > 1) {
        return None;
    }
    // Every input's curses are kept, so two cursed copies count the same curse twice.
    let curses: Vec<(i16, i16)> = [top, bottom]
        .iter()
        .flatten()
        .flat_map(|item| item.enchantments.iter().copied())
        .filter(|(id, _)| is_curse(*id))
        .collect();
    let repair_cost = *REPAIR_COST_BY_CURSES.get(curses.len())?;
    let mut kept = Vec::with_capacity(curses.len());
    for curse in &curses {
        if !kept.iter().any(|(id, _): &(i16, i16)| *id == curse.0) {
            kept.push(*curse);
        }
    }
    if let (Some(top), Some(bottom)) = (top, bottom) {
        let max = top.max_damage.filter(|max| *max > 0)?;
        if top.kind.is_none()
            || top.kind != bottom.kind
            || bottom.max_damage.filter(|max| *max > 0).is_none()
        {
            return None;
        }
        let bonus = max / 20;
        let damage = (top.damage + bottom.damage).saturating_sub(max + bonus);
        return Some(GrindstoneOutcome {
            source: if top.has_user_data { 0 } else { 1 },
            damage: Some(damage),
            enchantments: kept,
            repair_cost,
            plain_book: false,
        });
    }
    let (source, item) = match (top, bottom) {
        (Some(top), _) => (0, top),
        (None, bottom) => (1, bottom?),
    };
    if item.trimmed {
        return None;
    }
    // An item that would come out unchanged makes nothing.
    if kept.len() == item.enchantments.len() && repair_cost == item.repair_cost {
        return None;
    }
    Some(GrindstoneOutcome {
        source,
        damage: None,
        plain_book: item.enchanted_book && kept.is_empty(),
        enchantments: kept,
        repair_cost,
    })
}

#[cfg(test)]
mod tests;
