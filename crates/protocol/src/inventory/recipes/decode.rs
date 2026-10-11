use super::{
    budget::{Credits, MAX_UPDATE_BYTES},
    crafting::RecipeOutput,
    grammar,
    model::{Batch, Ingredient, MAX_INGREDIENTS, Recipe, RecipeUpdate, Record},
    reader::{Reader, Refusal},
    screen::{MAX_SCREEN_RECIPES, MultiRecipe, ScreenIngredient, ScreenRecipe, ScreenRecipes},
};
use std::{mem::size_of, sync::Arc};

pub(super) fn decode(body: &[u8]) -> Result<RecipeUpdate, super::super::InventoryPacketError> {
    decode_with_credits(body, &Credits::shared())
}

fn decode_with_credits(
    body: &[u8],
    credits: &Arc<Credits>,
) -> Result<RecipeUpdate, super::super::InventoryPacketError> {
    let parse = || -> Result<RecipeUpdate, Refusal> {
        let mut reader = Reader::new(body)?;
        let mut retained_names = 0usize;
        let (clear, total) = grammar::walk(
            &mut reader,
            |_, candidate| {
                retained_names += candidate.map_or(0, |candidate| {
                    candidate
                        .ingredients
                        .iter()
                        .filter(|item| item.count > 0)
                        .count()
                });
                Ok(())
            },
            |_| Ok(()),
        )?;
        // Conservative string allocation allowance includes per-string allocator
        // metadata for each retained ingredient name, Vec capacity, the batch
        // and its Arc/permit metadata.
        let charge = total
            .checked_mul(size_of::<Record>())
            .and_then(|n| n.checked_add(retained_names.checked_mul(128)?))
            .and_then(|n| n.checked_add(reader.string_bytes()))
            .and_then(|n| n.checked_add(512))
            .ok_or(Refusal::Policy)?;
        if charge > MAX_UPDATE_BYTES {
            return Err(Refusal::Policy);
        }
        let permit = credits.reserve(charge).ok_or(Refusal::Policy)?;
        let mut records = Vec::new();
        records
            .try_reserve_exact(total)
            .map_err(|_| Refusal::Policy)?;
        let mut reader = Reader::new(body)?;
        let mut screens = ScreenRecipes::default();
        grammar::walk(
            &mut reader,
            |id, candidate| {
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
                records.push(Record { id, recipe });
                Ok(())
            },
            |item| {
                match item {
                    grammar::ScreenItem::Skipped => screens.skipped += 1,
                    grammar::ScreenItem::Multi { uuid, id } => {
                        screens.multi.push(MultiRecipe { uuid, id })
                    }
                    grammar::ScreenItem::Recipe(record) => {
                        if screens.recipes.len() < MAX_SCREEN_RECIPES {
                            screens.recipes.push(ScreenRecipe {
                                id: record.id,
                                kind: record.kind,
                                ingredients: record.ingredients[..record.len]
                                    .iter()
                                    .map(|item| ScreenIngredient {
                                        name: Arc::from(item.name),
                                        tag: item.tag,
                                        aux: item.aux as u16,
                                    })
                                    .collect(),
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
                clear,
                _permit: permit,
            })),
            screen: (!screens.recipes.is_empty()
                || !screens.multi.is_empty()
                || screens.skipped > 0)
                .then(|| Arc::new(screens)),
        })
    };
    match parse() {
        Ok(update) => Ok(update),
        Err(Refusal::Policy) => Ok(RecipeUpdate::unavailable()),
        Err(Refusal::Wire) => Err(super::super::InventoryPacketError::MalformedWire),
    }
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
