//! The session's items and recipes as player mods read them (`cinnabar:session`): built from
//! what the client already decoded, and rebuilt only when the server resends a source.
//!
//! Known gaps, recorded rather than fixed here:
//! - Smelting: the vanilla client reads CraftingData entry types 0, 1 and 4–9 only, so furnace
//!   recipes never arrive in CraftingData.
//! - Brewing: the decoder skips CraftingData's potion and container-change vectors
//!   (`protocol` `recipes/grammar.rs`).
//! - A recipe with a Molang, complex alias, deferred or int-id ingredient is dropped whole by the
//!   decoder (`grammar.rs`'s `ingredient`), so it is missing here too.
//! - `max-stack` is the registry's negotiated component value when a server sends one, else 64.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use protocol::{
    CreativeCategory, ItemRegistryEntry, RECIPE_ANY_AUX, RecipeCatalog, RecipeOutput,
    ScreenRecipeKind,
};
use server_experience::session_data::{
    Category, Ingredient, IngredientKind, Item, ItemKey, Recipe, RecipeCategory, SessionData, Stack,
};

use super::UiRuntime;

/// The stack size assumed when the registry negotiated none.
const DEFAULT_MAX_STACK: u8 = 64;

/// What the session data is built from.
pub struct SessionInputs<'a> {
    pub creative: Option<&'a protocol::CreativeContentEvent>,
    pub registry: Option<&'a Arc<BTreeMap<i32, ItemRegistryEntry>>>,
    pub catalog: Option<&'a RecipeCatalog>,
    /// The language tables' generation, which localized names follow.
    pub language: [usize; 3],
}

/// What each half was last built from: the retained allocations of the creative content and
/// the registry, which a resend replaces, the language and the recipe catalog's revision.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ItemSources {
    creative: Option<(usize, usize)>,
    registry: Option<usize>,
    language: [usize; 3],
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct RecipeSources {
    catalog: Option<(u64, u64)>,
    registry: Option<usize>,
}

/// The last built session data, rebuilt only when a source changed. A half's revision advances
/// when its source is resent or its published data changes.
#[derive(Default)]
pub struct SessionDataCache {
    items: Option<ItemSources>,
    recipes: Option<RecipeSources>,
    data: Arc<SessionData>,
    // Pin every allocation whose address identifies the cached publication.
    creative_source: Option<protocol::CreativeContentEvent>,
    registry_source: Option<Arc<BTreeMap<i32, ItemRegistryEntry>>>,
    language_source: super::Translator,
}

impl SessionDataCache {
    /// The session data for `inputs`, with `name` localizing an identifier.
    pub fn update(
        &mut self,
        inputs: SessionInputs<'_>,
        name: &dyn Fn(&str) -> String,
    ) -> Arc<SessionData> {
        let registry = inputs
            .registry
            .map(|registry| Arc::as_ptr(registry) as usize);
        let items = ItemSources {
            creative: inputs.creative.map(|creative| {
                (
                    Arc::as_ptr(&creative.items) as *const u8 as usize,
                    Arc::as_ptr(&creative.groups) as *const u8 as usize,
                )
            }),
            registry,
            language: inputs.language,
        };
        let recipes = RecipeSources {
            catalog: inputs
                .catalog
                .map(|catalog| (catalog.session(), catalog.revision())),
            registry,
        };
        let (items_changed, recipes_changed) =
            (self.items != Some(items), self.recipes != Some(recipes));
        if !items_changed && !recipes_changed {
            return Arc::clone(&self.data);
        }
        let mut data = (*self.data).clone();
        let registry = inputs.registry.map(Arc::as_ref);
        if recipes_changed {
            let (recipes, skipped) = build_recipes(inputs.catalog, registry);
            if skipped > 0 {
                bevy::log::debug!(skipped, "session recipes skipped: unresolved output");
            }
            data.recipes = recipes.into();
            data.recipe_revision += 1;
        }
        if items_changed || recipes_changed {
            let (items, skipped) = build_items(inputs.creative, registry, name);
            if skipped > 0 {
                bevy::log::debug!(skipped, "session items skipped: unresolved or odd stacks");
            }
            let (items, tags) = with_tags(items, &data.recipes, registry);
            if items_changed
                || data.items.as_ref() != items.as_slice()
                || data.tags.as_ref() != &tags
            {
                data.item_revision += 1;
            }
            data.items = items.into();
            data.tags = Arc::new(tags);
        }
        self.creative_source = inputs.creative.cloned();
        self.registry_source = inputs.registry.cloned();
        self.items = Some(items);
        self.recipes = Some(recipes);
        self.data = Arc::new(data);
        Arc::clone(&self.data)
    }
}

/// The session data of `player_runtime`'s inventory and `runtime`'s language, through `cache`.
pub fn session_data(
    cache: &mut SessionDataCache,
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
) -> Arc<SessionData> {
    let ledger = runtime.inventory_ledger(player_runtime);
    let inputs = SessionInputs {
        creative: ledger.creative_catalog(),
        registry: ledger.negotiated_item_registry(),
        catalog: player_runtime.inventory.screen_catalog(),
        language: runtime.text_generation(),
    };
    let data = cache.update(inputs, &|identifier| {
        runtime.localized_item_name(identifier)
    });
    cache.language_source = runtime.translator();
    data
}

/// A wire stack as the session names it, as a player mod's key reports the hovered slot; `None`
/// for an empty stack or one the registry cannot name.
pub fn session_stack(
    registry: Option<&Arc<BTreeMap<i32, ItemRegistryEntry>>>,
    stack: &protocol::NetworkItemStack,
) -> Option<Stack> {
    if stack.is_empty() {
        return None;
    }
    let entry = registry?.get(&stack.network_id)?;
    let aux = u16::try_from(stack.metadata).ok()?;
    Some(Stack {
        key: ItemKey {
            identifier: entry.identifier.to_string(),
            aux,
        },
        icon: SessionData::icon_key(entry.network_id, aux),
        count: u8::try_from(stack.count).unwrap_or(u8::MAX),
    })
}

/// The creative content in its order, then every other registry item by network id. Returns
/// the items and how many entries were skipped (an unknown network id or an aux beyond u16).
fn build_items(
    creative: Option<&protocol::CreativeContentEvent>,
    registry: Option<&BTreeMap<i32, ItemRegistryEntry>>,
    name: &dyn Fn(&str) -> String,
) -> (Vec<Item>, usize) {
    let Some(registry) = registry else {
        return (Vec::new(), 0);
    };
    let item = |entry: &ItemRegistryEntry, aux: u16| Item {
        key: ItemKey {
            identifier: entry.identifier.to_string(),
            aux,
        },
        icon: SessionData::icon_key(entry.network_id, aux),
        name: name(&entry.identifier),
        group: None,
        category: None,
        max_stack: entry.negotiated_max_stack_size.unwrap_or(DEFAULT_MAX_STACK),
        tags: Vec::new(),
    };
    let (mut items, mut skipped, mut listed) = (Vec::new(), 0, BTreeSet::new());
    for creative_item in creative.iter().flat_map(|creative| creative.items.iter()) {
        let stack = &creative_item.stack;
        let (Some(entry), Ok(aux)) = (
            registry.get(&stack.network_id),
            u16::try_from(stack.metadata),
        ) else {
            skipped += 1;
            continue;
        };
        let group = creative.and_then(|creative| {
            creative
                .groups
                .get(usize::try_from(creative_item.group).ok()?)
        });
        listed.insert(stack.network_id);
        items.push(Item {
            group: group
                .filter(|group| !group.name.is_empty())
                .map(|group| group.name.to_string()),
            category: group.and_then(|group| category(group.category)),
            ..item(entry, aux)
        });
    }
    items.extend(
        registry
            .values()
            .filter(|entry| entry.network_id != 0 && !listed.contains(&entry.network_id))
            .map(|entry| item(entry, 0)),
    );
    (items, skipped)
}

fn category(category: CreativeCategory) -> Option<Category> {
    Some(match category {
        CreativeCategory::Construction => Category::Construction,
        CreativeCategory::Nature => Category::Nature,
        CreativeCategory::Equipment => Category::Equipment,
        CreativeCategory::Items => Category::Items,
        CreativeCategory::CommandOnly => Category::CommandOnly,
        CreativeCategory::Unknown(_) => return None,
    })
}

/// Tag members: the registry's tags, then the vanilla table's for every other tag a recipe
/// names. Each item carries the tags it is a member of.
fn with_tags(
    mut items: Vec<Item>,
    recipes: &[Recipe],
    registry: Option<&BTreeMap<i32, ItemRegistryEntry>>,
) -> (Vec<Item>, BTreeMap<String, Vec<String>>) {
    let mut tags: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in registry.into_iter().flat_map(BTreeMap::values) {
        for tag in entry.item_tags.iter() {
            let members = tags.entry(tag.to_string()).or_default();
            if !members.iter().any(|member| **member == *entry.identifier) {
                members.push(entry.identifier.to_string());
            }
        }
    }
    let named: BTreeSet<&str> = recipes
        .iter()
        .flat_map(|recipe| recipe.ingredients.iter().flatten())
        .filter_map(|ingredient| match &ingredient.kind {
            IngredientKind::Tag(tag) => Some(tag.as_str()),
            _ => None,
        })
        .collect();
    let identifiers: BTreeSet<&str> = items
        .iter()
        .map(|item| item.key.identifier.as_str())
        .collect();
    for tag in named {
        if tags.contains_key(tag) {
            continue;
        }
        let members: Vec<String> = identifiers
            .iter()
            .filter(|identifier| protocol::vanilla_tag_contains(tag, identifier) == Some(true))
            .map(|identifier| (*identifier).to_owned())
            .collect();
        if !members.is_empty() {
            tags.insert(tag.to_owned(), members);
        }
    }
    let mut of_item: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (tag, members) in &tags {
        for member in members {
            of_item.entry(member).or_default().push(tag.clone());
        }
    }
    for item in &mut items {
        if let Some(item_tags) = of_item.get(item.key.identifier.as_str()) {
            item.tags.clone_from(item_tags);
        }
    }
    (items, tags)
}

/// Crafting recipes in catalog order, then the screens' by kind. Returns the recipes and how
/// many were skipped because the registry does not name their output.
fn build_recipes(
    catalog: Option<&RecipeCatalog>,
    registry: Option<&BTreeMap<i32, ItemRegistryEntry>>,
) -> (Vec<Recipe>, usize) {
    let (Some(catalog), Some(registry)) = (catalog, registry) else {
        return (Vec::new(), 0);
    };
    let output = |output: &RecipeOutput| {
        let entry = registry.get(&output.network_id)?;
        Some(Stack {
            key: ItemKey {
                identifier: entry.identifier.to_string(),
                aux: output.aux,
            },
            icon: SessionData::icon_key(output.network_id, output.aux),
            count: output.count,
        })
    };
    let ingredient = |name: &str, tag: bool, aux: u16, count: u8| Ingredient {
        kind: if tag {
            IngredientKind::Tag(name.to_owned())
        } else if aux == RECIPE_ANY_AUX {
            IngredientKind::AnyAux(name.to_owned())
        } else {
            IngredientKind::Item(ItemKey {
                identifier: name.to_owned(),
                aux,
            })
        },
        count,
    };
    let (mut recipes, mut skipped) = (Vec::new(), 0);
    for handle in catalog.crafting_recipes() {
        let recipe = handle.recipe();
        let Some(stack) = output(&recipe.output()) else {
            skipped += 1;
            continue;
        };
        let shapeless = recipe.is_shapeless();
        let (width, height) = if shapeless {
            (0, 0)
        } else {
            recipe.dimensions()
        };
        recipes.push(Recipe {
            network_id: handle.network_id(),
            category: RecipeCategory::Crafting,
            shapeless,
            width,
            height,
            ingredients: handle
                .ingredient_views()
                .into_iter()
                .map(|cell| cell.map(|view| ingredient(&view.name, view.tag, view.aux, view.count)))
                .collect(),
            outputs: vec![stack],
        });
    }
    for (kind, category) in [
        (ScreenRecipeKind::Stonecutter, RecipeCategory::Stonecutter),
        (ScreenRecipeKind::Cartography, RecipeCategory::Cartography),
        (
            ScreenRecipeKind::SmithingTransform,
            RecipeCategory::SmithingTransform,
        ),
        (ScreenRecipeKind::SmithingTrim, RecipeCategory::SmithingTrim),
    ] {
        for recipe in catalog.screen_recipes(kind) {
            let outputs = match &recipe.output {
                Some(stack) => match output(stack) {
                    Some(stack) => vec![stack],
                    None => {
                        skipped += 1;
                        continue;
                    }
                },
                None => Vec::new(),
            };
            recipes.push(Recipe {
                network_id: recipe.id,
                category,
                shapeless: false,
                width: 0,
                height: 0,
                ingredients: recipe
                    .ingredients
                    .iter()
                    .map(|input| Some(ingredient(&input.name, input.tag, input.aux, 1)))
                    .collect(),
                outputs,
            });
        }
    }
    (recipes, skipped)
}

#[cfg(test)]
mod tests;
