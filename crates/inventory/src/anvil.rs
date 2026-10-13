//! The anvil result and its experience cost for the supported item operations.
//!
//! With a second item the anvil first tries it as a repair material: each one restores a
//! quarter of the target's durability and costs a level. Otherwise an enchanted book, or a
//! second copy of a damageable target, is combined: copies add their remaining durability plus
//! 12% for two levels, and enchantments merge. A second item that changes
//! nothing clears the result, whatever the name box holds. Renaming costs one more level.
//! The total adds both items' prior-work penalties; a rename alone stays below 40 levels.

use crate::enchantments::{applies_to, compatible, enchantment, item_slot};

/// Total cost at which survival players are refused ("Too Expensive!").
pub const ANVIL_TOO_EXPENSIVE: i32 = 40;

/// One anvil input, as read from its stack.
#[derive(Clone, Debug, Default)]
pub struct AnvilItem<'a> {
    pub identifier: &'a str,
    pub count: u16,
    pub damage: u32,
    /// Durability, for damageable items.
    pub max_damage: Option<u32>,
    pub enchantments: &'a [(i16, i16)],
    pub repair_cost: i32,
    pub custom_name: Option<&'a str>,
}

/// What taking the anvil result does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnvilOutcome {
    /// The result's new damage, when it changed.
    pub damage: Option<u32>,
    /// The result's enchantments, when they changed.
    pub enchantments: Option<Vec<(i16, i16)>>,
    /// The result's new custom name; `Some(None)` clears it.
    pub custom_name: Option<Option<String>>,
    /// The result's prior-work penalty.
    pub repair_cost: i32,
    /// Experience levels the take costs.
    pub cost: i32,
    /// Items the take consumes from the second input.
    pub material_used: u16,
    /// Survival players cannot take this result.
    pub too_expensive: bool,
}

/// Whether `material` repairs the damageable item `target`.
///
/// Provisional: follows vanilla repair materials by item name rather than each item's
/// registered repair materials.
#[must_use]
pub fn repairs_with(target: &str, material: &str) -> bool {
    let target = target.strip_prefix("minecraft:").unwrap_or(target);
    let material = material.strip_prefix("minecraft:").unwrap_or(material);
    let tier = target.split_once('_').map_or(target, |(tier, _)| tier);
    match tier {
        "wooden" => material.ends_with("_planks"),
        "stone" => matches!(material, "cobblestone" | "cobbled_deepslate" | "blackstone"),
        "iron" | "chainmail" => material == "iron_ingot",
        "golden" => material == "gold_ingot",
        "diamond" => material == "diamond",
        "netherite" => material == "netherite_ingot",
        "leather" => material == "leather",
        "copper" => material == "copper_ingot",
        _ => match target {
            "turtle_helmet" => material == "turtle_scute",
            "elytra" => material == "phantom_membrane",
            "shield" => material.ends_with("_planks"),
            "mace" => material == "breeze_rod",
            "wolf_armor" => material == "armadillo_scute",
            _ => false,
        },
    }
}

/// Whether `identifier` names an enchanted book.
fn is_enchanted_book(identifier: &str) -> bool {
    identifier.strip_prefix("minecraft:").unwrap_or(identifier) == "enchanted_book"
}

/// Merges `right`'s enchantments into `left`'s. Returns the merged list
/// and the levels it costs, or `None` when nothing changes.
fn merge_enchantments(
    left: &AnvilItem<'_>,
    right: &AnvilItem<'_>,
    from_book: bool,
) -> Option<(Vec<(i16, i16)>, i32)> {
    let slot = item_slot(left.identifier);
    let value = |enchantments: &[(i16, i16)]| -> i64 {
        enchantments
            .iter()
            .filter_map(|&(id, level)| {
                enchantment(id)
                    .map(|facts| i64::from(facts.anvil_multiplier(from_book)) * i64::from(level))
            })
            .sum()
    };
    let mut merged = left.enchantments.to_vec();
    let before = value(&merged);
    for &(id, level) in right.enchantments {
        let Some(facts) = enchantment(id) else {
            continue;
        };
        if !applies_to(id, slot) {
            continue;
        }
        match merged.iter().position(|(other, _)| *other == id) {
            Some(index) => {
                let current = &mut merged[index].1;
                if *current < facts.max_level {
                    *current = if *current == level {
                        *current + 1
                    } else {
                        (*current).max(level)
                    };
                }
            }
            None if merged.iter().all(|&(other, _)| compatible(id, other)) => {
                merged.push((id, level));
            }
            None => {}
        }
    }
    let gained = value(&merged) - before;
    (gained > 0).then(|| {
        // Enchanting a stack of more than one item is always too expensive.
        let cost = if left.count > 1 {
            ANVIL_TOO_EXPENSIVE
        } else {
            i32::try_from(gained).unwrap_or(i32::MAX)
        };
        (merged, cost)
    })
}

/// The anvil result for `left` combined with `right` and named `rename`, or `None` when the
/// inputs make nothing. `rename` is the name box's content and `current_name` the name the
/// target shows: equal text changes nothing, and empty text clears a custom name.
#[must_use]
pub fn evaluate_anvil(
    left: &AnvilItem<'_>,
    right: Option<&AnvilItem<'_>>,
    rename: &str,
    current_name: &str,
    creative: bool,
) -> Option<AnvilOutcome> {
    if left.count == 0 {
        return None;
    }
    let right = right.filter(|right| right.count > 0);
    let mut outcome = AnvilOutcome::default();
    let mut work: i32 = 0;
    if let Some(right) = right {
        let max = left.max_damage.filter(|max| *max > 0);
        if let Some(max) = max.filter(|_| repairs_with(left.identifier, right.identifier)) {
            let quarter = max / 4;
            let mut damage = left.damage;
            let mut repair = damage.min(quarter);
            while repair > 0 && outcome.material_used < right.count {
                damage -= repair;
                work += 1;
                outcome.material_used += 1;
                repair = damage.min(quarter);
            }
            outcome.damage = (work > 0).then_some(damage);
        } else {
            let book = is_enchanted_book(right.identifier) && !right.enchantments.is_empty();
            if !book && (right.identifier != left.identifier || max.is_none()) {
                return None;
            }
            if let (Some(max), false) = (max, book) {
                let remaining = u64::from(max.saturating_sub(left.damage))
                    + u64::from(max.saturating_sub(right.damage))
                    + u64::from(max) * 12 / 100;
                let damage = u64::from(max).saturating_sub(remaining) as u32;
                if damage < left.damage {
                    outcome.damage = Some(damage);
                    work = 2;
                }
            }
            if let Some((merged, cost)) = merge_enchantments(left, right, book) {
                outcome.enchantments = Some(merged);
                work = work.saturating_add(cost);
            }
            // The take consumes as many second items as the result holds; a book
            // is always used up, and it is a stack of one.
            outcome.material_used = left.count;
        }
        // A second item that changes nothing clears the result, even with a new name.
        if work <= 0 {
            return None;
        }
    }
    let renamed = if rename.is_empty() {
        left.custom_name
            .is_some()
            .then(|| outcome.custom_name = Some(None))
    } else {
        (rename != current_name).then(|| outcome.custom_name = Some(Some(rename.to_owned())))
    }
    .is_some();
    if renamed {
        work = work.saturating_add(1);
    }
    if work <= 0 {
        return None;
    }
    let rename_only = renamed && work == 1;
    let prior_work = left
        .repair_cost
        .max(0)
        .saturating_add(right.map_or(0, |right| right.repair_cost.max(0)));
    let mut total = prior_work.saturating_add(work);
    if rename_only && total >= ANVIL_TOO_EXPENSIVE {
        total = ANVIL_TOO_EXPENSIVE - 1;
    }
    outcome.cost = total;
    outcome.too_expensive = total >= ANVIL_TOO_EXPENSIVE && !creative;
    let penalty = left
        .repair_cost
        .max(right.map_or(0, |right| right.repair_cost))
        .max(0);
    outcome.repair_cost = if rename_only {
        penalty
    } else {
        penalty.saturating_mul(2).saturating_add(1)
    };
    Some(outcome)
}

#[cfg(test)]
mod tests;
