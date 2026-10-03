use super::*;
use bytes::BytesMut;
use valentine::bedrock::{codec::BedrockCodec, version::v1_26_51::*};

/// Encode an ingredient through the same descriptor grammar used on the wire.
fn named(name: Option<&str>) -> CerealizerRecipeIngredientSerializedData {
    CerealizerRecipeIngredientSerializedData {
        descriptor: name
            .into_iter()
            .map(
                |name| CerealizerRecipeIngredientSerializedDataDescriptorItem {
                    key: "name".into(),
                    value: name.into(),
                },
            )
            .collect(),
        aux_value: 0,
        stack_size: i32::from(name.is_some()),
    }
}

/// Build one wire recipe so matcher tests use public decoded data.
fn recipe(width: u8, height: u8, shapeless: bool, cells: &[Option<&str>]) -> CraftingDataPacket {
    let ingredients = cells.iter().copied().map(named).collect();
    let results = vec![CerealizerNetworkItemInstanceDescriptorSerializedData {
        id: 9,
        stacksize: 1,
        auxvalue: 0,
        block_runtime_id: 0,
        user_data_buffer: Vec::new(),
    }];
    if shapeless {
        CraftingDataPacket {
            shapeless_recipes: vec![ShapelessRecipePayload {
                recipe_id: "test:recipe".into(),
                ingredients,
                results,
                tag: "crafting_table".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    } else {
        CraftingDataPacket {
            shaped_recipes: vec![ShapedRecipePayload {
                recipe_id: "test:recipe".into(),
                width: i32::from(width),
                height: i32::from(height),
                ingredients,
                results,
                tag: "crafting_table".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }
}

/// Borrow fixture ingredients regardless of whether the recipe has a shape.
fn ingredients(
    recipe: &mut CraftingDataPacket,
) -> &mut Vec<CerealizerRecipeIngredientSerializedData> {
    if let Some(recipe) = recipe.shaped_recipes.first_mut() {
        &mut recipe.ingredients
    } else {
        &mut recipe.shapeless_recipes[0].ingredients
    }
}

/// Borrow fixture outputs regardless of whether the recipe has a shape.
fn results(
    recipe: &mut CraftingDataPacket,
) -> &mut Vec<CerealizerNetworkItemInstanceDescriptorSerializedData> {
    if let Some(recipe) = recipe.shaped_recipes.first_mut() {
        &mut recipe.results
    } else {
        &mut recipe.shapeless_recipes[0].results
    }
}

/// Admit fixture recipes through the actual protocol decoder and catalog.
fn catalog(recipes: Vec<CraftingDataPacket>) -> RecipeCatalog {
    let mut packet = CraftingDataPacket {
        clear_recipes: true,
        ..Default::default()
    };
    for (index, mut recipe) in recipes.into_iter().enumerate() {
        for recipe in &mut recipe.shaped_recipes {
            recipe.net_id.raw_id = index as u32 + 1;
        }
        for recipe in &mut recipe.shapeless_recipes {
            recipe.net_id.raw_id = index as u32 + 1;
        }
        packet.shaped_recipes.extend(recipe.shaped_recipes);
        packet.shapeless_recipes.extend(recipe.shapeless_recipes);
    }
    let mut bytes = BytesMut::new();
    packet.encode(&mut bytes).unwrap();
    let update = ::protocol::decode_recipe_update(&bytes).unwrap();
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    catalog
}

/// Represent one ordinary, plain grid item.
fn item(identifier: &str) -> Option<CraftGridItem<'_>> {
    Some(CraftGridItem {
        identifier,
        metadata: 0,
        count: 1,
        plain: true,
        tags: &[],
    })
}

/// Extract the chosen recipe identity for concise assertions.
fn unique_id(result: CraftGridMatch) -> Option<u32> {
    match result {
        CraftGridMatch::Unique(handle) => Some(handle.network_id()),
        _ => None,
    }
}

/// A shaped recipe matches anywhere its exact extent fits, never mirrored.
#[test]
fn shaped_recipes_translate_but_keep_orientation() {
    let catalog = catalog(vec![recipe(2, 1, false, &[Some("a:x"), Some("a:y")])]);
    let mut table = vec![None; 9];
    table[7] = item("a:x");
    table[8] = item("a:y");
    assert_eq!(unique_id(match_crafting_grid(&catalog, 3, &table)), Some(1));
    let mirrored = [item("a:y"), item("a:x"), None, None];
    assert!(matches!(
        match_crafting_grid(&catalog, 2, &mirrored),
        CraftGridMatch::NoMatch
    ));
}

/// A full 3x3 ring needs the table grid and every cell in place.
#[test]
fn three_by_three_recipes_need_the_table_grid() {
    let ring: Vec<Option<&str>> = (0..9)
        .map(|index| (index != 4).then_some("a:stone"))
        .collect();
    let catalog = catalog(vec![recipe(3, 3, false, &ring)]);
    let mut grid: Vec<_> = ring.iter().map(|cell| cell.and_then(item)).collect();
    assert_eq!(unique_id(match_crafting_grid(&catalog, 3, &grid)), Some(1));
    grid[4] = item("a:stone");
    assert!(matches!(
        match_crafting_grid(&catalog, 3, &grid),
        CraftGridMatch::NoMatch
    ));
    assert!(matches!(
        match_crafting_grid(&catalog, 2, &grid[..4]),
        CraftGridMatch::NoMatch
    ));
}

/// Shapeless recipes accept any placement but exactly their ingredients.
#[test]
fn shapeless_recipes_assign_each_cell_once() {
    let catalog = catalog(vec![recipe(0, 0, true, &[Some("a:x"), Some("a:y")])]);
    assert_eq!(
        unique_id(match_crafting_grid(
            &catalog,
            2,
            &[None, item("a:y"), item("a:x"), None]
        )),
        Some(1)
    );
    for grid in [
        [item("a:x"), item("a:x"), None, None],
        [item("a:x"), item("a:y"), item("a:y"), None],
        [item("a:x"), None, None, None],
    ] {
        assert!(matches!(
            match_crafting_grid(&catalog, 2, &grid),
            CraftGridMatch::NoMatch
        ));
    }
}

/// Tags, item data and metadata never match loosely; two recipes are ambiguous.
#[test]
fn tags_data_and_duplicates_fail_closed() {
    let mut unknown = recipe(1, 1, false, &[Some("custom:unknown_tag")]);
    ingredients(&mut unknown)[0].descriptor[0].key = "item_tag".into();
    let catalog_with_tag = catalog(vec![unknown]);
    assert!(matches!(
        match_crafting_grid(
            &catalog_with_tag,
            2,
            &[item("custom:unknown_tag"), None, None, None]
        ),
        CraftGridMatch::NoMatch
    ));
    let declared = [std::sync::Arc::from("custom:unknown_tag")];
    let mut member = item("custom:thing");
    member.as_mut().unwrap().tags = &declared;
    assert!(matches!(
        match_crafting_grid(&catalog_with_tag, 2, &[member, None, None, None]),
        CraftGridMatch::Unique(_)
    ));
    let mut other_output = recipe(0, 0, true, &[Some("a:x")]);
    results(&mut other_output)[0].id = 10;
    let duplicates = catalog(vec![recipe(1, 1, false, &[Some("a:x")]), other_output]);
    assert!(matches!(
        match_crafting_grid(&duplicates, 2, &[item("a:x"), None, None, None]),
        CraftGridMatch::Ambiguous
    ));
    let single = catalog(vec![recipe(1, 1, false, &[Some("a:x")])]);
    let mut data = item("a:x");
    data.as_mut().unwrap().plain = false;
    let mut variant = item("a:x");
    variant.as_mut().unwrap().metadata = 1;
    for cell in [data, variant] {
        assert!(matches!(
            match_crafting_grid(&single, 2, &[cell, None, None, None]),
            CraftGridMatch::NoMatch
        ));
    }
    assert!(matches!(
        match_crafting_grid(
            &RecipeCatalog::default(),
            2,
            &[item("a:x"), None, None, None]
        ),
        CraftGridMatch::Unavailable
    ));
}

/// Metadata 32767 accepts any variant, as real crafting data uses it for
/// most ingredients (torch: coal@32767 over stick@32767).
#[test]
fn any_metadata_ingredients_accept_every_variant() {
    let mut torch = recipe(
        1,
        2,
        false,
        &[Some("minecraft:coal"), Some("minecraft:stick")],
    );
    for ingredient in ingredients(&mut torch) {
        ingredient.aux_value = i32::from(::protocol::RECIPE_ANY_AUX);
    }
    let catalog = catalog(vec![torch]);
    let mut coal = item("minecraft:coal");
    coal.as_mut().unwrap().metadata = 1;
    assert_eq!(
        unique_id(match_crafting_grid(
            &catalog,
            2,
            &[coal, None, item("minecraft:stick"), None]
        )),
        Some(1)
    );
}

/// A symmetric recipe also matches its horizontal mirror; an asymmetric one
/// only as written.
#[test]
fn mirror_flag_allows_the_horizontal_mirror_only() {
    let hoe = [
        Some("a:head"),
        Some("a:head"),
        None,
        Some("a:stick"),
        None,
        Some("a:stick"),
    ];
    let mirrored: Vec<_> = [
        item("a:head"),
        item("a:head"),
        None,
        item("a:stick"),
        None,
        None,
        item("a:stick"),
        None,
        None,
    ]
    .into();
    let fixed = catalog(vec![recipe(2, 3, false, &hoe)]);
    assert!(matches!(
        match_crafting_grid(&fixed, 3, &mirrored),
        CraftGridMatch::NoMatch
    ));
    let mut symmetric = recipe(2, 3, false, &hoe);
    symmetric.shaped_recipes[0].assume_symmetry = true;
    let catalog = catalog(vec![symmetric]);
    assert_eq!(
        unique_id(match_crafting_grid(&catalog, 3, &mirrored)),
        Some(1)
    );
}

/// Among matches, the lowest priority wins even with a different output.
#[test]
fn lowest_priority_wins_among_matches() {
    let mut generic = recipe(1, 1, false, &[Some("a:x")]);
    generic.shaped_recipes[0].priority = -1;
    let mut specific = recipe(0, 0, true, &[Some("a:x")]);
    specific.shapeless_recipes[0].priority = 2;
    results(&mut specific)[0].id = 10;
    let catalog = catalog(vec![specific, generic]);
    assert_eq!(
        unique_id(match_crafting_grid(
            &catalog,
            2,
            &[item("a:x"), None, None, None]
        )),
        Some(2)
    );
}

/// Vanilla tag membership comes from the pinned table.
#[test]
fn vanilla_tags_match_their_members_only() {
    let mut sticks = recipe(
        1,
        2,
        false,
        &[Some("minecraft:planks"), Some("minecraft:planks")],
    );
    for ingredient in ingredients(&mut sticks) {
        ingredient.descriptor[0].key = "item_tag".into();
    }
    let catalog = catalog(vec![sticks]);
    let planks = [
        item("minecraft:oak_planks"),
        None,
        item("minecraft:birch_planks"),
        None,
    ];
    assert_eq!(
        unique_id(match_crafting_grid(&catalog, 2, &planks)),
        Some(1)
    );
    let logs = [
        item("minecraft:oak_log"),
        None,
        item("minecraft:oak_log"),
        None,
    ];
    assert!(matches!(
        match_crafting_grid(&catalog, 2, &logs),
        CraftGridMatch::NoMatch
    ));
}
