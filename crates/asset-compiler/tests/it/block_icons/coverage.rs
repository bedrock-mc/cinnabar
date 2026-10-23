//! Pinned-pack coverage: every registry item draws an icon unless it is named here with a reason.

use super::*;

const VOID: &str = "its GUI shape has no geometry in the world carrier";

/// Registry items without an icon, each with the reason it stays unresolved.
const UNRESOLVED: [(&str, &str); 1] = [("minecraft:structure_void", VOID)];

fn pinned_icons() -> Option<RuntimeIconCatalog> {
    let (Some(pack), Some(world)) = (
        crate::fixture_input::env_path("PINNED_VANILLA_PACK"),
        crate::fixture_input::env_path("PINNED_WORLD_CARRIER"),
    ) else {
        return None;
    };
    let world = RuntimeAssets::decode(&fs::read(world).unwrap()).unwrap();
    let pack = fs::canonicalize(pack).unwrap();
    let compiled = compile_icon_assets_with_blocks(&pack, MANIFEST, &world).unwrap();
    Some(RuntimeIconCatalog::decode(&compiled.bytes).unwrap())
}

#[test]
fn every_pinned_registry_item_has_an_icon() {
    let Some(catalog) = pinned_icons() else {
        return;
    };
    let missing = assets::item::names::identifiers()
        .filter(|identifier| catalog.lookup_index(identifier, 0).is_none())
        .collect::<Vec<_>>();
    let expected = UNRESOLVED.map(|(identifier, _)| identifier);
    assert_eq!(missing, expected, "registry items without an icon");
}

/// Legacy and icon-component sprites beat block models, and tipped arrows show their potion.
#[test]
fn pinned_sprite_items_draw_their_own_texture() {
    let Some(catalog) = pinned_icons() else {
        return;
    };
    let source = |path: &str| {
        catalog
            .sources()
            .unwrap()
            .iter()
            .find(|source| source.path.as_ref() == path)
            .map(|source| source.sprite as usize)
            .unwrap_or_else(|| panic!("no sprite source {path}"))
    };
    for (identifier, metadata, path) in [
        ("minecraft:poplar_door", 0, "textures/items/poplar_door.png"),
        (
            "minecraft:poplar_hanging_sign",
            0,
            "textures/items/poplar_hanging_sign.png",
        ),
        ("minecraft:poplar_boat", 0, "textures/items/poplar_boat.png"),
        ("minecraft:red_cushion", 0, "textures/items/red_cushion.png"),
        ("minecraft:red_shrub", 0, "textures/blocks/red_shrub.png"),
        ("minecraft:filled_map", 0, "textures/items/map_filled.png"),
        ("minecraft:arrow", 0, "textures/items/arrow.png"),
        ("minecraft:arrow", 1, "textures/items/tipped_arrow.png"),
        (
            "minecraft:arrow",
            15,
            "textures/items/tipped_arrow_swift.png",
        ),
        (
            "minecraft:arrow",
            47,
            "textures/items/tipped_arrow_infested.png",
        ),
    ] {
        assert_eq!(
            catalog.lookup_index(identifier, metadata),
            Some(source(path)),
            "{identifier}:{metadata}"
        );
    }
    // Flat block icons show one frame of a flipbook strip and the light block's own art.
    let vein = catalog.lookup("minecraft:sculk_vein", 0).unwrap();
    assert_eq!([vein.width, vein.height], [16, 16]);
    let light = catalog.lookup("minecraft:light_block_15", 0).unwrap();
    assert_eq!([light.width, light.height], [32, 32]);
}
