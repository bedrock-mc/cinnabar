//! Grid matching over recipe shapes taken from real crafting data (Dragonfly's
//! MIT `crafting_data.nbt`): metadata 32767 wildcards, tag ingredients and
//! priorities that split generic tag recipes from per-variant ones.

use ::protocol::wire::valentine::bedrock::{codec::BedrockCodec, version::v1_26_51::*};
use ::protocol::*;
use bytes::BytesMut;
use inventory::{CraftGridItem, CraftGridMatch, match_crafting_grid};

const ANY: i32 = ::protocol::RECIPE_ANY_AUX as i32;

fn named(value: &str, aux: i32) -> CerealizerRecipeIngredientSerializedData {
    CerealizerRecipeIngredientSerializedData {
        descriptor: vec![CerealizerRecipeIngredientSerializedDataDescriptorItem {
            key: "name".into(),
            value: value.into(),
        }],
        aux_value: aux,
        stack_size: 1,
    }
}

fn tagged(value: &str) -> CerealizerRecipeIngredientSerializedData {
    CerealizerRecipeIngredientSerializedData {
        descriptor: vec![CerealizerRecipeIngredientSerializedDataDescriptorItem {
            key: "item_tag".into(),
            value: value.into(),
        }],
        aux_value: 0,
        stack_size: 1,
    }
}

fn empty() -> CerealizerRecipeIngredientSerializedData {
    CerealizerRecipeIngredientSerializedData::default()
}

fn output(id: i32, count: u16) -> Vec<CerealizerNetworkItemInstanceDescriptorSerializedData> {
    vec![CerealizerNetworkItemInstanceDescriptorSerializedData {
        id,
        stacksize: count,
        auxvalue: 0,
        block_runtime_id: 0,
        user_data_buffer: Vec::new(),
    }]
}

#[allow(clippy::too_many_arguments)]
fn shaped(
    id: u32,
    width: i32,
    height: i32,
    ingredients: Vec<CerealizerRecipeIngredientSerializedData>,
    result: i32,
    count: u16,
    priority: i32,
    assume_symmetry: bool,
) -> ShapedRecipePayload {
    ShapedRecipePayload {
        recipe_id: format!("test:{id}"),
        width,
        height,
        ingredients,
        results: output(result, count),
        tag: "crafting_table".into(),
        priority,
        assume_symmetry,
        net_id: TypedServerNetIdstructRecipeNetIdTag { raw_id: id },
        ..Default::default()
    }
}

fn catalog() -> RecipeCatalog {
    let planks = || tagged("minecraft:planks");
    let chest_ring = (0..9)
        .map(|index| if index == 4 { empty() } else { planks() })
        .collect();
    let mut bytes = BytesMut::new();
    CraftingDataPacket {
        shaped_recipes: vec![
            // torch x4: coal@32767 over stick@32767
            shaped(
                1,
                1,
                2,
                vec![named("minecraft:coal", ANY), named("minecraft:stick", ANY)],
                50,
                4,
                0,
                true,
            ),
            // stick x4: #planks over #planks, the generic recipe
            shaped(2, 1, 2, vec![planks(), planks()], 51, 4, -1, true),
            // crafting table: the #planks recipe and the oak variant
            shaped(3, 2, 2, (0..4).map(|_| planks()).collect(), 52, 1, 0, true),
            shaped(
                4,
                2,
                2,
                (0..4).map(|_| named("minecraft:oak_planks", ANY)).collect(),
                52,
                1,
                1,
                true,
            ),
            // chest: a #planks ring
            shaped(5, 3, 3, chest_ring, 53, 1, 0, true),
            // an asymmetric shape both as written and symmetric
            shaped(
                6,
                2,
                2,
                vec![
                    named("minecraft:flint", ANY),
                    empty(),
                    empty(),
                    named("minecraft:stick", ANY),
                ],
                54,
                1,
                0,
                false,
            ),
            shaped(
                7,
                2,
                2,
                vec![
                    named("minecraft:feather", ANY),
                    empty(),
                    empty(),
                    named("minecraft:stick", ANY),
                ],
                55,
                1,
                0,
                true,
            ),
        ],
        shapeless_recipes: vec![ShapelessRecipePayload {
            recipe_id: "test:book".into(),
            ingredients: vec![
                named("minecraft:paper", 0),
                named("minecraft:paper", 0),
                named("minecraft:paper", 0),
                named("minecraft:leather", 0),
            ],
            results: output(56, 1),
            tag: "crafting_table".into(),
            net_id: TypedServerNetIdstructRecipeNetIdTag { raw_id: 8 },
            ..Default::default()
        }],
        clear_recipes: true,
        ..Default::default()
    }
    .encode(&mut bytes)
    .unwrap();
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &decode_recipe_update(&bytes).unwrap()));
    catalog
}

fn cell(identifier: &str, metadata: u32) -> Option<CraftGridItem<'_>> {
    Some(CraftGridItem {
        identifier,
        metadata,
        count: 1,
        plain: true,
        tags: &[],
    })
}

fn matched(catalog: &RecipeCatalog, width: u8, grid: &[Option<CraftGridItem<'_>>]) -> Option<u32> {
    match match_crafting_grid(catalog, width, grid) {
        CraftGridMatch::Unique(recipe) => Some(recipe.network_id()),
        _ => None,
    }
}

/// Wildcard metadata accepts any variant, e.g. charcoal-style coal data.
#[test]
fn wildcard_metadata_accepts_every_variant() {
    let catalog = catalog();
    for metadata in [0, 1, 7] {
        let torch = [
            cell("minecraft:coal", metadata),
            None,
            cell("minecraft:stick", 0),
            None,
        ];
        assert_eq!(matched(&catalog, 2, &torch), Some(1), "metadata {metadata}");
    }
}

/// Tag recipes match any member; the lower priority wins over a variant.
#[test]
fn tag_recipes_and_priorities_resolve_one_output() {
    let catalog = catalog();
    let sticks = [
        cell("minecraft:oak_planks", 0),
        None,
        cell("minecraft:spruce_planks", 0),
        None,
    ];
    assert_eq!(matched(&catalog, 2, &sticks), Some(2));
    let oak = [
        cell("minecraft:oak_planks", 0),
        cell("minecraft:oak_planks", 0),
        cell("minecraft:oak_planks", 0),
        cell("minecraft:oak_planks", 0),
    ];
    assert_eq!(matched(&catalog, 2, &oak), Some(3), "priority 0 beats 1");
    let ring: Vec<_> = (0..9)
        .map(|index| {
            (index != 4)
                .then(|| cell("minecraft:birch_planks", 0))
                .flatten()
        })
        .collect();
    assert_eq!(matched(&catalog, 3, &ring), Some(5));
    let logs = [
        cell("minecraft:oak_log", 0),
        None,
        cell("minecraft:oak_log", 0),
        None,
    ];
    assert_eq!(matched(&catalog, 2, &logs), None);
}

/// Only symmetric recipes match their horizontal mirror.
#[test]
fn symmetry_flag_decides_mirrored_matches() {
    let catalog = catalog();
    let flint = [
        None,
        cell("minecraft:flint", 0),
        cell("minecraft:stick", 0),
        None,
    ];
    assert_eq!(matched(&catalog, 2, &flint), None);
    let feather = [
        None,
        cell("minecraft:feather", 0),
        cell("minecraft:stick", 0),
        None,
    ];
    assert_eq!(matched(&catalog, 2, &feather), Some(7));
    let book = [
        cell("minecraft:leather", 0),
        cell("minecraft:paper", 0),
        cell("minecraft:paper", 0),
        cell("minecraft:paper", 0),
    ];
    assert_eq!(matched(&catalog, 2, &book), Some(8));
}
