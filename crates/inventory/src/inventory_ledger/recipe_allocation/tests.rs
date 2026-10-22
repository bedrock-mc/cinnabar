use super::*;

mod catalog;

/// Exhaustively checks all choices for a small fixture without capacity pruning or caching.
fn exhaustive(
    allocation: &RecipeAllocation<'_>,
    remaining: &mut [u32],
    cell: usize,
    missing: usize,
    in_grid: bool,
) -> bool {
    if cell == allocation.counts.len() {
        return true;
    }
    let demand = allocation.counts[cell];
    if demand == 0 {
        return exhaustive(allocation, remaining, cell + 1, missing, in_grid);
    }
    for &variant in &allocation.accepts[cell] {
        if remaining[variant] < u32::from(demand)
            || (in_grid && allocation.variants[variant].capacity < demand)
        {
            continue;
        }
        remaining[variant] -= u32::from(demand);
        let found = exhaustive(allocation, remaining, cell + 1, missing, in_grid);
        remaining[variant] += u32::from(demand);
        if found {
            return true;
        }
    }
    missing > 0 && exhaustive(allocation, remaining, cell + 1, missing - 1, in_grid)
}

#[test]
fn canonical_search_preserves_exact_assignments_and_partial_grid_choices() {
    let stack = NetworkItemStack::empty();
    let entry = &protocol::vanilla_item_registry()[0];
    let mut seed = 1u32;
    let mut next = |range| {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 16) as usize % range
    };
    for _ in 0..4096 {
        let variants: Vec<_> = (0..4)
            .map(|_| Variant {
                stack: &stack,
                entry,
                capacity: next(4) as u16 + 1,
                sources: Vec::new(),
                total: next(8) as u32,
            })
            .collect();
        let allocation = RecipeAllocation {
            variants: &variants,
            counts: (0..4).map(|_| next(5) as u16).collect(),
            accepts: (0..4)
                .map(|_| {
                    let mask = next(16);
                    (0..4)
                        .filter(|variant| mask & (1 << variant) != 0)
                        .collect()
                })
                .collect(),
        };
        for in_grid in [false, true] {
            for missing in 0..=2 {
                let mut remaining: Vec<_> = variants.iter().map(|variant| variant.total).collect();
                let expected = exhaustive(&allocation, &mut remaining, 0, missing, in_grid);
                assert_eq!(
                    allocation.assign(1, in_grid, missing).is_some(),
                    expected,
                    "demands {:?}, accepts {:?}, supply {remaining:?}, missing {missing}, in_grid {in_grid}",
                    allocation.counts,
                    allocation.accepts
                );
            }
        }
    }
}

/// Equal-count variants cannot each fill more than one count-two cell.
#[test]
fn equal_variants_reject_indivisible_shortages_within_a_small_work_budget() {
    let stack = NetworkItemStack::empty();
    let entry = &protocol::vanilla_item_registry()[0];
    let variants: Vec<_> = (0..8)
        .map(|_| Variant {
            stack: &stack,
            entry,
            capacity: 64,
            sources: Vec::new(),
            total: 3,
        })
        .collect();
    let allocation = RecipeAllocation {
        variants: &variants,
        counts: vec![2; protocol::MAX_RECIPE_INGREDIENTS],
        accepts: vec![(0..variants.len()).collect(); protocol::MAX_RECIPE_INGREDIENTS],
    };
    let mut search = Search::new(&allocation, 1, true);
    search.work_budget = Some(100_000);
    assert!(!search.assign(0, 0));
    let states = 100_000 - search.work_budget.unwrap();
    eprintln!("exact shortage visited {states} states");
    assert_eq!(states, 1);
}

/// Mixed indivisible demands also avoid enumerating equivalent variant permutations.
#[test]
fn equivalent_mixed_demand_states_stay_within_the_work_budget() {
    let stack = NetworkItemStack::empty();
    let entry = &protocol::vanilla_item_registry()[0];
    let variants: Vec<_> = (0..5)
        .map(|_| Variant {
            stack: &stack,
            entry,
            capacity: 64,
            sources: Vec::new(),
            total: 7,
        })
        .collect();
    let allocation = RecipeAllocation {
        variants: &variants,
        counts: vec![2, 2, 2, 2, 2, 4, 6, 6, 6],
        accepts: vec![(0..variants.len()).collect(); protocol::MAX_RECIPE_INGREDIENTS],
    };
    let mut search = Search::new(&allocation, 1, true);
    search.work_budget = Some(128);
    assert!(!search.assign(0, 0));
    assert!(search.work_budget.unwrap() > 0);
}

/// Shared exact-item shortages must be found before enumerating unrelated tag choices.
#[test]
fn shared_shortages_prune_before_enumerating_unrelated_variants() {
    let stack = NetworkItemStack::empty();
    let entry = &protocol::vanilla_item_registry()[0];
    for (tag_cells, missing, exact_count, possible) in
        [(7, 0, 1, false), (6, 1, 1, false), (7, 0, 2, true)]
    {
        let variants: Vec<_> = (0..36)
            .map(|index| Variant {
                stack: &stack,
                entry,
                capacity: 64,
                sources: Vec::new(),
                total: if index == 35 { exact_count } else { 1 },
            })
            .collect();
        let allocation = RecipeAllocation {
            variants: &variants,
            counts: vec![1; protocol::MAX_RECIPE_INGREDIENTS],
            accepts: (0..protocol::MAX_RECIPE_INGREDIENTS)
                .map(|cell| {
                    if cell < tag_cells {
                        (0..35).collect()
                    } else {
                        vec![35]
                    }
                })
                .collect(),
        };
        let mut search = Search::new(&allocation, 1, true);
        search.work_budget = Some(1000);
        assert_eq!(search.assign(0, missing), possible);
        assert!(search.work_budget.unwrap() >= 990);
    }
}

/// Sufficient total quantity can still leave too few variants for indivisible cell demands.
#[test]
fn indivisible_demands_are_resolved_before_unrelated_variant_choices() {
    let stack = NetworkItemStack::empty();
    let entry = &protocol::vanilla_item_registry()[0];
    let variants: Vec<_> = (0..35)
        .map(|index| Variant {
            stack: &stack,
            entry,
            capacity: 64,
            sources: Vec::new(),
            total: if index < 33 { 1 } else { 3 },
        })
        .collect();
    let counts = [1, 1, 1, 1, 1, 1, 2, 2, 2].to_vec();
    let allocation = RecipeAllocation {
        variants: &variants,
        counts,
        accepts: (0..9)
            .map(|cell| {
                if cell < 6 {
                    (0..33).collect()
                } else {
                    (33..35).collect()
                }
            })
            .collect(),
    };
    let mut search = Search::new(&allocation, 1, true);
    search.work_budget = Some(1000);
    assert!(!search.assign(0, 0));
    assert!(search.work_budget.unwrap() >= 990);
}
