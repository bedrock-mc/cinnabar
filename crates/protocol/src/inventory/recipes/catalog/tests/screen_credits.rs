//! Station-only catalogs retain decoded and merged credits across immutable snapshots.

use super::*;
use crate::inventory::recipes::decode::decode_with_credits;
use bedrock_protocol_1_26_51::{
    packets::CraftingData,
    types::{
        NetworkItemInstanceDescriptorSerializedData, RecipeIngredientSerializedData, RecipeNetID,
        ShapelessRecipe,
    },
    wire::{self, Encode},
};

const LIMIT: usize = 65536;

/// Decodes original station records through the same credited admission as ingress.
fn stations(owner: &Arc<Credits>, clear: bool, ids: &[u32], tag: &str) -> RecipeUpdate {
    let body = CraftingData {
        shapeless_recipes: ids
            .iter()
            .map(|id| ShapelessRecipe {
                recipe_id: "test:station".into(),
                ingredients: vec![RecipeIngredientSerializedData {
                    descriptor: vec![("name".into(), "minecraft:stone".into())],
                    aux_value: wire::ZigZag32(0),
                    stack_size: wire::ZigZag32(1),
                }],
                results: vec![NetworkItemInstanceDescriptorSerializedData {
                    id: wire::ZigZag32(9),
                    stack_size: wire::U16LE(1),
                    aux_value: wire::VarUInt(0),
                    block_runtime_id: wire::ZigZag32(0),
                    user_data_buffer: vec![0; 10].into(),
                }],
                tag: tag.into(),
                net_id: RecipeNetID(*id),
                ..Default::default()
            })
            .collect(),
        clear_recipes: clear,
        ..Default::default()
    }
    .encode_to_vec()
    .unwrap();
    decode_with_credits(&body, owner).unwrap()
}

#[test]
fn station_only_catalogs_hold_batch_credit_until_the_last_snapshot_drops() {
    let owner = Credits::isolated(LIMIT);
    let update = stations(&owner, true, &[5], "stonecutter");
    let decoded = owner.used();
    let batch = Arc::downgrade(update.batch.as_ref().unwrap());
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply_with_credits(1, 1, &update, &owner));
    assert!(catalog.recipe(5).is_none());
    let published = owner.used();
    assert!(published > decoded);
    drop(update);
    assert_eq!(owner.used(), published);
    assert!(batch.upgrade().is_some());
    let old = catalog.clone();
    assert_eq!(owner.used(), published);
    assert!(Arc::ptr_eq(
        catalog.storage.as_ref().unwrap(),
        old.storage.as_ref().unwrap()
    ));
    catalog.begin_session(2);
    assert_eq!(owner.used(), published);
    assert_eq!(
        &*old.screen_recipe(5).unwrap().ingredients[0].name,
        "minecraft:stone"
    );
    drop(catalog);
    drop(old);
    assert_eq!(owner.used(), 0);
    assert!(batch.upgrade().is_none());
}

#[test]
fn cow_station_merges_share_old_storage_and_release_each_owner_once() {
    let owner = Credits::isolated(LIMIT);
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    let first = stations(&owner, true, &[5], "stonecutter");
    catalog.apply_with_credits(1, 1, &first, &owner);
    drop(first);
    let original_charge = owner.used();
    let mut current = Arc::new(catalog);
    let old = Arc::clone(&current);
    let second = stations(&owner, false, &[9], "stonecutter");
    assert!(Arc::make_mut(&mut current).apply_with_credits(1, 2, &second, &owner));
    drop(second);
    assert_eq!(
        current
            .screen_recipe_entries()
            .iter()
            .map(|recipe| recipe.id)
            .collect::<Vec<_>>(),
        [5, 9]
    );
    assert!(old.screen_recipe(9).is_none());
    assert!(Arc::ptr_eq(
        &current.screen_recipe(5).unwrap().ingredients[0].name,
        &old.screen_recipe(5).unwrap().ingredients[0].name
    ));
    assert!(owner.used() > original_charge);
    drop(current);
    assert_eq!(owner.used(), original_charge);
    drop(old);
    assert_eq!(owner.used(), 0);
}

#[test]
fn refused_station_merge_retires_current_authority_without_changing_a_snapshot() {
    let owner = Credits::isolated(LIMIT);
    let mut current = RecipeCatalog::default();
    current.begin_session(1);
    let first = stations(&owner, true, &[5], "stonecutter");
    current.apply_with_credits(1, 1, &first, &owner);
    drop(first);
    let old = current.clone();
    let old_charge = owner.used();
    let incoming = stations(&owner, false, &[9], "stonecutter");
    let screen_charge = screens::merged_charge(
        &old.storage.as_ref().unwrap().screen,
        &incoming.batch.as_ref().unwrap().screen,
        &incoming.batch.as_ref().unwrap().records,
    )
    .unwrap();
    let entry_charge = 2 * size_of::<Entry>() + MERGE_METADATA_BYTES;
    let occupied = owner
        .reserve(LIMIT - owner.used() - entry_charge - screen_charge + 1)
        .unwrap();
    assert!(current.apply_with_credits(1, 2, &incoming, &owner));
    assert!(!current.is_available());
    assert!(current.screen_recipe_entries().is_empty());
    assert!(old.screen_recipe(5).is_some());
    assert!(old.screen_recipe(9).is_none());
    drop(occupied);
    drop(incoming);
    assert_eq!(owner.used(), old_charge);
    drop(current);
    drop(old);
    assert_eq!(owner.used(), 0);
}

#[test]
fn numeric_tombstones_retire_station_entries_but_keep_named_batch_credit() {
    for ids in [&[5][..], &[5, 5][..]] {
        let owner = Credits::isolated(LIMIT);
        let mut current = RecipeCatalog::default();
        current.begin_session(1);
        let first = stations(&owner, true, &[5], "stonecutter");
        current.apply_with_credits(1, 1, &first, &owner);
        let name = Arc::downgrade(&current.screen_recipe(5).unwrap().ingredients[0].name);
        drop(first);
        let replacement = stations(
            &owner,
            false,
            ids,
            if ids.len() == 1 {
                "unsupported"
            } else {
                "stonecutter"
            },
        );
        assert!(current.apply_with_credits(1, 2, &replacement, &owner));
        assert!(current.screen_recipe_entries().is_empty());
        assert!(current.recipe_identities().any(|identity| {
            identity.identifier() == "test:station"
                && identity.tag() == "stonecutter"
                && identity.network_id() == 5
        }));
        assert!(name.upgrade().is_some());
        drop(replacement);
        let clear = stations(&owner, true, &[], "stonecutter");
        assert!(current.apply_with_credits(1, 3, &clear, &owner));
        assert!(current.recipe_identities().next().is_none());
        assert!(name.upgrade().is_none());
        drop(clear);
        drop(current);
        assert_eq!(owner.used(), 0);
    }
}
