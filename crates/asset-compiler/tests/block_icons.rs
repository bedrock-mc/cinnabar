#[path = "support/fixture_input.rs"]
mod fixture_input;

use assets::*;
use pack_compiler::{compile_entity_assets, compile_icon_assets, compile_icon_assets_with_blocks};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

const MANIFEST: &[u8] = include_bytes!("../../../assets/vanilla-source.json");

#[path = "block_icons/carried.rs"]
mod carried;

fn write(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn pack() -> tempfile::TempDir {
    let pack = tempfile::tempdir().unwrap();
    for (path, bytes) in [
        ("entity/item.json", &br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:item","geometry":{"default":"geometry.item"},"render_controllers":["controller.render.item"]}}}"#[..]),
        ("models/entity/item.json", &br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.item"},"bones":[{"name":"root"}]}]}"#[..]),
        ("animations/empty.json", &br#"{"format_version":"1.8.0","animations":{}}"#[..]),
        ("animation_controllers/empty.json", &br#"{"format_version":"1.10.0","animation_controllers":{}}"#[..]),
        ("render_controllers/item.json", &br#"{"format_version":"1.8.0","render_controllers":{"controller.render.item":{"geometry":"Geometry.default"}}}"#[..]),
        ("textures/entity/item.png", &b"entity-raster"[..]),
        ("textures/item_texture.json", &br#"{"resource_pack_name":"synthetic","texture_name":"atlas.items","texture_data":{"bundle_blue":{"textures":"textures/items/shared"},"bundle_light_blue":{"textures":"textures/items/shared"}}}"#[..]),
    ] { write(pack.path(), path, bytes); }
    fs::create_dir_all(pack.path().join("textures/items")).unwrap();
    image::save_buffer(
        pack.path().join("textures/items/shared.png"),
        &[20, 80, 160, 255],
        1,
        1,
        image::ColorType::Rgba8,
    )
    .unwrap();
    pack
}

fn world(entity: &CompiledEntityAssets) -> CompiledAssets {
    let count = entity.block_visual_count as usize;
    let mut visuals =
        vec![BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary); count];
    for name in [
        "minecraft:stone",
        "minecraft:dirt",
        "minecraft:crafting_table",
    ] {
        let route = entity
            .item_visuals
            .iter()
            .find(|v| v.key.identifier.as_ref() == name && v.key.metadata == 0)
            .unwrap();
        let ItemVisualDefinitionRoute::BlockItem { block_visual } = route.route else {
            panic!("real block route required")
        };
        visuals[block_visual.0 as usize] = BlockVisual {
            faces: [1, 2, 3, 4, 5, 6],
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            kind: VisualKind::Cube,
            support: VisualSupport::Exact,
            contributor_role: ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        };
    }
    let materials = (0..7)
        .map(|layer| Material {
            texture: TextureRef::new(0, layer).unwrap(),
            flags: 0,
            animation: NO_ANIMATION,
            ..assets::Material::unvaried()
        })
        .collect::<Vec<_>>();
    let page = TexturePage::new(TextureArray {
        layers: 7,
        mips: [16, 8, 4, 2, 1]
            .into_iter()
            .map(|size| {
                let mut pixels = Vec::new();
                for layer in 0..7 {
                    for y in 0..size {
                        for x in 0..size {
                            pixels.extend_from_slice(&[
                                (30 + layer * 25) as u8,
                                x as u8 * 10,
                                y as u8 * 10,
                                255,
                            ]);
                        }
                    }
                }
                TextureMip {
                    size,
                    rgba8: pixels.into(),
                }
            })
            .collect::<Vec<_>>()
            .into(),
    });
    CompiledAssets {
        visuals: visuals.into(),
        light_properties: vec![LightProperties::default(); count].into(),
        hashed: Box::new([]),
        materials: materials.into(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![page].into(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: BlobProvenance {
            source_manifest_sha256: entity.source_manifest_sha256,
            block_registry_sha256: Sha256::digest(include_bytes!(
                "../../assets/data/block-registry-v2193.bin"
            ))
            .into(),
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    }
}

fn runtime(source: &CompiledAssets) -> RuntimeAssets {
    RuntimeAssets::decode(&encode_blob(source).unwrap()).unwrap()
}

#[test]
fn real_block_routes_compile_decode_lookup_preserving_original_sprites_and_aliases() {
    let pack = pack();
    let entity = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let source = world(&entity);
    let world = runtime(&source);
    let old = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    let new = compile_icon_assets_with_blocks(pack.path(), MANIFEST, &world).unwrap();
    assert_eq!(
        new.bytes,
        compile_icon_assets_with_blocks(pack.path(), MANIFEST, &world)
            .unwrap()
            .bytes
    );
    let old = RuntimeIconCatalog::decode(&old.bytes).unwrap();
    let catalog = RuntimeIconCatalog::decode(&new.bytes).unwrap();
    assert_eq!(&catalog.sprites()[..old.sprites().len()], old.sprites());
    for entry in old.entries() {
        assert_eq!(
            catalog.lookup_index(&entry.identifier, entry.metadata),
            Some(usize::try_from(entry.sprite).unwrap())
        );
    }
    for name in [
        "minecraft:stone",
        "minecraft:dirt",
        "minecraft:crafting_table",
    ] {
        assert!(old.lookup_index(name, 0).is_none());
        let index = catalog.lookup_index(name, 0).unwrap();
        assert_eq!(catalog.lookup_index(name, 99), Some(index));
        let sprite = &catalog.sprites()[index as usize];
        assert_eq!((sprite.width, sprite.height), (16, 16));
        assert!(sprite.rgba8.chunks_exact(4).any(|p| p[3] == 0));
        assert!(sprite.rgba8.chunks_exact(4).any(|p| p[3] == 255));
        for red in [130, 90, 40] {
            assert!(
                sprite
                    .rgba8
                    .chunks_exact(4)
                    .any(|pixel| pixel[0] == red && pixel[3] == 255),
                "top, south, and west must retain their distinct checked face pixels"
            );
        }
    }
    assert_eq!(
        catalog.sprites().len(),
        old.sprites().len() + 1,
        "identical actual six-face pixels deduplicate"
    );
    assert_eq!(new.report.block_policy, Some("opaque-cube-thumbnail-v1"));
    assert!(new.report.skipped_blocks > 0);
    assert_eq!(
        encode_icon_catalog(
            catalog.source_manifest_sha256(),
            catalog.sprites(),
            catalog.entries()
        )
        .unwrap(),
        new.bytes
    );
}

#[test]
fn provenance_and_hidden_face_eligibility_are_fail_closed() {
    let pack = pack();
    let entity = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    for field in 0..3 {
        let mut source = world(&entity);
        match field {
            0 => source.provenance.source_manifest_sha256 = [9; 32],
            1 => source.provenance.block_registry_sha256 = [9; 32],
            _ => {
                source.visuals = source.visuals[..source.visuals.len() - 1].into();
                source.light_properties =
                    source.light_properties[..source.light_properties.len() - 1].into();
            }
        }
        assert!(compile_icon_assets_with_blocks(pack.path(), MANIFEST, &runtime(&source)).is_err());
    }
    let mut source = world(&entity);
    // East is invisible in this thumbnail, but still must be admitted.
    source.materials[2].flags = assets::MATERIAL_FLAG_ROTATE_UV;
    let result = compile_icon_assets_with_blocks(pack.path(), MANIFEST, &runtime(&source)).unwrap();
    assert_eq!(result.report.block_visuals, 0);
    assert!(result.report.block_refusals[1] >= 3);
    let mut source = world(&entity);
    source.texture_pages[0].texture.mips[0].rgba8[16 * 16 * 4 * 2 + 3] = 0;
    let result = compile_icon_assets_with_blocks(pack.path(), MANIFEST, &runtime(&source)).unwrap();
    // The opaque-cube path refuses the hole; the cutout model path draws it instead.
    assert_eq!(
        result.report.block_visuals,
        result.report.model_block_visuals
    );
    assert_eq!(result.report.block_visuals, 3);
}

#[test]
fn command_world_input_reencodes_and_refuses_output_collision_atomically() {
    let pack = pack();
    let entity = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let world = world(&entity);
    let input = pack.path().join("world.mcbea");
    let manifest = pack.path().join("vanilla-source.json");
    let output = pack.path().join("icons.mcbeico");
    let report = pack.path().join("icons.json");
    fs::write(&input, encode_blob(&world).unwrap()).unwrap();
    fs::write(&manifest, MANIFEST).unwrap();
    let run = |report: &Path| {
        std::process::Command::new(env!("CARGO_BIN_EXE_assetc"))
            .args(["icon-assets", "--pack"])
            .arg(pack.path())
            .args(["--source-manifest"])
            .arg(&manifest)
            .args(["--block-assets"])
            .arg(&input)
            .args(["--out"])
            .arg(&output)
            .args(["--report"])
            .arg(report)
            .output()
            .unwrap()
    };
    assert!(run(&report).status.success());
    let first = fs::read(&output).unwrap();
    let report_bytes = fs::read(&report).unwrap();
    assert!(
        report_bytes
            .windows(b"opaque-cube-thumbnail-v1".len())
            .any(|window| window == b"opaque-cube-thumbnail-v1")
    );
    assert_eq!(
        RuntimeIconCatalog::decode(&first)
            .unwrap()
            .lookup_index("minecraft:stone", 0),
        Some(1)
    );
    assert!(run(&report).status.success());
    assert_eq!(fs::read(&output).unwrap(), first);
    assert_eq!(fs::read(&report).unwrap(), report_bytes);
    assert!(!run(&output).status.success());
    assert_eq!(fs::read(&output).unwrap(), first);
}

#[test]
fn legacy_sprite_only_accepts_valid_carrier_despite_many_missing_keys() {
    let pack = pack();
    let template = pack.path().join("fixture.png");
    image::save_buffer(
        &template,
        &vec![255; 64 * 64 * 4],
        64,
        64,
        image::ColorType::Rgba8,
    )
    .unwrap();
    let encoded_png = fs::read(template).unwrap();
    let mut texture_data = serde_json::Map::new();
    for index in 0..900 {
        let name = format!("s{index:04}");
        let path = format!("textures/items/{name}");
        write(pack.path(), &format!("{path}.png"), &encoded_png);
        texture_data.insert(name, serde_json::json!({"textures":path}));
    }
    for index in 0..9000 {
        let name = format!("missing_{index:05}_{}", "x".repeat(230));
        assert!(format!("minecraft:{name}").len() <= MAX_ICON_KEY_BYTES);
        texture_data.insert(
            name,
            serde_json::json!({"textures":"textures/items/absent"}),
        );
    }
    write(pack.path(),"textures/item_texture.json",&serde_json::to_vec(&serde_json::json!({
        "resource_pack_name":"synthetic", "texture_name":"atlas.items", "texture_data":texture_data
    })).unwrap());
    let output = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    let catalog = RuntimeIconCatalog::decode(&output.bytes).unwrap();
    assert_eq!(catalog.sprites().len(), 900);
    assert!(output.bytes.len() < MAX_ICON_CARRIER_BYTES);
    assert!(
        catalog
            .lookup_index(&format!("minecraft:missing_00000_{}", "x".repeat(230)), 0)
            .is_none()
    );
    assert!(catalog.lookup_index("minecraft:s0000", 0).is_some());
    // The conservative world-aware estimate counts these 9000 valid missing
    // identifiers, but the legacy carrier never serializes them.
    assert!(9000 * (10 + 254) + output.bytes.len() > MAX_ICON_CARRIER_BYTES);
}

// Pinned-pack coverage: plants draw flat, sprite items beat their block routes; prints leftovers.
#[test]
fn pinned_block_items_resolve_icons_when_requested() {
    let (Some(pack), Some(world)) = (
        crate::fixture_input::env_path("PINNED_VANILLA_PACK"),
        crate::fixture_input::env_path("PINNED_WORLD_CARRIER"),
    ) else {
        return;
    };
    let world = RuntimeAssets::decode(&fs::read(world).unwrap()).unwrap();
    let pack = fs::canonicalize(pack).unwrap();
    let compiled = compile_icon_assets_with_blocks(&pack, MANIFEST, &world).unwrap();
    let catalog = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
    let missing = [
        "poppy",
        "dandelion",
        "red_tulip",
        "orange_tulip",
        "white_tulip",
        "pink_tulip",
        "cornflower",
        "allium",
        "azure_bluet",
        "blue_orchid",
        "oxeye_daisy",
        "lily_of_the_valley",
        "torchflower",
        "oak_sapling",
        "cherry_sapling",
        "brown_mushroom",
        "red_mushroom",
        "fern",
        "short_grass",
        "deadbush",
        "wooden_door",
        "oak_sign",
        "carrot",
        "oak_leaves",
        "oak_fence",
        "cobblestone_wall",
        "oak_slab",
        "grass_block",
    ]
    .into_iter()
    .filter(|name| {
        catalog
            .lookup_index(&format!("minecraft:{name}"), 0)
            .is_none()
    })
    .collect::<Vec<_>>();
    eprintln!(
        "{} flat and {} model block icons; {} block items unresolved: {:?}",
        compiled.report.flat_block_visuals,
        compiled.report.model_block_visuals,
        compiled.report.unresolved_block_items.len(),
        compiled.report.unresolved_block_items
    );
    assert!(missing.is_empty(), "no icon: {missing:?}");
    let ItemVisualDefinitionRoute::BlockItem { block_visual } =
        compile_entity_assets(&pack, MANIFEST)
            .unwrap()
            .item_visuals
            .iter()
            .find(|visual| visual.key.identifier.as_ref() == "minecraft:grass_block")
            .unwrap()
            .route
    else {
        panic!("pinned grass block item route");
    };
    let binding = catalog
        .block_sheets()
        .iter()
        .find(|sheet| sheet.visual == block_visual)
        .unwrap();
    let sprite = &catalog.sprites()[binding.sprite as usize];
    assert_eq!([sprite.width, sprite.height], BLOCK_ITEM_SHEET_SIZE);
    assert!(sprite.rgba8.chunks_exact(4).all(|pixel| pixel[3] == 255));
}
