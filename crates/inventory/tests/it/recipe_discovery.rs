//! Ordinary BDS crafting recipes remain usable when their discovery metadata
//! is present. The oak-log -> four-planks shape follows the pinned vanilla
//! `behavior_pack/recipes/oak_planks.json`; numeric bindings are fixture-owned.

use ::protocol::RecipeCatalog;
use ::protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture, UnlockingFixture,
};
use inventory::{CraftGridItem, CraftGridMatch, match_crafting_grid};

const BLOCK_IDENTITY: u32 = 0xf234_5678;

// Wire values of the named discovery contexts; the test also sends unknown 127.
const ALWAYS_UNLOCKED: i32 = 1;
const PLAYER_IN_WATER: i32 = 2;
const PLAYER_HAS_MANY_ITEMS: i32 = 3;

fn recipe() -> RecipeFixture {
    let ingredient = IngredientFixture::named("minecraft:oak_log", 0);
    let output = OutputFixture {
        block_runtime_id: i32::from_ne_bytes(BLOCK_IDENTITY.to_ne_bytes()),
        user_data: vec![0; 10],
        ..OutputFixture::new(5, 4)
    };
    RecipeFixture {
        unlocking: Some(UnlockingFixture {
            context: 0,
            ingredients: Some(vec![ingredient.clone()]),
        }),
        net_id: 1,
        ..RecipeFixture::shaped("minecraft:oak_planks", 1, 1, vec![ingredient], vec![output])
    }
}

fn catalog(recipe: RecipeFixture) -> RecipeCatalog {
    let update = CraftingDataFixture::clearing(vec![recipe]).update();
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
    for context in [ALWAYS_UNLOCKED, PLAYER_IN_WATER, PLAYER_HAS_MANY_ITEMS, 127] {
        let mut recipe = recipe();
        recipe.unlocking.as_mut().unwrap().context = context;
        assert!(catalog(recipe).recipe(1).is_some(), "context {context:?}");
    }
}

#[test]
fn discovery_ingredients_still_obey_nested_decode_bounds() {
    let mut recipe = recipe();
    recipe.unlocking.as_mut().unwrap().ingredients = Some(vec![recipe.ingredients[0].clone(); 65]);
    let update = CraftingDataFixture {
        recipes: vec![recipe],
        clear_recipes: false,
    }
    .update();
    assert!(update.is_unavailable());
}
