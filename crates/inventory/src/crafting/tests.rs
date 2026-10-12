use super::*;
use ::protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
};

/// Encode an ingredient through the same descriptor grammar used on the wire.
fn named(name: Option<&str>) -> IngredientFixture {
    name.map_or_else(IngredientFixture::empty, |name| {
        IngredientFixture::named(name, 0)
    })
}

/// Build one wire recipe so matcher tests use public decoded data.
fn recipe(width: u8, height: u8, shapeless: bool, cells: &[Option<&str>]) -> RecipeFixture {
    let ingredients = cells.iter().copied().map(named).collect();
    let results = vec![OutputFixture::new(9, 1)];
    if shapeless {
        RecipeFixture::shapeless("test:recipe", ingredients, results)
    } else {
        RecipeFixture::shaped(
            "test:recipe",
            i32::from(width),
            i32::from(height),
            ingredients,
            results,
        )
    }
}

/// Admit fixture recipes through the actual protocol decoder and catalog.
fn catalog(recipes: Vec<RecipeFixture>) -> RecipeCatalog {
    let recipes = recipes
        .into_iter()
        .enumerate()
        .map(|(index, mut recipe)| {
            recipe.net_id = index as u32 + 1;
            recipe
        })
        .collect();
    let update = CraftingDataFixture::clearing(recipes).update();
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
    unknown.ingredients[0].descriptor[0].0 = "item_tag".into();
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
    other_output.results[0].network_id = 10;
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
    for ingredient in &mut torch.ingredients {
        ingredient.aux = i32::from(::protocol::RECIPE_ANY_AUX);
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
    symmetric.assume_symmetry = true;
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
    generic.priority = -1;
    let mut specific = recipe(0, 0, true, &[Some("a:x")]);
    specific.priority = 2;
    specific.results[0].network_id = 10;
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
    for ingredient in &mut sticks.ingredients {
        ingredient.descriptor[0].0 = "item_tag".into();
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
