use super::*;
use ::protocol::wire::valentine::bedrock::{codec::BedrockCodec, version::v1_26_51::*};

/// Admits original wire recipe inputs through the same bounded catalog as the session.
fn catalog(inputs: impl IntoIterator<Item = (String, bool, u16)>) -> Arc<RecipeCatalog> {
    let packet = CraftingDataPacket {
        shapeless_recipes: inputs
            .into_iter()
            .enumerate()
            .map(|(index, (name, tag, aux))| ShapelessRecipePayload {
                ingredients: vec![CerealizerRecipeIngredientSerializedData {
                    descriptor: vec![CerealizerRecipeIngredientSerializedDataDescriptorItem {
                        key: if tag { "item_tag" } else { "name" }.into(),
                        value: name,
                    }],
                    aux_value: i32::from(aux),
                    stack_size: 1,
                }],
                results: vec![CerealizerNetworkItemInstanceDescriptorSerializedData {
                    id: 1,
                    stacksize: 1,
                    ..Default::default()
                }],
                tag: "furnace".into(),
                net_id: TypedServerNetIdstructRecipeNetIdTag {
                    raw_id: index as u32 + 1,
                },
                ..Default::default()
            })
            .collect(),
        clear_recipes: true,
        ..Default::default()
    };
    let mut bytes = Vec::new();
    packet.encode(&mut bytes).unwrap();
    let update = ::protocol::decode_recipe_update(&bytes).unwrap();
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
