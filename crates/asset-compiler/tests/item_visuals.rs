use std::{fs, path::Path};

use asset_compiler::compile_entity_assets;
use assets::{
    ItemTextureReference, ItemVisualDefinition, ItemVisualDefinitionRoute, encode_entity_blob,
};
use tempfile::TempDir;

const MANIFEST: &[u8] = include_bytes!("../../../assets/vanilla-source.json");

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn item_pack(reverse: bool) -> TempDir {
    let temporary = tempfile::tempdir().unwrap();
    let files: [(&str, &[u8]); 9] = [
        ("entity/item.entity.json", br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:item","geometry":{"default":"geometry.item"},"render_controllers":["controller.render.item"]}}}"#),
        ("models/entity/item.geo.json", br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.item"},"bones":[{"name":"root"}]}]}"#),
        ("animations/empty.json", br#"{"format_version":"1.8.0","animations":{}}"#),
        ("animation_controllers/empty.json", br#"{"format_version":"1.10.0","animation_controllers":{}}"#),
        ("render_controllers/item.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.item":{"geometry":"Geometry.default"}}}"#),
        ("textures/entity/item.png", b"entity-raster"),
        ("textures/item_texture.json", br#"{"resource_pack_name":"synthetic","texture_name":"atlas.items","texture_data":{"apple":{"textures":["textures/items/apple","textures/items/apple_alt"]},"missing":{"textures":"textures/items/missing"},"stone":{"textures":"textures/blocks/stone"}}}"#),
        ("textures/items/apple.png", b"apple-raster"),
        ("textures/items/apple_alt.png", b"apple-alt-raster"),
    ];
    let iterator: Box<dyn Iterator<Item = &(&str, &[u8])>> = if reverse {
        Box::new(files.iter().rev())
    } else {
        Box::new(files.iter())
    };
    for (path, bytes) in iterator {
        write(temporary.path(), path, bytes);
    }
    temporary
}

fn visual<'a>(
    visuals: &'a [ItemVisualDefinition],
    identifier: &str,
    metadata: u32,
) -> &'a ItemVisualDefinition {
    visuals
        .iter()
        .find(|visual| {
            visual.key.identifier.as_ref() == identifier && visual.key.metadata == metadata
        })
        .unwrap()
}

#[test]
fn compiles_exact_metadata_variants_reviewed_block_routes_and_missing_items() {
    let first = compile_entity_assets(item_pack(false).path(), MANIFEST).unwrap();
    let second = compile_entity_assets(item_pack(true).path(), MANIFEST).unwrap();
    assert_eq!(
        encode_entity_blob(&first).unwrap(),
        encode_entity_blob(&second).unwrap()
    );
    assert_eq!(first.block_visual_count, 16_913);
    assert!(matches!(
        visual(&first.item_visuals, "minecraft:air", 0).route,
        ItemVisualDefinitionRoute::EmptyHand
    ));
    assert!(matches!(
        visual(&first.item_visuals, "minecraft:apple", 0).route,
        ItemVisualDefinitionRoute::Sprite {
            texture: ItemTextureReference { variant: 0, .. }
        }
    ));
    assert!(matches!(
        visual(&first.item_visuals, "minecraft:apple", 1).route,
        ItemVisualDefinitionRoute::Sprite {
            texture: ItemTextureReference { variant: 1, .. }
        }
    ));
    assert!(matches!(
        visual(&first.item_visuals, "minecraft:stone", 0).route,
        ItemVisualDefinitionRoute::BlockItem { .. }
    ));
    assert!(matches!(
        visual(&first.item_visuals, "minecraft:missing", 0).route,
        ItemVisualDefinitionRoute::Missing
    ));
    assert!(first.item_visual_aliases.is_empty());
}

#[test]
fn item_texture_variants_must_be_nonempty_strings() {
    for textures in [
        serde_json::json!([]),
        serde_json::json!(["textures/items/apple", 3]),
    ] {
        let pack = item_pack(false);
        let atlas = serde_json::json!({
            "resource_pack_name": "synthetic",
            "texture_name": "atlas.items",
            "texture_data": {"apple": {"textures": textures}}
        });
        write(
            pack.path(),
            "textures/item_texture.json",
            &serde_json::to_vec(&atlas).unwrap(),
        );
        assert!(compile_entity_assets(pack.path(), MANIFEST).is_err());
    }
}

#[test]
fn no_pack_sidecar_is_required_or_consumed() {
    let pack = item_pack(false);
    write(
        pack.path(),
        "textures/item_visuals.json",
        br#"{"invented":"policy"}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert!(matches!(
        visual(&compiled.item_visuals, "minecraft:stone", 0).route,
        ItemVisualDefinitionRoute::BlockItem { .. }
    ));
    assert!(
        !compiled
            .sources
            .iter()
            .any(|source| source.path.as_ref() == "textures/item_visuals.json")
    );
}

#[test]
fn reviewed_block_routes_and_empty_hand_do_not_depend_on_an_item_atlas() {
    let pack = item_pack(false);
    fs::remove_file(pack.path().join("textures/item_texture.json")).unwrap();
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let stone = visual(&compiled.item_visuals, "minecraft:stone", 0);
    assert!(matches!(
        stone.route,
        ItemVisualDefinitionRoute::BlockItem { .. }
    ));
    assert!(matches!(
        visual(&compiled.item_visuals, "minecraft:air", 0).route,
        ItemVisualDefinitionRoute::EmptyHand
    ));
    assert_eq!(
        compiled.sources[stone.source as usize].path.as_ref(),
        "registry/block-item-routes-v1001.json"
    );
}
