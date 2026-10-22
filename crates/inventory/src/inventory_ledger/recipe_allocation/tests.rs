use super::*;

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
        let mut search = Search {
            allocation: &allocation,
            demands: allocation.counts.clone(),
            in_grid: true,
            remaining: variants.iter().map(|variant| variant.total).collect(),
            assigned: vec![None; allocation.counts.len()],
            work_budget: Some(1000),
        };
        assert_eq!(search.assign(0, missing), possible);
        assert!(search.work_budget.unwrap() >= 990);
    }
}
