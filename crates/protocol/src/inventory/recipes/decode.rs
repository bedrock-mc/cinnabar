use super::{
    brewing::BrewingMix,
    budget::{Credits, MAX_UPDATE_BYTES},
    crafting::RecipeOutput,
    grammar,
    identity::{RecipeIdentity, RecipeUnlockingContext},
    model::{Batch, Ingredient, MAX_INGREDIENTS, Recipe, RecipeUpdate, Record},
    reader::{Reader, Refusal},
    screen::{
        BrewingRecipe, MAX_SCREEN_RECIPES, MultiRecipe, ScreenIngredient, ScreenRecipe,
        ScreenRecipes,
    },
};
use std::{mem::size_of, sync::Arc};

pub(super) fn decode(body: &[u8]) -> Result<RecipeUpdate, super::super::InventoryPacketError> {
    decode_with_credits(body, &Credits::shared())
}

/// Admits the complete update under the supplied shared allocation owner.
pub(super) fn decode_with_credits(
    body: &[u8],
    credits: &Arc<Credits>,
) -> Result<RecipeUpdate, super::super::InventoryPacketError> {
    let parse = || -> Result<RecipeUpdate, Refusal> {
        let mut reader = Reader::new(body)?;
        let mut retained_names = 0usize;
        let mut screen_count = 0usize;
        let mut screen_names = 0usize;
        let mut multi_count = 0usize;
        let mut brewing_count = 0usize;
        let mut screen_brewing_count = 0usize;
        let mut recipe_count = 0usize;
        let mut identities_count = 0usize;
        let (clear, _) = grammar::walk(
            &mut reader,
            |_, candidate, identity| {
                recipe_count += 1;
                identities_count += usize::from(identity.is_some());
                retained_names += candidate.map_or(0, |candidate| {
                    candidate
                        .ingredients
                        .iter()
                        .filter(|item| item.count > 0)
                        .count()
                });
                Ok(())
            },
            |item| {
                match item {
                    grammar::ScreenItem::Recipe(record) if screen_count < MAX_SCREEN_RECIPES => {
                        screen_count += 1;
                        screen_names += record.len;
                    }
                    grammar::ScreenItem::Multi { .. } => multi_count += 1,
                    grammar::ScreenItem::Brewing(_)
                        if screen_brewing_count < MAX_SCREEN_RECIPES =>
                    {
                        screen_brewing_count += 1;
                    }
                    _ => {}
                }
                Ok(())
            },
            |_| {
                brewing_count += 1;
                Ok(())
            },
        )?;
        // Conservative string allocation allowance includes per-string allocator
        // metadata for each retained ingredient name, Vec capacity, the batch
        // and its Arc/permit metadata.
        let retained_names = retained_names
            .checked_add(screen_names)
            .ok_or(Refusal::Policy)?;
        let charge = recipe_count
            .checked_mul(size_of::<Record>())
            .and_then(|n| n.checked_add(retained_names.checked_mul(128)?))
            .and_then(|n| n.checked_add(screen_names.checked_mul(size_of::<ScreenIngredient>())?))
            .and_then(|n| n.checked_add(screen_count.checked_mul(size_of::<ScreenRecipe>())?))
            .and_then(|n| n.checked_add(multi_count.checked_mul(size_of::<MultiRecipe>())?))
            .and_then(|n| n.checked_add(brewing_count.checked_mul(size_of::<BrewingMix>())?))
            .and_then(|n| {
                n.checked_add(screen_brewing_count.checked_mul(size_of::<BrewingRecipe>())?)
            })
            .and_then(|n| {
                n.checked_add(identities_count.checked_mul(size_of::<RecipeIdentity>() + 64)?)
            })
            .and_then(|n| n.checked_add(reader.string_bytes()))
            .and_then(|n| n.checked_add(512))
            .ok_or(Refusal::Policy)?;
        if charge > MAX_UPDATE_BYTES {
            return Err(Refusal::Policy);
        }
        let permit = credits.reserve(charge).ok_or(Refusal::Policy)?;
        let mut records = Vec::new();
        records
            .try_reserve_exact(recipe_count)
            .map_err(|_| Refusal::Policy)?;
        let mut reader = Reader::new(body)?;
        let mut brewing = Vec::new();
        brewing
            .try_reserve_exact(brewing_count)
            .map_err(|_| Refusal::Policy)?;
        let mut identities = Vec::new();
        identities
            .try_reserve_exact(identities_count)
            .map_err(|_| Refusal::Policy)?;
        let mut screens = ScreenRecipes::default();
        screens
            .recipes
            .try_reserve_exact(screen_count)
            .map_err(|_| Refusal::Policy)?;
        screens
            .multi
            .try_reserve_exact(multi_count)
            .map_err(|_| Refusal::Policy)?;
        screens
            .brewing
            .try_reserve_exact(screen_brewing_count)
            .map_err(|_| Refusal::Policy)?;
        grammar::walk(
            &mut reader,
            |id, candidate, identity| {
                let identity_index = if let Some(identity) = identity {
                    let index = identities.len();
                    identities.push(RecipeIdentity {
                        name: owned_text(identity.name)?,
                        tag: owned_text(identity.tag)?,
                        context: RecipeUnlockingContext::from_index(identity.context),
                        network_id: id,
                    });
                    Some(index)
                } else {
                    None
                };
                let recipe = if let Some(candidate) = candidate {
                    let mut ingredients: [Option<Ingredient>; MAX_INGREDIENTS] =
                        std::array::from_fn(|_| None);
                    for (index, item) in candidate.ingredients.into_iter().enumerate() {
                        if item.count == 0 {
                            continue;
                        }
                        let mut name = String::new();
                        name.try_reserve_exact(item.name.len())
                            .map_err(|_| Refusal::Policy)?;
                        name.push_str(item.name);
                        ingredients[index] = Some(Ingredient {
                            name,
                            tag: item.tag,
                            aux: item.aux as u16,
                            count: item.count as u8,
                        });
                    }
                    Some(Recipe {
                        width: candidate.width,
                        height: candidate.height,
                        shapeless: candidate.shapeless,
                        mirror: candidate.mirror,
                        priority: candidate.priority,
                        ingredients,
                        output: candidate.output,
                    })
                } else {
                    None
                };
                records.push(Record {
                    id,
                    recipe,
                    identity_index,
                });
                Ok(())
            },
            |item| {
                match item {
                    grammar::ScreenItem::Skipped => screens.skipped += 1,
                    grammar::ScreenItem::Brewing(recipe) => {
                        if screens.brewing.len() < MAX_SCREEN_RECIPES {
                            screens.brewing.push(recipe);
                        }
                    }
                    grammar::ScreenItem::Multi { uuid, id } => {
                        screens.multi.push(MultiRecipe { uuid, id })
                    }
                    grammar::ScreenItem::Recipe(record) => {
                        if screens.recipes.len() < MAX_SCREEN_RECIPES {
                            let mut ingredients = Vec::new();
                            ingredients
                                .try_reserve_exact(record.len)
                                .map_err(|_| Refusal::Policy)?;
                            ingredients.extend(record.ingredients[..record.len].iter().map(
                                |item| ScreenIngredient {
                                    name: Arc::from(item.name),
                                    tag: item.tag,
                                    aux: item.aux as u16,
                                },
                            ));
                            screens.recipes.push(ScreenRecipe {
                                id: record.id,
                                kind: record.kind,
                                ingredients,
                                output: record.output.map(|output| RecipeOutput {
                                    network_id: output.id,
                                    aux: output.aux,
                                    count: output.count,
                                    block_runtime_id: output.block,
                                    empty_envelope: output.empty_envelope,
                                }),
                            });
                        }
                    }
                }
                Ok(())
            },
            |mix| {
                brewing.push(mix);
                Ok(())
            },
        )?;
        records.sort_unstable_by_key(|record| record.id);
        // Ambiguous duplicates are tombstones, independent of arrival order.
        let mut duplicates = Vec::new();
        let mut output = 0;
        let mut input = 0;
        while input < records.len() {
            let id = records[input].id;
            let mut end = input + 1;
            while end < records.len() && records[end].id == id {
                end += 1;
            }
            if end != input + 1 {
                records[input].recipe = None;
                duplicates.push(id);
            }
            records.swap(output, input);
            output += 1;
            input = end;
        }
        records.truncate(output);
        screens.recipes.sort_unstable_by_key(|recipe| recipe.id);
        screens.multi.sort_unstable_by_key(|recipe| recipe.id);
        // Duplicates across all families invalidate every view of that ID.
        let mut duplicate = 0;
        screens.recipes.retain(|recipe| {
            while duplicate < duplicates.len() && duplicates[duplicate] < recipe.id {
                duplicate += 1;
            }
            duplicate == duplicates.len() || duplicates[duplicate] != recipe.id
        });
        duplicate = 0;
        screens.multi.retain(|recipe| {
            while duplicate < duplicates.len() && duplicates[duplicate] < recipe.id {
                duplicate += 1;
            }
            duplicate == duplicates.len() || duplicates[duplicate] != recipe.id
        });
        Ok(RecipeUpdate {
            batch: Some(Arc::new(Batch {
                records,
                identities,
                screen: screens,
                brewing,
                clear,
                _permit: permit,
            })),
        })
    };
    match parse() {
        Ok(update) => Ok(update),
        Err(Refusal::Policy) => Ok(RecipeUpdate::unavailable()),
        Err(Refusal::Wire) => Err(super::super::InventoryPacketError::MalformedWire),
    }
}

/// Copies text only after the complete decoded batch has reserved its allocation credit.
fn owned_text(value: &str) -> Result<String, Refusal> {
    let mut text = String::new();
    text.try_reserve_exact(value.len())
        .map_err(|_| Refusal::Policy)?;
    text.push_str(value);
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_updates_across_replacement_sessions_share_credit() {
        let owner = Credits::isolated(1024);
        let body = [0; 12];
        let old_session = decode_with_credits(&body, &owner).unwrap();
        let old_queue = old_session.clone();
        drop(old_session);
        let new_session = decode_with_credits(&body, &owner).unwrap();
        assert!(!new_session.is_unavailable());
        assert!(decode_with_credits(&body, &owner).unwrap().is_unavailable());
        assert_eq!(owner.used(), 1024);
        drop(old_queue);
        assert_eq!(owner.used(), 512);
        drop(new_session);
        assert_eq!(owner.used(), 0);
        assert!(decode_with_credits(&[1], &owner).is_err());
        assert_eq!(owner.used(), 0);
    }
}
