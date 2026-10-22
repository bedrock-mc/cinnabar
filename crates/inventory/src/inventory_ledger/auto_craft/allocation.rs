//! Capacitated ingredient matching over the fixed recipe and source-cell bounds.

use super::SOURCE_CELLS;
use protocol::MAX_RECIPE_INGREDIENTS;

type Amounts = [[u16; SOURCE_CELLS]; MAX_RECIPE_INGREDIENTS];
type Accepted = [[bool; SOURCE_CELLS]; MAX_RECIPE_INGREDIENTS];

/// Preserves free first choices, then augments through prior assignments until all demands fit.
pub(super) fn plan(
    demands: &[u16],
    counts: &[u16; SOURCE_CELLS],
    accepts: &Accepted,
) -> Option<Amounts> {
    if demands.len() > MAX_RECIPE_INGREDIENTS {
        return None;
    }
    let mut assigned = [[0; SOURCE_CELLS]; MAX_RECIPE_INGREDIENTS];
    let mut used = [0; SOURCE_CELLS];
    for (ingredient, demand) in demands.iter().copied().enumerate() {
        let mut needed = demand;
        for slot in 0..SOURCE_CELLS {
            if accepts[ingredient][slot] {
                let amount = needed.min(counts[slot] - used[slot]);
                assigned[ingredient][slot] += amount;
                used[slot] += amount;
                needed -= amount;
            }
        }
        while needed > 0 {
            needed -= augment(
                ingredient,
                needed,
                counts,
                accepts,
                &mut assigned,
                &mut used,
            )?;
        }
    }
    Some(assigned)
}

/// Finds a shortest residual path to spare inventory capacity without recursion or unit expansion.
fn augment(
    start: usize,
    needed: u16,
    counts: &[u16; SOURCE_CELLS],
    accepts: &Accepted,
    assigned: &mut Amounts,
    used: &mut [u16; SOURCE_CELLS],
) -> Option<u16> {
    let mut ingredient_parent = [None; MAX_RECIPE_INGREDIENTS];
    let mut slot_parent = [None; SOURCE_CELLS];
    let mut queue = [0; MAX_RECIPE_INGREDIENTS];
    queue[0] = start;
    let (mut head, mut tail) = (0, 1);
    let end = 'search: loop {
        if head == tail {
            return None;
        }
        let ingredient = queue[head];
        head += 1;
        for slot in 0..SOURCE_CELLS {
            if !accepts[ingredient][slot] || slot_parent[slot].is_some() {
                continue;
            }
            slot_parent[slot] = Some(ingredient);
            if used[slot] < counts[slot] {
                break 'search slot;
            }
            for previous in 0..start {
                if assigned[previous][slot] > 0 && ingredient_parent[previous].is_none() {
                    ingredient_parent[previous] = Some(slot);
                    queue[tail] = previous;
                    tail += 1;
                }
            }
        }
    };
    let mut amount = needed.min(counts[end] - used[end]);
    let mut slot = end;
    loop {
        let ingredient = slot_parent[slot]?;
        if ingredient == start {
            break;
        }
        slot = ingredient_parent[ingredient]?;
        amount = amount.min(assigned[ingredient][slot]);
    }
    let mut slot = end;
    loop {
        let ingredient = slot_parent[slot]?;
        assigned[ingredient][slot] += amount;
        used[slot] += amount;
        if ingredient == start {
            break;
        }
        slot = ingredient_parent[ingredient]?;
        assigned[ingredient][slot] -= amount;
        used[slot] -= amount;
    }
    Some(amount)
}
