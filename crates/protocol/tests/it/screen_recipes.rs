use ::protocol::{RecipeCatalog, ScreenRecipeKind, decode_recipe_update};
use bedrock_protocol_1_26_51::packets::CraftingData;
use bedrock_protocol_1_26_51::types::{
    MultiRecipe, NetworkItemInstanceDescriptorSerializedData, RecipeIngredientSerializedData,
    RecipeNetID, ShapedRecipe, ShapelessRecipe, SmithingTransformRecipe,
};
use bedrock_protocol_1_26_51::wire::{self, Encode};

fn ingredient(key: &str, name: &str) -> RecipeIngredientSerializedData {
    RecipeIngredientSerializedData {
        descriptor: vec![(key.into(), name.into())],
        aux_value: wire::ZigZag32(0),
        stack_size: wire::ZigZag32(1),
    }
}

fn result(id: i32, count: u16) -> NetworkItemInstanceDescriptorSerializedData {
    NetworkItemInstanceDescriptorSerializedData {
        id: wire::ZigZag32(id),
        stack_size: wire::U16LE(count),
        aux_value: wire::VarUInt(0),
        block_runtime_id: wire::ZigZag32(0),
        user_data_buffer: vec![0; 10].into(),
    }
}

fn stonecutter(id: u32) -> ShapelessRecipe {
    ShapelessRecipe {
        recipe_id: "test:stone_stairs".into(),
        ingredients: vec![ingredient("name", "minecraft:stone")],
        results: vec![result(9, 1)],
        tag: "stonecutter".into(),
        net_id: RecipeNetID(id),
        ..Default::default()
    }
}

fn update(packet: CraftingData) -> ::protocol::RecipeUpdate {
    decode_recipe_update(&packet.encode_to_vec().unwrap()).unwrap()
}

/// Stonecutter, smithing and multi recipes reach the catalog beside crafting ones.
#[test]
fn screen_recipes_are_retained_by_kind() {
    let update = update(CraftingData {
        shapeless_recipes: vec![stonecutter(5)],
        multi_recipes: vec![MultiRecipe {
            multi_recipe_uuid: uuid::Uuid::from_u128(1),
            net_id: RecipeNetID(8),
        }],
        smithing_transform_recipes: vec![SmithingTransformRecipe {
            recipe_id: "test:smith".into(),
            template_ingredient: ingredient("name", "minecraft:template"),
            base_ingredient: ingredient("name", "minecraft:iron_sword"),
            addition_ingredient: ingredient("name", "minecraft:netherite_ingot"),
            result: result(11, 1),
            tag: "smithing_table".into(),
            net_id: RecipeNetID(6),
        }],
        clear_recipes: true,
        ..Default::default()
    });
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    let cutter: Vec<_> = catalog
        .screen_recipes(ScreenRecipeKind::Stonecutter)
        .collect();
    assert_eq!(cutter.len(), 1);
    assert_eq!(cutter[0].id, 5);
    assert_eq!(&*cutter[0].ingredients[0].name, "minecraft:stone");
    assert_eq!(cutter[0].output.unwrap().network_id, 9);
    let smith = catalog.screen_recipe(6).expect("smithing transform");
    assert_eq!(smith.kind, ScreenRecipeKind::SmithingTransform);
    assert_eq!(smith.ingredients.len(), 3);
    assert_eq!(catalog.repair_multi_recipe_id(), Some(8));
    // A crafting-table catalog still holds no stonecutter recipe.
    assert!(catalog.recipe(5).is_none());
}

#[test]
fn furnace_recipes_reach_the_screen_catalog_without_entering_the_crafting_grid() {
    let recipes = ["furnace", "blast_furnace", "smoker"]
        .into_iter()
        .enumerate()
        .map(|(index, tag)| ShapelessRecipe {
            tag: tag.into(),
            ..stonecutter(index as u32 + 1)
        })
        .collect();
    let update = update(CraftingData {
        shapeless_recipes: recipes,
        clear_recipes: true,
        ..Default::default()
    });
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    for id in 1..=3 {
        let recipe = catalog.screen_recipe(id).expect("furnace recipe retained");
        assert_eq!(&*recipe.ingredients[0].name, "minecraft:stone");
        assert_eq!(recipe.output.unwrap().network_id, 9);
        assert!(catalog.recipe(id).is_none());
    }
}

/// A later clearing update drops every screen recipe with the rest.
#[test]
fn a_clearing_update_replaces_screen_recipes() {
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    catalog.apply(
        1,
        1,
        &update(CraftingData {
            shapeless_recipes: vec![stonecutter(5)],
            clear_recipes: true,
            ..Default::default()
        }),
    );
    catalog.apply(
        1,
        2,
        &update(CraftingData {
            clear_recipes: true,
            ..Default::default()
        }),
    );
    assert!(catalog.screen_recipe(5).is_none());
}

/// Crafting recipes expose their ingredients to the recipe book in wire order.
#[test]
fn crafting_recipes_expose_ingredient_views() {
    let mut recipe = ShapedRecipe {
        recipe_id: "test:planks".into(),
        width: wire::ZigZag32(1),
        height: wire::ZigZag32(1),
        ingredients: vec![ingredient("name", "minecraft:oak_log")],
        results: vec![result(7, 4)],
        tag: "crafting_table".into(),
        net_id: RecipeNetID(3),
        ..Default::default()
    };
    recipe.ingredients[0].stack_size = wire::ZigZag32(1);
    let update = update(CraftingData {
        shaped_recipes: vec![recipe],
        clear_recipes: true,
        ..Default::default()
    });
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    let handles = catalog.crafting_handles();
    assert_eq!(handles.len(), 1);
    let views = handles[0].ingredient_views();
    assert_eq!(views.len(), 1);
    let log = views[0].as_ref().unwrap();
    assert_eq!(&*log.name, "minecraft:oak_log");
    assert!(!log.tag);
    assert_eq!(log.aux, 0);
    assert_eq!(log.count, 1);
}

/// Block items such as spruce planks have negative network ids; a recipe making one is still a
/// crafting recipe, and its output keeps that id.
#[test]
fn recipes_making_negative_id_block_items_are_kept() {
    let recipe = |id, output| ShapedRecipe {
        recipe_id: "test:planks".into(),
        width: wire::ZigZag32(1),
        height: wire::ZigZag32(1),
        ingredients: vec![ingredient("name", "minecraft:spruce_log")],
        results: vec![result(output, 4)],
        tag: "crafting_table".into(),
        net_id: RecipeNetID(id),
        ..Default::default()
    };
    let update = update(CraftingData {
        shaped_recipes: vec![recipe(3, -739), recipe(4, 0)],
        clear_recipes: true,
        ..Default::default()
    });
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    let handles = catalog.crafting_handles();
    // Air (id 0) makes nothing.
    assert_eq!(handles.len(), 1);
    assert_eq!(handles[0].network_id(), 3);
    assert_eq!(handles[0].recipe().output().network_id, -739);
}

/// Incremental replacements invalidate old station views even when the replacement is rejected.
#[test]
fn incremental_replacements_retire_all_old_views() {
    let mut invalid = stonecutter(5);
    invalid.results = vec![result(0, 1)];
    let mut retyped = stonecutter(5);
    retyped.tag = "crafting_table".into();
    for replacements in [
        vec![invalid],
        vec![stonecutter(5), stonecutter(5)],
        vec![retyped],
    ] {
        let mut catalog = RecipeCatalog::default();
        catalog.begin_session(1);
        catalog.apply(
            1,
            1,
            &update(CraftingData {
                shapeless_recipes: vec![stonecutter(5), stonecutter(9)],
                ..Default::default()
            }),
        );
        assert!(catalog.screen_recipe(5).is_some());
        catalog.apply(
            1,
            2,
            &update(CraftingData {
                shapeless_recipes: replacements,
                ..Default::default()
            }),
        );
        assert!(catalog.screen_recipe(5).is_none());
        assert!(catalog.screen_recipe(9).is_some());
    }
}

/// Unsupported input counts and quantities are counted, rather than reduced to a preview.
#[test]
fn station_shapes_are_explicitly_skipped() {
    let mut extra = stonecutter(5);
    extra.ingredients = vec![ingredient("name", "minecraft:stone"); 4];
    let mut quantity = stonecutter(6);
    quantity.ingredients[0].stack_size = wire::ZigZag32(2);
    let update = update(CraftingData {
        shapeless_recipes: vec![extra, quantity, stonecutter(7)],
        ..Default::default()
    });
    assert_eq!(update.skipped_screen_recipes(), 2);
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    catalog.apply(1, 1, &update);
    assert!(catalog.screen_recipe(5).is_none());
    assert!(catalog.screen_recipe(6).is_none());
    assert!(catalog.screen_recipe(7).is_some());
}

/// Duplicate IDs across station and multi families cannot preserve either interpretation.
#[test]
fn multi_recipe_replacements_and_cross_family_duplicates_retire_old_views() {
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    catalog.apply(
        1,
        1,
        &update(CraftingData {
            multi_recipes: vec![MultiRecipe {
                multi_recipe_uuid: uuid::Uuid::from_u128(1),
                net_id: RecipeNetID(5),
            }],
            ..Default::default()
        }),
    );
    assert_eq!(catalog.repair_multi_recipe_id(), Some(5));
    catalog.apply(
        1,
        2,
        &update(CraftingData {
            shapeless_recipes: vec![stonecutter(5)],
            multi_recipes: vec![MultiRecipe {
                multi_recipe_uuid: uuid::Uuid::from_u128(1),
                net_id: RecipeNetID(5),
            }],
            ..Default::default()
        }),
    );
    assert!(catalog.repair_multi_recipe_id().is_none());
    assert!(catalog.screen_recipe(5).is_none());
}

/// Conflicting repair advertisements must not choose an arbitrary request ID.
#[test]
fn multiple_repair_ids_are_ambiguous() {
    let update = update(CraftingData {
        multi_recipes: [5, 6]
            .into_iter()
            .map(|id| MultiRecipe {
                multi_recipe_uuid: uuid::Uuid::from_u128(1),
                net_id: RecipeNetID(id),
            })
            .collect(),
        ..Default::default()
    });
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    catalog.apply(1, 1, &update);
    assert!(catalog.repair_multi_recipe_id().is_none());
}

/// Potion and container mixes reach the catalog, and an update without any keeps them.
#[test]
fn brewing_recipes_are_retained_until_an_update_lists_new_ones() {
    use ::protocol::{BrewingItem, BrewingRecipe};
    use bedrock_protocol_1_26_51::types::{ContainerMixDataEntry, PotionMixDataEntry};
    let z = wire::ZigZag32;
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    let brewing = update(CraftingData {
        potion_mixes: vec![PotionMixDataEntry {
            from_potion_id: z(400),
            from_item_aux: z(0),
            reagent_item_id: z(401),
            reagent_item_aux: z(0),
            to_potion_id: z(400),
            to_item_aux: z(4),
        }],
        container_mixes: vec![ContainerMixDataEntry {
            from_item_id: z(400),
            reagent_item_id: z(402),
            to_item_id: z(403),
        }],
        clear_recipes: true,
        ..Default::default()
    });
    assert!(catalog.apply(1, 1, &brewing));
    let item = |network_id, aux| BrewingItem { network_id, aux };
    let expected = [
        BrewingRecipe {
            input: item(400, Some(0)),
            reagent: item(401, Some(0)),
            output: item(400, Some(4)),
        },
        BrewingRecipe {
            input: item(400, None),
            reagent: item(402, None),
            output: item(403, None),
        },
    ];
    assert_eq!(catalog.brewing_recipes(), expected);
    let crafting_only = update(CraftingData {
        shapeless_recipes: vec![stonecutter(5)],
        ..Default::default()
    });
    assert!(catalog.apply(1, 2, &crafting_only));
    assert_eq!(catalog.brewing_recipes(), expected);
}
