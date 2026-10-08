//! Ordinary BDS crafting recipes remain usable when their discovery metadata
//! is present. The oak-log -> four-planks shape follows the pinned vanilla
//! `behavior_pack/recipes/oak_planks.json`; numeric bindings are fixture-owned.

use ::protocol::wire::valentine::bedrock::{codec::BedrockCodec, version::v1_26_51::*};
use ::protocol::{RecipeCatalog, decode_recipe_update};
use bytes::BytesMut;
use inventory::{CraftGridItem, CraftGridMatch, match_crafting_grid};

const BLOCK_IDENTITY: u32 = 0xf234_5678;

fn recipe() -> ShapedRecipePayload {
    let ingredient = CerealizerRecipeIngredientSerializedData {
        descriptor: vec![CerealizerRecipeIngredientSerializedDataDescriptorItem {
            key: "name".into(),
            value: "minecraft:oak_log".into(),
        }],
        aux_value: 0,
        stack_size: 1,
    };
    ShapedRecipePayload {
        recipe_id: "minecraft:oak_planks".into(),
        width: 1,
        height: 1,
        ingredients: vec![ingredient.clone()],
        results: vec![CerealizerNetworkItemInstanceDescriptorSerializedData {
            id: 5,
            stacksize: 4,
            auxvalue: 0,
            block_runtime_id: i32::from_ne_bytes(BLOCK_IDENTITY.to_ne_bytes()),
            user_data_buffer: vec![0; 10],
        }],
        tag: "crafting_table".into(),
        unlocking_requirement: Some(CerealizerRecipeUnlockingRequirementSerializedData {
            unlocking_context: EnumsRecipeUnlockingRequirementUnlockingContext::None,
            unlocking_ingredients: Some(vec![ingredient]),
        }),
        net_id: TypedServerNetIdstructRecipeNetIdTag { raw_id: 1 },
        ..Default::default()
    }
}

fn catalog(recipe: ShapedRecipePayload) -> RecipeCatalog {
    let mut bytes = BytesMut::new();
    CraftingDataPacket {
        shaped_recipes: vec![recipe],
        clear_recipes: true,
        ..Default::default()
    }
    .encode(&mut bytes)
    .unwrap();
    let update = decode_recipe_update(&bytes).unwrap();
    assert!(!update.is_unavailable());
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    catalog
}

#[test]
fn ingredient_discovery_does_not_discard_manual_crafting_and_signed_block_bits_survive() {
    let catalog = catalog(recipe());
    let handle = catalog.recipe(1).expect("discoverable recipe retained");
    let ingredients = handle.ingredient_views();
    let log = ingredients[0].as_ref().expect("named log ingredient");
    assert!(inventory::ingredient_accepts(
        log,
        "minecraft:oak_log",
        0,
        &[]
    ));
    assert!(!inventory::ingredient_accepts(
        log,
        "minecraft:stone",
        0,
        &[]
    ));
    let output = handle.output();
    assert_eq!(output.block_runtime_id, BLOCK_IDENTITY);
    assert_eq!(output.count, 4);
    // A shaped one-cell recipe fits any cell in both personal and table grids.
    for width in [2, 3] {
        for slot in 0..usize::from(width) * usize::from(width) {
            let mut grid = vec![None; usize::from(width) * usize::from(width)];
            grid[slot] = Some(CraftGridItem {
                identifier: "minecraft:oak_log",
                metadata: 0,
                count: 8,
                plain: true,
                tags: &[],
            });
            let CraftGridMatch::Unique(matched) = match_crafting_grid(&catalog, width, &grid)
            else {
                panic!("no oak-planks match at width {width}, slot {slot}");
            };
            assert_eq!(matched.network_id(), handle.network_id());
            assert_eq!(matched.output(), output);
        }
    }
}

#[test]
fn discovery_context_is_not_a_recipe_admission_gate() {
    for context in [
        EnumsRecipeUnlockingRequirementUnlockingContext::Alwaysunlocked,
        EnumsRecipeUnlockingRequirementUnlockingContext::Playerinwater,
        EnumsRecipeUnlockingRequirementUnlockingContext::Playerhasmanyitems,
        EnumsRecipeUnlockingRequirementUnlockingContext::Unknown(127),
    ] {
        let mut recipe = recipe();
        recipe
            .unlocking_requirement
            .as_mut()
            .unwrap()
            .unlocking_context = context;
        assert!(catalog(recipe).recipe(1).is_some(), "context {context:?}");
    }
}

#[test]
fn discovery_ingredients_still_obey_nested_decode_bounds() {
    let mut recipe = recipe();
    recipe
        .unlocking_requirement
        .as_mut()
        .unwrap()
        .unlocking_ingredients = Some(vec![recipe.ingredients[0].clone(); 65]);
    let mut bytes = BytesMut::new();
    CraftingDataPacket {
        shaped_recipes: vec![recipe],
        ..Default::default()
    }
    .encode(&mut bytes)
    .unwrap();
    assert!(decode_recipe_update(&bytes).unwrap().is_unavailable());
}
