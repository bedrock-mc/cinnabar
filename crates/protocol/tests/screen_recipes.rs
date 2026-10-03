use ::protocol::{RecipeCatalog, ScreenRecipeKind, decode_recipe_update};
use bytes::BytesMut;
use valentine::bedrock::{codec::BedrockCodec, version::v1_26_51::*};

fn ingredient(key: &str, name: &str) -> CerealizerRecipeIngredientSerializedData {
    CerealizerRecipeIngredientSerializedData {
        descriptor: vec![CerealizerRecipeIngredientSerializedDataDescriptorItem {
            key: key.into(),
            value: name.into(),
        }],
        aux_value: 0,
        stack_size: 1,
    }
}

fn result(id: i32, count: u16) -> CerealizerNetworkItemInstanceDescriptorSerializedData {
    CerealizerNetworkItemInstanceDescriptorSerializedData {
        id,
        stacksize: count,
        auxvalue: 0,
        block_runtime_id: 0,
        user_data_buffer: vec![0; 10],
    }
}

fn stonecutter(id: u32) -> ShapelessRecipePayload {
    ShapelessRecipePayload {
        recipe_id: "test:stone_stairs".into(),
        ingredients: vec![ingredient("name", "minecraft:stone")],
        results: vec![result(9, 1)],
        tag: "stonecutter".into(),
        net_id: TypedServerNetIdstructRecipeNetIdTag { raw_id: id },
        ..Default::default()
    }
}

fn update(packet: CraftingDataPacket) -> ::protocol::RecipeUpdate {
    let mut bytes = BytesMut::new();
    packet.encode(&mut bytes).unwrap();
    decode_recipe_update(&bytes).unwrap()
}

/// Stonecutter, smithing and multi recipes reach the catalog beside crafting ones.
#[test]
fn screen_recipes_are_retained_by_kind() {
    let update = update(CraftingDataPacket {
        shapeless_recipes: vec![stonecutter(5)],
        multi_recipes: vec![MultiRecipePayload {
            multi_recipe_uuid: uuid::Uuid::from_u128(1),
            net_id: TypedServerNetIdstructRecipeNetIdTag { raw_id: 8 },
        }],
        smithing_transform_recipes: vec![SmithingTransformRecipePayload {
            recipe_id: "test:smith".into(),
            template_ingredient: ingredient("name", "minecraft:template"),
            base_ingredient: ingredient("name", "minecraft:iron_sword"),
            addition_ingredient: ingredient("name", "minecraft:netherite_ingot"),
            result: result(11, 1),
            tag: "smithing_table".into(),
            net_id: TypedServerNetIdstructRecipeNetIdTag { raw_id: 6 },
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

/// A later clearing update drops every screen recipe with the rest.
#[test]
fn a_clearing_update_replaces_screen_recipes() {
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    catalog.apply(
        1,
        1,
        &update(CraftingDataPacket {
            shapeless_recipes: vec![stonecutter(5)],
            clear_recipes: true,
            ..Default::default()
        }),
    );
    catalog.apply(
        1,
        2,
        &update(CraftingDataPacket {
            clear_recipes: true,
            ..Default::default()
        }),
    );
    assert!(catalog.screen_recipe(5).is_none());
}

/// Crafting recipes expose their ingredients to the recipe book in wire order.
#[test]
fn crafting_recipes_expose_ingredient_views() {
    let mut recipe = ShapedRecipePayload {
        recipe_id: "test:planks".into(),
        width: 1,
        height: 1,
        ingredients: vec![ingredient("name", "minecraft:oak_log")],
        results: vec![result(7, 4)],
        tag: "crafting_table".into(),
        net_id: TypedServerNetIdstructRecipeNetIdTag { raw_id: 3 },
        ..Default::default()
    };
    recipe.ingredients[0].stack_size = 1;
    let update = update(CraftingDataPacket {
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
