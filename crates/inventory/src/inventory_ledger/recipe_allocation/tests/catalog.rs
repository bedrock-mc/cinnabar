//! Deterministic search budgets over the installed, manifest-pinned recipe shapes.

use super::*;
use serde_json::Value;

/// Expands a sample crafting recipe into the same ingredient rows used by allocation.
fn layout(recipe: &Value) -> Option<Vec<Option<RecipeIngredientView>>> {
    let ingredient = |value: &Value| {
        let tag = value.get("tag").and_then(Value::as_str);
        let name = tag.or_else(|| value.get("item").and_then(Value::as_str))?;
        let (name, aux) = name
            .rsplit_once(':')
            .and_then(|(name, aux)| aux.parse::<u16>().ok().map(|aux| (name, aux)))
            .unwrap_or((
                name,
                value.get("data").and_then(Value::as_u64).unwrap_or(0) as u16,
            ));
        Some(RecipeIngredientView {
            name: name.into(),
            tag: tag.is_some(),
            aux,
            count: value.get("count").and_then(Value::as_u64).unwrap_or(1) as u8,
        })
    };
    if let Some(shaped) = recipe.get("minecraft:recipe_shaped") {
        let key = shaped.get("key")?.as_object()?;
        Some(
            shaped
                .get("pattern")?
                .as_array()?
                .iter()
                .flat_map(|row| {
                    row.as_str().unwrap().chars().map(|symbol| {
                        if symbol == ' ' {
                            None
                        } else {
                            Some(ingredient(&key[&symbol.to_string()]).expect("sample ingredient"))
                        }
                    })
                })
                .collect(),
        )
    } else {
        let shapeless = recipe.get("minecraft:recipe_shapeless")?;
        Some(
            shapeless
                .get("ingredients")?
                .as_array()?
                .iter()
                .map(|value| Some(ingredient(value).expect("sample ingredient")))
                .collect(),
        )
    }
}

#[test]
fn installed_recipe_catalog_stays_within_the_search_work_budget() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source: Value =
        serde_json::from_str(include_str!("../../../../../../assets/vanilla-source.json")).unwrap();
    let directory = root
        .join(source["cache_dir"].as_str().unwrap())
        .join("behavior_pack/recipes");
    let Ok(files) = std::fs::read_dir(&directory) else {
        eprintln!(
            "skipping installed_recipe_catalog_stays_within_the_search_work_budget: missing sample recipe fixture {}",
            directory.display()
        );
        return;
    };
    let registry = protocol::vanilla_item_registry();
    let mut recipes = 0;
    let mut worst = 0;
    for file in files {
        let path = file.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let recipe: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let Some(layout) = layout(&recipe) else {
            continue;
        };
        assert!(
            layout.len() <= protocol::MAX_RECIPE_INGREDIENTS,
            "{}",
            path.display()
        );
        recipes += 1;
        let mut accepted = Vec::new();
        for entry in registry.iter() {
            for view in layout.iter().flatten() {
                let metadata = if view.aux == 0x7fff {
                    0
                } else {
                    u32::from(view.aux)
                };
                if crate::ingredient_accepts(view, &entry.identifier, metadata, &entry.item_tags)
                    && !accepted.iter().any(
                        |(known, stack): &(&ItemRegistryEntry, NetworkItemStack)| {
                            known.network_id == entry.network_id && stack.metadata == metadata
                        },
                    )
                {
                    accepted.push((
                        entry,
                        NetworkItemStack {
                            metadata,
                            ..NetworkItemStack::empty()
                        },
                    ));
                }
            }
        }
        // Distinct item data can give every accepted item several stack-incompatible variants.
        for supply in [1, 3, 7, 64] {
            let variants: Vec<_> = accepted
                .iter()
                .cycle()
                .take(
                    (usize::from(protocol::PLAYER_INVENTORY_SLOTS)
                        + protocol::MAX_RECIPE_INGREDIENTS)
                        .min(accepted.len() * 8),
                )
                .map(|(entry, stack)| Variant {
                    stack,
                    entry,
                    capacity: 64,
                    sources: Vec::new(),
                    total: supply,
                })
                .collect();
            let inputs = RecipeInputs { variants };
            let allocation = inputs.allocation(&layout);
            for crafts in [1, 2, 3] {
                for missing in [0, 1] {
                    let mut search = Search::new(&allocation, crafts, true);
                    search.work_budget = Some(128);
                    search.assign(0, missing);
                    worst = worst.max(128 - search.work_budget.unwrap());
                }
            }
        }
    }
    assert!(
        recipes > 0,
        "the installed fixture must contain crafting recipes"
    );
    eprintln!(
        "sample catalog: {recipes} crafting recipes, maximum {worst} search states (budget 128)"
    );
}
