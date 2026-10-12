use ::protocol::{RecipeCatalog, decode_recipe_update};
use bedrock_protocol_1_26_51::enums::RecipeUnlockingRequirementUnlockingContext;
use bedrock_protocol_1_26_51::packets::CraftingData;
use bedrock_protocol_1_26_51::types::{
    MultiRecipe, NetworkItemInstanceDescriptorSerializedData, RecipeIngredientSerializedData,
    RecipeNetID, RecipeUnlockRequirementSerializedData, ShapedRecipe, ShapelessRecipe,
};
use bedrock_protocol_1_26_51::wire::{self, Encode};

/// A placeholder result aux the encoder accepts, as its varint.
const RESULT_AUX_PLACEHOLDER: (u32, &[u8]) = (12_345, &[0xb9, 0x60]);

fn recipe(id: u32) -> ShapedRecipe {
    ShapedRecipe {
        recipe_id: "test:one".into(),
        width: wire::ZigZag32(1),
        height: wire::ZigZag32(1),
        ingredients: vec![RecipeIngredientSerializedData {
            descriptor: vec![("name".into(), "minecraft:oak_log".into())],
            aux_value: wire::ZigZag32(0),
            stack_size: wire::ZigZag32(1),
        }],
        results: vec![NetworkItemInstanceDescriptorSerializedData {
            id: wire::ZigZag32(7),
            stack_size: wire::U16LE(4),
            aux_value: wire::VarUInt(0),
            block_runtime_id: wire::ZigZag32(0),
            user_data_buffer: vec![0; 10].into(),
        }],
        tag: "crafting_table".into(),
        net_id: RecipeNetID(id),
        ..Default::default()
    }
}

fn encode(recipes: Vec<ShapedRecipe>, clear: bool) -> Vec<u8> {
    CraftingData {
        shaped_recipes: recipes,
        clear_recipes: clear,
        ..Default::default()
    }
    .encode_to_vec()
    .unwrap()
}

fn update(recipes: Vec<ShapedRecipe>, clear: bool) -> ::protocol::RecipeUpdate {
    decode_recipe_update(&encode(recipes, clear)).unwrap()
}

/// Swaps the one encoded `placeholder` for wire bytes the encoder's schema checks refuse.
fn splice(bytes: Vec<u8>, placeholder: &[u8], replacement: &[u8]) -> Vec<u8> {
    let starts: Vec<_> = bytes
        .windows(placeholder.len())
        .enumerate()
        .filter(|(_, window)| *window == placeholder)
        .map(|(start, _)| start)
        .collect();
    let [start] = starts[..] else {
        panic!("placeholder must occur exactly once");
    };
    let mut spliced = bytes[..start].to_vec();
    spliced.extend_from_slice(replacement);
    spliced.extend_from_slice(&bytes[start + placeholder.len()..]);
    spliced
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
    unsupported.ingredients[0].descriptor[0].0 = "molang".into();
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
    replacement.width = wire::ZigZag32(2);
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
        odd.width = wire::ZigZag32(-1);
        let pair = if reverse {
            vec![odd, recipe(1)]
        } else {
            vec![recipe(1), odd]
        };
        catalog.apply(9, 2, &update(pair, false));
        assert!(catalog.recipe(1).is_none());
        let mut odd = recipe(2);
        odd.results[0].aux_value = wire::VarUInt(RESULT_AUX_PLACEHOLDER.0);
        // Result aux 65536 is above the schema bound, so it is spliced in after encoding.
        let odd = splice(
            encode(vec![odd], true),
            RESULT_AUX_PLACEHOLDER.1,
            &[0x80, 0x80, 0x04],
        );
        catalog.apply(9, 3, &decode_recipe_update(&odd).unwrap());
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
    r.ingredients[0].aux_value = wire::ZigZag32(-1);
    cases.push(encode(vec![r], true));
    let mut r = recipe(1);
    let duplicate = r.ingredients[0].descriptor[0].clone();
    r.ingredients[0].descriptor.push(duplicate);
    cases.push(encode(vec![r], true));
    let mut r = recipe(1);
    r.ingredients[0].descriptor[0].1 = "invalid name".into();
    cases.push(encode(vec![r], true));
    let mut r = recipe(1);
    r.results[0].user_data_buffer = vec![0xff, 0xff, 1].into();
    cases.push(encode(vec![r], true));
    let mut r = recipe(1);
    r.height = wire::ZigZag32(3);
    cases.push(encode(vec![r], true));
    for bytes in cases {
        let mut catalog = RecipeCatalog::default();
        catalog.begin_session(1);
        let update = decode_recipe_update(&bytes).unwrap();
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
    r.recipe_id = "a".repeat(16385).into();
    cases.push(r);
    let mut r = recipe(1);
    r.results[0].user_data_buffer = vec![0; 65537].into();
    cases.push(r);
    let mut r = recipe(1);
    r.unlocking_requirement = Some(RecipeUnlockRequirementSerializedData {
        unlocking_context: RecipeUnlockingRequirementUnlockingContext::Alwaysunlocked,
        unlocking_ingredients: Some(vec![base.ingredients[0].clone(); 65]),
    });
    cases.push(r);
    for recipe in cases {
        assert!(update(vec![recipe], false).is_unavailable());
    }
}

#[test]
fn all_well_formed_unavailable_families_are_traversed_without_activation() {
    let bytes = CraftingData {
        shapeless_recipes: vec![ShapelessRecipe {
            net_id: RecipeNetID(1),
            ..Default::default()
        }],
        multi_recipes: vec![MultiRecipe {
            net_id: RecipeNetID(2),
            ..Default::default()
        }],
        user_data_shapeless_recipes: vec![Default::default()],
        shapeless_chemistry_recipes: vec![Default::default()],
        shaped_chemistry_recipes: vec![Default::default()],
        smithing_transform_recipes: vec![Default::default()],
        smithing_trim_recipes: vec![Default::default()],
        potion_mixes: vec![Default::default()],
        container_mixes: vec![Default::default()],
        material_reducers: vec![Default::default()],
        clear_recipes: true,
        ..Default::default()
    }
    .encode_to_vec()
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
