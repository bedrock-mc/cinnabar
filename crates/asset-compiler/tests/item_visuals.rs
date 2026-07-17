use std::{fs, path::Path};

use asset_compiler::compile_entity_assets;
use assets::{BlockVisualId, ItemVisualDefinitionRoute, ItemVisualId, encode_entity_blob};
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
        ("entity/test.entity.json", br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},"render_controllers":["controller.render.test"]}}}"#),
        ("models/entity/test.geo.json", br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.test"},"bones":[]}]}"#),
        ("animations/empty.animation.json", br#"{"format_version":"1.8.0","animations":{}}"#),
        ("animation_controllers/empty.animation_controllers.json", br#"{"format_version":"1.10.0","animation_controllers":{}}"#),
        ("render_controllers/test.render_controllers.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test":{"geometry":"Geometry.default","textures":["Texture.default"]}}}"#),
        ("textures/entity/test.png", b"entity-raster"),
        ("textures/item_texture.json", br#"{"resource_pack_name":"synthetic","texture_name":"atlas.items","texture_data":{"apple":{"textures":"textures/items/apple"},"apple_copy":{"textures":"textures/items/apple"},"missing":{"textures":"textures/items/missing"},"stone":{"textures":"textures/blocks/stone"}}}"#),
        ("textures/items/apple.png", b"item-raster"),
        ("textures/item_visuals.json", br#"{"block_visual_count":8,"items":{"minecraft:apple":{"texture":"apple","aliases":["minecraft:apple_alias"],"display":{"first_person":{"translation":[1,2,3],"rotation":[0,90,0],"scale":[1,1,1]},"third_person":{"translation":[0,1,0]},"dropped":{"scale":[0.5,0.5,0.5]}}},"minecraft:stone":{"texture":"stone","block_visual":3},"minecraft:missing":{"texture":"missing"},"minecraft:air":{"empty_hand":true}}}"#),
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

#[test]
fn compiles_canonical_sprite_block_missing_empty_and_alias_routes() {
    let first = compile_entity_assets(item_pack(false).path(), MANIFEST).unwrap();
    let second = compile_entity_assets(item_pack(true).path(), MANIFEST).unwrap();
    assert_eq!(
        encode_entity_blob(&first).unwrap(),
        encode_entity_blob(&second).unwrap()
    );
    assert_eq!(first.block_visual_count, 8);
    assert_eq!(
        first
            .item_visuals
            .iter()
            .map(|item| item.identifier.as_ref())
            .collect::<Vec<_>>(),
        vec![
            "minecraft:air",
            "minecraft:apple",
            "minecraft:missing",
            "minecraft:stone"
        ]
    );
    assert_eq!(
        first.item_visuals[0].route,
        ItemVisualDefinitionRoute::EmptyHand
    );
    assert_eq!(first.item_visuals[1].first_person.translation[0].get(), 1.0);
    assert_eq!(first.item_visuals[1].dropped.scale[0].get(), 0.5);
    assert!(matches!(
        first.item_visuals[1].route,
        ItemVisualDefinitionRoute::Sprite { .. }
    ));
    assert_eq!(
        first.item_visuals[2].route,
        ItemVisualDefinitionRoute::Missing
    );
    assert_eq!(
        first.item_visuals[3].route,
        ItemVisualDefinitionRoute::BlockItem {
            block_visual: BlockVisualId(3)
        }
    );
    assert_eq!(first.item_visual_aliases.len(), 2);
    assert_eq!(
        first.item_visual_aliases[0].identifier.as_ref(),
        "minecraft:apple_alias"
    );
    assert_eq!(first.item_visual_aliases[0].visual, ItemVisualId(1));
    assert_eq!(
        first.item_visual_aliases[1].identifier.as_ref(),
        "minecraft:apple_copy"
    );
    assert_eq!(first.item_visual_aliases[1].visual, ItemVisualId(1));
    assert_eq!(
        first.item_visuals[2].source, first.item_visuals[1].source,
        "missing and resolved visuals retain their defining rules source"
    );
}

#[test]
fn rejects_non_finite_display_values_and_out_of_range_block_routes() {
    for sidecar in [
        br#"{"block_visual_count":1,"items":{"minecraft:bad":{"texture":"apple","display":{"first_person":{"translation":["NaN",0,0]}}}}}"#.as_slice(),
        br#"{"block_visual_count":1,"items":{"minecraft:bad":{"texture":"apple","block_visual":1}}}"#.as_slice(),
    ] {
        let pack = item_pack(false);
        write(pack.path(), "textures/item_visuals.json", sidecar);
        assert!(compile_entity_assets(pack.path(), MANIFEST).is_err());
    }
}

#[test]
fn item_texture_atlas_compiles_without_optional_reviewed_block_rules() {
    let pack = item_pack(false);
    fs::remove_file(pack.path().join("textures/item_visuals.json")).unwrap();
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert!(compiled.item_visuals.iter().any(|visual| {
        visual.identifier.as_ref() == "minecraft:air"
            && visual.route == ItemVisualDefinitionRoute::EmptyHand
    }));
    assert!(compiled.item_visuals.iter().any(|visual| {
        visual.identifier.as_ref() == "minecraft:apple"
            && matches!(visual.route, ItemVisualDefinitionRoute::Sprite { .. })
    }));
    assert!(compiled.item_visuals.iter().any(|visual| {
        visual.identifier.as_ref() == "minecraft:missing"
            && visual.route == ItemVisualDefinitionRoute::Missing
    }));
}
