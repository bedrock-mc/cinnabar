use super::*;
use ::protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
};

/// Admits original wire recipe inputs through the same bounded catalog as the session.
fn catalog(inputs: impl IntoIterator<Item = (String, bool, u16)>) -> Arc<RecipeCatalog> {
    let recipes = inputs
        .into_iter()
        .enumerate()
        .map(|(index, (name, tag, aux))| {
            let mut ingredient = IngredientFixture::named(&name, i32::from(aux));
            if tag {
                ingredient.descriptor[0].0 = "item_tag".into();
            }
            RecipeFixture {
                tag: "furnace".into(),
                net_id: index as u32 + 1,
                ..RecipeFixture::shapeless("", vec![ingredient], vec![OutputFixture::new(1, 1)])
            }
        })
        .collect();
    let update = CraftingDataFixture::clearing(recipes).update();
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    Arc::new(catalog)
}

/// Creates distinct numeric aliases with one shared icon name and one declared tag.
fn registry() -> Arc<BTreeMap<i32, ItemRegistryEntry>> {
    Arc::new(
        (21..=23)
            .map(|id| {
                let mut entry = ::protocol::vanilla_item_registry()[0].clone();
                entry.network_id = id;
                entry.identifier = Arc::from("custom:input");
                entry.item_tags = if id == 21 {
                    Arc::from([Arc::from("custom:cookable")])
                } else {
                    Arc::from([])
                };
                (id, entry)
            })
            .collect(),
    )
}

#[test]
fn furnace_inputs_join_declared_tags_once_and_preserve_unknown_catalogs() {
    let mut inputs = FurnaceInputs::default();
    let catalog = catalog([("custom:cookable".into(), true, 0)]);
    let registry = registry();
    assert!(inputs.refresh(Some(Arc::clone(&catalog)), Some(Arc::clone(&registry))));
    assert_eq!(
        inputs.accepts(WindowKind::Furnace, 21, u32::MAX),
        Some(true)
    );
    assert_eq!(inputs.accepts(WindowKind::Furnace, 22, 0), Some(false));
    assert!(!inputs.refresh(Some(catalog), Some(Arc::clone(&registry))));
    assert!(inputs.refresh(None, Some(registry)));
    assert_eq!(inputs.accepts(WindowKind::Furnace, 21, 0), None);
}

#[test]
fn furnace_input_join_overflow_keeps_the_whole_admission_unresolved() {
    let registry = registry();
    let count = MAX_BINDINGS / registry.len() + 1;
    let catalog =
        catalog((0..count).map(|aux| ("custom:input".into(), false, u16::try_from(aux).unwrap())));
    let mut inputs = FurnaceInputs::default();
    inputs.refresh(Some(catalog), Some(registry));
    assert_eq!(inputs.accepts(WindowKind::Furnace, 21, 0), None);
}
