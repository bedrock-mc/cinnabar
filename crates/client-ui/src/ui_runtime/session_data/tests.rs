use super::*;
use ::protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
};
use ::protocol::{CreativeCategory, CreativeGroup, CreativeItem, ItemRegistryVersion};

fn entry(network_id: i32, identifier: &str, tags: &[&str]) -> ItemRegistryEntry {
    ItemRegistryEntry {
        identifier: identifier.into(),
        network_id,
        component_based: !tags.is_empty(),
        version: ItemRegistryVersion::None,
        components: None,
        component_digest: [0; 32],
        negotiated_max_stack_size: (network_id == 3).then_some(16),
        canonical_empty_component_data: true,
        item_tags: tags.iter().map(|tag| Arc::from(*tag)).collect(),
    }
}

fn registry() -> Arc<BTreeMap<i32, ItemRegistryEntry>> {
    Arc::new(
        [
            entry(1, "minecraft:stone", &[]),
            entry(2, "minecraft:oak_planks", &["minecraft:planks"]),
            entry(3, "minecraft:ender_pearl", &[]),
            entry(4, "minecraft:wool", &[]),
            entry(5, "minecraft:bed", &[]),
            entry(6, "minecraft:stone_slab", &[]),
            entry(7, "minecraft:stick", &[]),
        ]
        .into_iter()
        .map(|entry| (entry.network_id, entry))
        .collect(),
    )
}

fn stack(network_id: i32, metadata: u32) -> ::protocol::NetworkItemStack {
    ::protocol::NetworkItemStack {
        network_id,
        metadata,
        count: 1,
        ..::protocol::NetworkItemStack::empty()
    }
}

/// Creative content listing wool (aux 14, then 0) in the construction group, then stone.
fn creative() -> ::protocol::CreativeContentEvent {
    let groups = vec![
        CreativeGroup {
            category: CreativeCategory::Construction,
            name: "itemGroup.name.wool".into(),
            icon: None,
        },
        CreativeGroup {
            category: CreativeCategory::Nature,
            name: "".into(),
            icon: None,
        },
    ];
    let items = vec![
        CreativeItem {
            creative_network_id: 1,
            stack: stack(4, 14),
            group: 0,
        },
        CreativeItem {
            creative_network_id: 2,
            stack: stack(4, 0),
            group: 0,
        },
        CreativeItem {
            creative_network_id: 3,
            stack: stack(1, 0),
            group: 1,
        },
    ];
    ::protocol::CreativeContentEvent {
        groups: groups.into(),
        items: items.into(),
        skipped: 0,
    }
}

fn descriptor(key: &str, name: &str, aux: i32) -> IngredientFixture {
    IngredientFixture {
        descriptor: vec![(key.into(), name.into())],
        aux,
        count: 1,
    }
}

fn result(id: i32) -> Vec<OutputFixture> {
    vec![OutputFixture::new(id, 1)]
}

/// A bed (shaped 2x2: planks tag, empty, any wool, empty), sticks (shapeless planks), a recipe
/// whose output network id the registry lacks, and a stonecutter slab.
fn catalog() -> RecipeCatalog {
    let bed = RecipeFixture {
        net_id: 1,
        ..RecipeFixture::shaped(
            "test:bed",
            2,
            2,
            vec![
                descriptor("item_tag", "minecraft:planks", 0),
                IngredientFixture::empty(),
                descriptor(
                    "name",
                    "minecraft:wool",
                    i32::from(::protocol::RECIPE_ANY_AUX),
                ),
                IngredientFixture::empty(),
            ],
            result(5),
        )
    };
    let shapeless = |net_id, recipe_id, ingredient, output, tag: &str| RecipeFixture {
        tag: tag.into(),
        net_id,
        ..RecipeFixture::shapeless(recipe_id, vec![ingredient], result(output))
    };
    let update = CraftingDataFixture::clearing(vec![
        bed,
        shapeless(
            10,
            "test:sticks",
            descriptor("name", "minecraft:oak_planks", 0),
            7,
            "crafting_table",
        ),
        shapeless(
            11,
            "test:unknown",
            descriptor("name", "minecraft:stone", 0),
            99,
            "crafting_table",
        ),
        shapeless(
            12,
            "test:slab",
            descriptor("name", "minecraft:stone", 0),
            6,
            "stonecutter",
        ),
    ])
    .update();
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    catalog
}

fn name(identifier: &str, _aux: u16) -> String {
    format!("Name of {identifier}")
}

fn build(creative: &::protocol::CreativeContentEvent, catalog: &RecipeCatalog) -> SessionData {
    let registry = registry();
    let mut cache = SessionDataCache::default();
    let inputs = SessionInputs {
        creative: Some(creative),
        registry: Some(&registry),
        catalog: Some(catalog),
        language: [0; 3],
    };
    Arc::unwrap_or_clone(cache.update(inputs, &name))
}

fn key(identifier: &str, aux: u16) -> ItemKey {
    ItemKey {
        identifier: identifier.into(),
        aux,
    }
}

#[test]
fn items_follow_creative_order_then_the_rest_of_the_registry() {
    let data = build(&creative(), &catalog());
    let keys: Vec<_> = data.items.iter().map(|item| item.key.clone()).collect();
    assert_eq!(
        keys,
        [
            key("minecraft:wool", 14),
            key("minecraft:wool", 0),
            key("minecraft:stone", 0),
            key("minecraft:oak_planks", 0),
            key("minecraft:ender_pearl", 0),
            key("minecraft:bed", 0),
            key("minecraft:stone_slab", 0),
            key("minecraft:stick", 0),
        ]
    );
    let wool = &data.items[0];
    assert_eq!(wool.name, "Name of minecraft:wool");
    assert_eq!(wool.icon, SessionData::icon_key(4, 14));
    assert_eq!(wool.group.as_deref(), Some("itemGroup.name.wool"));
    assert_eq!(wool.category, Some(Category::Construction));
    let stone = &data.items[2];
    assert_eq!(
        (stone.group.as_deref(), stone.category),
        (None, Some(Category::Nature))
    );
    let pearl = data.lookup(&key("minecraft:ender_pearl", 0)).unwrap();
    assert_eq!((pearl.max_stack, pearl.category), (16, None));
    assert_eq!(data.items[3].max_stack, 64);
}

#[test]
fn tags_come_from_the_registry_then_the_vanilla_table() {
    let data = build(&creative(), &catalog());
    assert_eq!(
        data.tag_members("minecraft:planks"),
        ["minecraft:oak_planks"]
    );
    let planks = data.lookup(&key("minecraft:oak_planks", 0)).unwrap();
    assert_eq!(planks.tags, ["minecraft:planks"]);
    assert!(data.tag_members("minecraft:unreferenced").is_empty());
}

#[test]
fn recipes_carry_shape_ingredient_kinds_and_categories() {
    let data = build(&creative(), &catalog());
    let bed = data
        .recipes
        .iter()
        .find(|recipe| recipe.outputs[0].key.identifier == "minecraft:bed")
        .unwrap();
    assert_eq!(
        (bed.category, bed.shapeless, bed.width, bed.height),
        (RecipeCategory::Crafting, false, 2, 2)
    );
    assert_eq!(
        bed.ingredients,
        [
            Some(Ingredient {
                kind: IngredientKind::Tag("minecraft:planks".into()),
                count: 1
            }),
            None,
            Some(Ingredient {
                kind: IngredientKind::AnyAux("minecraft:wool".into()),
                count: 1
            }),
            None,
        ]
    );
    assert_eq!(bed.outputs[0].icon, SessionData::icon_key(5, 0));
    let sticks = data
        .recipes
        .iter()
        .find(|recipe| recipe.outputs[0].key.identifier == "minecraft:stick")
        .unwrap();
    assert!(sticks.shapeless);
    assert_eq!((sticks.width, sticks.height), (0, 0));
    assert_eq!(
        sticks.ingredients,
        [Some(Ingredient {
            kind: IngredientKind::Item(key("minecraft:oak_planks", 0)),
            count: 1
        })]
    );
    let slab = data
        .recipes
        .iter()
        .find(|recipe| recipe.category == RecipeCategory::Stonecutter)
        .unwrap();
    assert_eq!(slab.outputs[0].key, key("minecraft:stone_slab", 0));
    assert_eq!(
        slab.ingredients,
        [Some(Ingredient {
            kind: IngredientKind::Item(key("minecraft:stone", 0)),
            count: 1
        })]
    );
    // The recipe whose output the registry does not know is skipped.
    assert_eq!(data.recipes.len(), 3);
}

#[test]
fn furnace_recipes_list_under_their_station() {
    let smelt = |net_id, tag: &str| RecipeFixture {
        tag: tag.into(),
        net_id,
        ..RecipeFixture::shapeless(
            "test:smelt",
            vec![descriptor("name", "minecraft:stone", 0)],
            result(6),
        )
    };
    let update = CraftingDataFixture::clearing(vec![
        smelt(1, "furnace"),
        smelt(2, "blast_furnace"),
        smelt(3, "smoker"),
    ])
    .update();
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    let data = build(&creative(), &catalog);
    let categories: Vec<_> = data.recipes.iter().map(|recipe| recipe.category).collect();
    assert_eq!(
        categories,
        [
            RecipeCategory::Furnace,
            RecipeCategory::BlastFurnace,
            RecipeCategory::Smoker
        ]
    );
    for recipe in data.recipes.iter() {
        assert_eq!(recipe.outputs[0].key, key("minecraft:stone_slab", 0));
        assert_eq!(
            recipe.ingredients,
            [Some(Ingredient {
                kind: IngredientKind::Item(key("minecraft:stone", 0)),
                count: 1
            })]
        );
    }
}

#[test]
fn revisions_move_only_when_their_inputs_are_resent() {
    let registry = registry();
    let (creative, catalog) = (creative(), catalog());
    let mut cache = SessionDataCache::default();
    let inputs = |creative, registry, language| SessionInputs {
        creative,
        registry,
        catalog: Some(&catalog),
        language,
    };
    let first = cache.update(inputs(Some(&creative), Some(&registry), [0; 3]), &name);
    let again = cache.update(inputs(Some(&creative), Some(&registry), [0; 3]), &name);
    assert!(Arc::ptr_eq(&first, &again));
    assert!(first.item_revision > 0 && first.recipe_revision > 0);
    let resent = creative.clone();
    let same_items = cache.update(inputs(Some(&resent), Some(&registry), [0; 3]), &name);
    assert!(
        Arc::ptr_eq(&first, &same_items),
        "a clone shares the retained items"
    );
    let republished = ::protocol::CreativeContentEvent {
        items: creative.items.iter().cloned().collect(),
        ..creative.clone()
    };
    let moved = cache.update(inputs(Some(&republished), Some(&registry), [0; 3]), &name);
    assert_eq!(moved.item_revision, first.item_revision + 1);
    assert_eq!(moved.recipe_revision, first.recipe_revision);
    let language = cache.update(
        inputs(Some(&republished), Some(&registry), [1, 0, 0]),
        &name,
    );
    assert_eq!(language.item_revision, moved.item_revision + 1);
    let registry = Arc::new((*registry).clone());
    let rebound = cache.update(
        inputs(Some(&republished), Some(&registry), [1, 0, 0]),
        &name,
    );
    assert_eq!(rebound.item_revision, language.item_revision + 1);
    assert_eq!(rebound.recipe_revision, language.recipe_revision + 1);
}

#[test]
fn without_a_registry_nothing_resolves() {
    let catalog = catalog();
    let mut cache = SessionDataCache::default();
    let data = cache.update(
        SessionInputs {
            creative: Some(&creative()),
            registry: None,
            catalog: Some(&catalog),
            language: [0; 3],
        },
        &name,
    );
    assert!(data.items.is_empty() && data.recipes.is_empty());
}

#[test]
fn a_recipe_tag_the_registry_lacks_reads_the_vanilla_table() {
    let mut untagged = (*registry()).clone();
    untagged.insert(2, entry(2, "minecraft:oak_planks", &[]));
    let untagged = Arc::new(untagged);
    let (creative, catalog) = (creative(), catalog());
    let data = SessionDataCache::default().update(
        SessionInputs {
            creative: Some(&creative),
            registry: Some(&untagged),
            catalog: Some(&catalog),
            language: [0; 3],
        },
        &name,
    );
    assert_eq!(
        data.tag_members("minecraft:planks"),
        ["minecraft:oak_planks"]
    );
    let planks = data.lookup(&key("minecraft:oak_planks", 0)).unwrap();
    assert_eq!(planks.tags, ["minecraft:planks"]);
}

#[test]
fn a_hovered_stack_reads_as_its_session_stack() {
    let registry = registry();
    let wool = ::protocol::NetworkItemStack {
        count: 12,
        ..stack(4, 14)
    };
    assert_eq!(
        session_stack(Some(&registry), &wool),
        Some(Stack {
            key: key("minecraft:wool", 14),
            icon: SessionData::icon_key(4, 14),
            count: 12,
        })
    );
    assert_eq!(session_stack(Some(&registry), &stack(99, 0)), None);
    assert_eq!(session_stack(None, &wool), None);
    assert_eq!(
        session_stack(Some(&registry), &::protocol::NetworkItemStack::empty()),
        None
    );
}

#[test]
fn recipe_derived_tag_changes_advance_the_item_revision() {
    let mut untagged = (*registry()).clone();
    untagged.insert(2, entry(2, "minecraft:oak_planks", &[]));
    let registry = Arc::new(untagged);
    let creative = creative();
    let mut cache = SessionDataCache::default();
    let mut publish = |catalog: &RecipeCatalog| {
        cache.update(
            SessionInputs {
                creative: Some(&creative),
                registry: Some(&registry),
                catalog: Some(catalog),
                language: [0; 3],
            },
            &name,
        )
    };
    let mut recipes = RecipeCatalog::default();
    recipes.begin_session(1);
    let empty = publish(&recipes);
    assert!(empty.tag_members("minecraft:planks").is_empty());
    recipes = catalog();
    let tagged = publish(&recipes);
    assert_eq!(
        tagged.tag_members("minecraft:planks"),
        ["minecraft:oak_planks"]
    );
    assert_eq!(tagged.item_revision, empty.item_revision + 1);
    assert_eq!(
        tagged.lookup(&key("minecraft:oak_planks", 0)).unwrap().tags,
        ["minecraft:planks"]
    );
    let update = CraftingDataFixture::clearing(Vec::new()).update();
    assert!(recipes.apply(1, 2, &update));
    let cleared = publish(&recipes);
    assert!(cleared.tag_members("minecraft:planks").is_empty());
    assert!(
        cleared
            .lookup(&key("minecraft:oak_planks", 0))
            .unwrap()
            .tags
            .is_empty()
    );
    assert_eq!(cleared.item_revision, tagged.item_revision + 1);
}

#[test]
fn cache_pins_source_identities_until_the_next_publication() {
    let registry = registry();
    let creative = creative();
    let old_registry = Arc::downgrade(&registry);
    let old_items = Arc::downgrade(&creative.items);
    let old_groups = Arc::downgrade(&creative.groups);
    let mut cache = SessionDataCache::default();
    let data = cache.update(
        SessionInputs {
            creative: Some(&creative),
            registry: Some(&registry),
            catalog: None,
            language: [0; 3],
        },
        &|identifier, _| identifier.to_owned(),
    );
    assert!(!data.items.is_empty());
    drop(registry);
    drop(creative);
    assert!(
        old_registry.upgrade().is_some(),
        "registry identity must remain live"
    );
    assert!(
        old_items.upgrade().is_some(),
        "creative item identity must remain live"
    );
    assert!(
        old_groups.upgrade().is_some(),
        "creative group identity must remain live"
    );
    let empty = cache.update(
        SessionInputs {
            creative: None,
            registry: None,
            catalog: None,
            language: [0; 3],
        },
        &|identifier, _| identifier.to_owned(),
    );
    assert!(empty.items.is_empty());
    assert!(empty.item_revision > data.item_revision);
    assert!(old_registry.upgrade().is_none());
    assert!(old_items.upgrade().is_none());
    assert!(old_groups.upgrade().is_none());
}

#[test]
fn session_adapter_pins_the_language_identity_used_for_item_names() {
    let text = b"item.stone.name=Server Stone";
    let overlay = assets::ServerLangOverlay::read(text.len(), |out| {
        out.copy_from_slice(text);
        true
    })
    .unwrap();
    let old = Arc::downgrade(&overlay);
    let mut runtime = UiRuntime::new(1);
    runtime.set_server_lang(Some(overlay));
    let player = player_state::PlayerState::new(1);
    let mut cache = SessionDataCache::default();
    session_data(&mut cache, &player, &runtime);
    runtime.set_server_lang(None);
    assert!(
        old.upgrade().is_some(),
        "language identity must remain live until the cache observes its replacement"
    );
    session_data(&mut cache, &player, &runtime);
    assert!(old.upgrade().is_none());
}

#[test]
fn brewing_recipes_list_the_potion_then_the_reagent() {
    let update = CraftingDataFixture {
        potions: vec![[4, 0, 3, 0, 4, 14]],
        containers: vec![[4, 7, 5]],
        clear_recipes: true,
        ..CraftingDataFixture::default()
    }
    .update();
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    let data = build(&creative(), &catalog);
    let [potion, container] = &data.recipes[..] else {
        panic!("{:?}", data.recipes);
    };
    assert_eq!(potion.category, RecipeCategory::Brewing);
    assert_eq!(
        potion.ingredients,
        [
            Some(Ingredient {
                kind: IngredientKind::Item(key("minecraft:wool", 0)),
                count: 1
            }),
            Some(Ingredient {
                kind: IngredientKind::Item(key("minecraft:ender_pearl", 0)),
                count: 1
            }),
        ]
    );
    assert_eq!(potion.outputs[0].key, key("minecraft:wool", 14));
    assert_eq!(
        container.ingredients[0],
        Some(Ingredient {
            kind: IngredientKind::AnyAux("minecraft:wool".into()),
            count: 1
        })
    );
    assert_eq!(container.outputs[0].key, key("minecraft:bed", 0));
}

#[test]
fn items_are_named_with_their_aux() {
    let registry = registry();
    let mut cache = SessionDataCache::default();
    let inputs = SessionInputs {
        creative: Some(&creative()),
        registry: Some(&registry),
        catalog: None,
        language: [0; 3],
    };
    let data = cache.update(inputs, &|identifier, aux| format!("{identifier}#{aux}"));
    assert_eq!(data.items[0].name, "minecraft:wool#14");
    assert_eq!(data.items[1].name, "minecraft:wool#0");
}
