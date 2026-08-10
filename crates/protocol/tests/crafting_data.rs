use bytes::Bytes;
use jolyne::batch::{decode_batch_raw, encode_batch_multi};
use jolyne::valentine::{McpePacketArgs, McpePacketData};
use protocol::BedrockSession;

// Regenerated for protocol 2168 from gophertunnel at commit
// be6713da4dc051a4197f897d04835e89e9c54321:
//
//   packet.CraftingData{ShapedRecipes: []protocol.ShapedRecipe{{
//       RecipeID: "x", Width: 1, Height: 1,
//       Input:  []protocol.ItemDescriptorCount{{
//           Descriptor: &protocol.DefaultItemDescriptor{Name: "minecraft:stone"},
//           Count:      1}},
//       Output: []protocol.ItemStack{{
//           ItemType: protocol.ItemType{NetworkID: 7}, Count: 1}},
//       Block: "crafting_table", RecipeNetworkID: 1,
//   }}}
//
// Two things changed against protocol 1001. `CraftingData` no longer carries a
// single fused `Recipes` slice: `packet/crafting_data.go` marshals eight typed
// recipe vectors (shaped, shapeless, multi, the reserved vectors at positions
// 4 and 5, smithing transform, and smithing trim) before the potion,
// potion-container and reserved position-10 vectors, so a lone shaped recipe
// now costs seven extra empty counts on the wire. And shaped ingredients are a
// `FuncSlice` with its own varuint32 length prefix (`marshalShaped`,
// minecraft/protocol/recipe.go); the protocol-1001 "exactly width*height
// descriptors, no length prefix" layout is gone.
// SHA-256: 4da7bde1453aa62d17a660c9481e4827bdb9b3c3ee54f648b5209245a80ed9d9
const GOPHERTUNNEL_ONE_CELL_SHAPED_RECIPE: &[u8] = &[
    0xfe, 0x5f, 0xb4, 0x48, 0x01, 0x01, 0x78, 0x02, 0x02, 0x01, 0x01, 0x04, 0x6e, 0x61, 0x6d, 0x65,
    0x0f, 0x6d, 0x69, 0x6e, 0x65, 0x63, 0x72, 0x61, 0x66, 0x74, 0x3a, 0x73, 0x74, 0x6f, 0x6e, 0x65,
    0x00, 0x02, 0x01, 0x0e, 0x01, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x0e, 0x63, 0x72, 0x61, 0x66, 0x74, 0x69, 0x6e, 0x67, 0x5f, 0x74, 0x61, 0x62,
    0x6c, 0x65, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00,
];

fn raw_crafting_data() -> jolyne::raw::RawPacket {
    let mut batch = Bytes::from_static(GOPHERTUNNEL_ONE_CELL_SHAPED_RECIPE);
    decode_batch_raw(&mut batch, false, Some(1024))
        .expect("raw batch decode")
        .into_iter()
        .next()
        .expect("one packet")
}

fn assert_one_cell_shaped_recipe(data: &McpePacketData) {
    let McpePacketData::CraftingDataPacket(packet) = data else {
        panic!("expected CraftingData, got {:?}", data.packet_id());
    };
    // The recipe lands in the typed `shaped_recipes` vector; every other typed
    // vector must stay empty, which is what pins the eight-vector wire order.
    assert_eq!(packet.shaped_recipes.len(), 1);
    assert!(packet.shapeless_recipes.is_empty());
    assert!(packet.multi_recipes.is_empty());
    assert!(packet.user_data_shapeless_recipes.is_empty());
    assert!(packet.reserved_recipes_4.is_empty());
    assert!(packet.reserved_recipes_5.is_empty());
    assert!(packet.smithing_transform_recipes.is_empty());
    assert!(packet.smithing_trim_recipes.is_empty());
    assert!(packet.potion_mixes.is_empty());
    assert!(packet.container_mixes.is_empty());
    assert!(packet.reserved_entries_10.is_empty());
    assert!(!packet.clear_recipes);

    let recipe = &packet.shaped_recipes[0];
    assert_eq!(recipe.recipe_id, "x");
    assert_eq!((recipe.width, recipe.height), (1, 1));
    assert_eq!(recipe.ingredients.len(), 1);
    assert_eq!(recipe.results.len(), 1);
    assert_eq!(recipe.results[0].id, 7);
    // gophertunnel's `Block` is `tag` in the 1.26.40 generated crate.
    assert_eq!(recipe.tag, "crafting_table");
    assert_eq!(recipe.net_id.raw_id, 1);
}

#[test]
fn pinned_gophertunnel_shaped_recipe_owned_decodes_and_round_trips_exactly() {
    let packet = raw_crafting_data()
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect("owned CraftingData decode");
    assert_one_cell_shaped_recipe(&packet.data);
    let encoded = encode_batch_multi(&[packet], false, 0, 0, true).expect("re-encode");
    assert_eq!(encoded.as_ref(), GOPHERTUNNEL_ONE_CELL_SHAPED_RECIPE);
}

#[test]
fn pinned_gophertunnel_shaped_recipe_borrowed_materializes() {
    let borrowed = raw_crafting_data()
        .decode_borrowed()
        .expect("borrowed CraftingData decode");
    let owned = borrowed
        .data
        .into_owned(McpePacketArgs)
        .expect("materialize borrowed CraftingData");
    assert_one_cell_shaped_recipe(&owned);
}

/// Shaped-recipe dimensions can no longer drive an allocation.
///
/// RETARGETED. Protocol 1001 modelled shaped ingredients as an implicit
/// `width * height` run with no length prefix, so a hostile `width = -1,
/// height = -1` pair (product `1`, but each factor negative) was a real
/// over-read/over-allocation lever and the old test only checked that encoding
/// such a recipe was refused. gophertunnel's `marshalShaped`
/// (minecraft/protocol/recipe.go at be6713da4dc051a4197f897d04835e89e9c54321)
/// writes `FuncSlice(r, &recipe.Input, r.ItemDescriptorCount)`, so the
/// ingredient count is now explicit on the wire and the dimensions are pure
/// metadata. The premise of the old assertion is gone, so this pins the
/// property that made it matter instead: absurd dimensions do not change how
/// many ingredients are read, and the frame still round-trips byte for byte.
///
/// Note that gophertunnel additionally rejects `len(Input) != Width*Height`
/// via `r.InvalidValue`, a consistency check the 1.26.40 generated crate does
/// not emit on either the encode or the decode path.
#[test]
fn shaped_recipe_dimensions_do_not_drive_the_ingredient_count() {
    let mut packet = raw_crafting_data()
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect("owned CraftingData decode");
    let McpePacketData::CraftingDataPacket(content) = &mut packet.data else {
        panic!("expected CraftingData");
    };
    let recipe = &mut content.shaped_recipes[0];
    recipe.width = -1;
    recipe.height = -1;

    let encoded =
        encode_batch_multi(&[packet], false, 0, 0, true).expect("negative dimensions still encode");

    let mut batch = encoded.clone();
    let round_tripped = decode_batch_raw(&mut batch, false, Some(1024))
        .expect("raw batch decode")
        .into_iter()
        .next()
        .expect("one packet")
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect("negative dimensions must not derail the ingredient read");

    let McpePacketData::CraftingDataPacket(content) = &round_tripped.data else {
        panic!("expected CraftingData");
    };
    let recipe = &content.shaped_recipes[0];
    assert_eq!((recipe.width, recipe.height), (-1, -1));
    assert_eq!(
        recipe.ingredients.len(),
        1,
        "the explicit ingredient count, not width*height, decides the read"
    );
    assert_eq!(recipe.results.len(), 1);
    assert_eq!(recipe.tag, "crafting_table");

    let re_encoded =
        encode_batch_multi(&[round_tripped], false, 0, 0, true).expect("stable re-encode");
    assert_eq!(re_encoded, encoded);
}
