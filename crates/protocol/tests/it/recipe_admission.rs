use ::protocol::{RecipeCatalog, decode_recipe_update};
use bytes::BytesMut;
use valentine::bedrock::{codec::BedrockCodec, version::v1_26_51::*};

fn recipe(id: u32) -> ShapedRecipePayload {
    ShapedRecipePayload {
        recipe_id: "test:one".into(),
        width: 1,
        height: 1,
        ingredients: vec![CerealizerRecipeIngredientSerializedData {
            descriptor: vec![CerealizerRecipeIngredientSerializedDataDescriptorItem {
                key: "name".into(),
                value: "minecraft:oak_log".into(),
            }],
            aux_value: 0,
            stack_size: 1,
        }],
        results: vec![CerealizerNetworkItemInstanceDescriptorSerializedData {
            id: 7,
            stacksize: 4,
            auxvalue: 0,
            block_runtime_id: 0,
            user_data_buffer: vec![0; 10],
        }],
        tag: "crafting_table".into(),
        net_id: TypedServerNetIdstructRecipeNetIdTag { raw_id: id },
        ..Default::default()
    }
}
fn update(recipes: Vec<ShapedRecipePayload>, clear: bool) -> ::protocol::RecipeUpdate {
    let mut bytes = BytesMut::new();
    CraftingDataPacket {
        shaped_recipes: recipes,
        clear_recipes: clear,
        ..Default::default()
    }
    .encode(&mut bytes)
    .unwrap();
    decode_recipe_update(&bytes).unwrap()
}

#[test]
fn clear_merge_unsupported_replacement_and_fifo_are_authoritative() {
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update(vec![recipe(1)], true)));
    let retained = catalog.recipe(1).unwrap();
    assert_eq!(retained.dimensions(), (1, 1));
    assert!(catalog.apply(1, 2, &update(vec![recipe(2)], false)));
    assert!(catalog.recipe(1).is_some());
    let mut unsupported = recipe(1);
    unsupported.ingredients[0].descriptor[0].key = "molang".into();
    assert!(catalog.apply(1, 3, &update(vec![unsupported], false)));
    assert!(catalog.recipe(1).is_none());
    assert!(catalog.recipe(2).is_some());
    assert!(!catalog.apply(1, 2, &update(vec![recipe(1)], true)));
    assert!(!catalog.apply(2, 4, &update(vec![recipe(1)], true)));
    assert!(catalog.apply(1, 4, &update(vec![], true)));
    assert!(catalog.recipe(2).is_none());
    assert_eq!(retained.network_id(), 1);
}

#[test]
fn cloned_catalog_is_an_immutable_authority_snapshot() {
    let mut current = RecipeCatalog::default();
    current.begin_session(1);
    current.apply(1, 1, &update(vec![recipe(1)], true));
    let old = current.clone();
    let revision = old.revision();
    let mut replacement = recipe(1);
    replacement.width = 2;
    replacement
        .ingredients
        .push(replacement.ingredients[0].clone());
    current.apply(1, 2, &update(vec![replacement], true));
    assert_eq!(current.recipe(1).unwrap().dimensions(), (2, 1));
    assert_eq!(old.recipe(1).unwrap().dimensions(), (1, 1));
    assert_eq!(old.revision(), revision);
    current.begin_session(2);
    assert!(!current.is_available());
    assert!(old.is_available());
}

#[test]
fn unsupported_only_clear_and_duplicate_ids_never_keep_old_execution() {
    for reverse in [false, true] {
        let mut catalog = RecipeCatalog::default();
        catalog.begin_session(9);
        catalog.apply(9, 1, &update(vec![recipe(1)], true));
        let mut odd = recipe(1);
        odd.width = -1;
        let pair = if reverse {
            vec![odd, recipe(1)]
        } else {
            vec![recipe(1), odd]
        };
        catalog.apply(9, 2, &update(pair, false));
        assert!(catalog.recipe(1).is_none());
        let mut odd = recipe(2);
        odd.results[0].auxvalue = 65536;
        catalog.apply(9, 3, &update(vec![odd], true));
        assert!(catalog.recipe(1).is_none());
        assert!(catalog.recipe(2).is_none());
    }
}

#[test]
fn refusal_is_atomic_and_truncation_or_trailing_wire_is_not_semantic() {
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    catalog.apply(1, 1, &update(vec![recipe(1)], true));
    // No traversal/allocation of this declared vector occurs.
    let refused = decode_recipe_update(&[0x81, 0x40]).unwrap();
    assert!(refused.is_unavailable());
    catalog.apply(1, 2, &refused);
    assert!(!catalog.is_available());
    assert!(catalog.recipe(1).is_none());
    assert!(decode_recipe_update(&[1]).is_err());
    let mut empty = [0u8; 13];
    empty[11] = 1;
    assert!(decode_recipe_update(&empty).is_err());
    assert!(!decode_recipe_update(&empty[..12]).unwrap().is_unavailable());
    assert!(
        decode_recipe_update(&vec![0; 16 * 1024 * 1024 + 1])
            .unwrap()
            .is_unavailable()
    );
}

#[test]
fn descriptors_dimensions_and_complex_outputs_are_unavailable_not_fatal() {
    let mut cases = Vec::new();
    let mut r = recipe(1);
    r.ingredients[0].aux_value = -1;
    cases.push(r);
    let mut r = recipe(1);
    let duplicate = r.ingredients[0].descriptor[0].clone();
    r.ingredients[0].descriptor.push(duplicate);
    cases.push(r);
    let mut r = recipe(1);
    r.ingredients[0].descriptor[0].value = "invalid name".into();
    cases.push(r);
    let mut r = recipe(1);
    r.results[0].user_data_buffer = vec![0xff, 0xff, 1];
    cases.push(r);
    let mut r = recipe(1);
    r.height = 3;
    cases.push(r);
    for r in cases {
        let mut catalog = RecipeCatalog::default();
        catalog.begin_session(1);
        let update = update(vec![r], true);
        assert!(!update.is_unavailable());
        catalog.apply(1, 1, &update);
        assert!(catalog.recipe(1).is_none());
    }
}

#[test]
fn every_top_level_family_is_bounded_before_its_declared_entries() {
    for family in 0..11 {
        let mut body = vec![0; family];
        body.extend_from_slice(&[0x81, 0x40]); // 8193 entries, no materialization.
        assert!(decode_recipe_update(&body).unwrap().is_unavailable());
    }
}

#[test]
fn nested_declared_work_and_string_userdata_limits_refuse_atomically() {
    let base = recipe(1);
    let mut cases = Vec::new();
    let mut r = recipe(1);
    r.ingredients = vec![base.ingredients[0].clone(); 65];
    cases.push(r);
    let mut r = recipe(1);
    r.results = vec![base.results[0].clone(); 65];
    cases.push(r);
    let mut r = recipe(1);
    r.ingredients[0].descriptor = vec![base.ingredients[0].descriptor[0].clone(); 9];
    cases.push(r);
    let mut r = recipe(1);
    r.recipe_id = "a".repeat(16385);
    cases.push(r);
    let mut r = recipe(1);
    r.results[0].user_data_buffer = vec![0; 65537];
    cases.push(r);
    let mut r = recipe(1);
    r.unlocking_requirement = Some(CerealizerRecipeUnlockingRequirementSerializedData {
        unlocking_context: EnumsRecipeUnlockingRequirementUnlockingContext::Alwaysunlocked,
        unlocking_ingredients: Some(vec![base.ingredients[0].clone(); 65]),
    });
    cases.push(r);
    for recipe in cases {
        assert!(update(vec![recipe], false).is_unavailable());
    }
}

#[test]
fn all_well_formed_unavailable_families_are_traversed_without_activation() {
    let mut bytes = BytesMut::new();
    CraftingDataPacket {
        shapeless_recipes: vec![ShapelessRecipePayload {
            net_id: TypedServerNetIdstructRecipeNetIdTag { raw_id: 1 },
            ..Default::default()
        }],
        multi_recipes: vec![MultiRecipePayload {
            net_id: TypedServerNetIdstructRecipeNetIdTag { raw_id: 2 },
            ..Default::default()
        }],
        user_data_shapeless_recipes: vec![Default::default()],
        reserved_field_4: vec![Default::default()],
        reserved_field_5: vec![Default::default()],
        smithing_transform_recipes: vec![Default::default()],
        smithing_trim_recipes: vec![Default::default()],
        potion_mixes: vec![Default::default()],
        container_mixes: vec![Default::default()],
        reserved_field_10: vec![Default::default()],
        clear_recipes: true,
        ..Default::default()
    }
    .encode(&mut bytes)
    .unwrap();
    let update = decode_recipe_update(&bytes).unwrap();
    assert!(!update.is_unavailable());
    assert!(update.clears_catalog());
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    catalog.apply(1, 1, &update);
    assert!(catalog.recipe(1).is_none());
    assert!(catalog.recipe(2).is_none());
}
