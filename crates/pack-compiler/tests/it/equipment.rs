//! Full-pipeline equipment ingestion: synthetic pack -> bindings + populated
//! item transforms -> hash-pinned equipment carrier round-trip.

use std::{fs, path::Path};

use assets::{
    ArmorSlot, EntityDependencyResolution, EquipmentCategory, EquipmentTransform,
    ItemVisualDefinition, RuntimeEquipmentCatalog, encode_entity_blob, encode_equipment_catalog,
};
use pack_compiler::compile_entity_assets_with_report;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

mod texture_resolution;

const MANIFEST: &[u8] = include_bytes!("../../../../assets/vanilla-source.json");

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn equipment_pack() -> TempDir {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    write(root, "entity/item.entity.json", br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:item","geometry":{"default":"geometry.item"},"render_controllers":["controller.render.item"]}}}"#);
    write(root, "models/entity/item.geo.json", br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.item"},"bones":[{"name":"root"}]}]}"#);
    write(root, "models/entity/trident.geo.json", br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.trident"},"bones":[{"name":"pole"}]}]}"#);
    write(root, "models/entity/player_armor.json", br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.player.armor.helmet"},"bones":[{"name":"helmet"}]}]}"#);
    write(
        root,
        "animations/empty.json",
        br#"{"format_version":"1.8.0","animations":{}}"#,
    );
    write(root, "animations/trident.animation.json", br#"{"format_version":"1.10.0","animations":{"animation.trident.wield_first_person":{"loop":true,"bones":{"pole":{"position":[-7.0,-3.0,-2.0],"rotation":[152.0,-9.0,25.0]}}},"animation.trident.wield_third_person":{"loop":true,"bones":{"pole":{"position":[1.5,-2.5,-10.5],"rotation":[97.0,-1.5,-49.0]}}}}}"#);
    write(
        root,
        "animation_controllers/empty.json",
        br#"{"format_version":"1.10.0","animation_controllers":{}}"#,
    );
    write(root, "render_controllers/item.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.item":{"geometry":"Geometry.default"}}}"#);
    write(root, "textures/entity/item.png", b"entity-raster");
    write(root, "textures/entity/trident.png", b"trident-raster");
    write(root, "textures/models/armor/diamond_1.png", b"armor-raster");
    write(root, "textures/item_texture.json", br#"{"resource_pack_name":"synthetic","texture_name":"atlas.items","texture_data":{"trident":{"textures":"textures/items/trident"}}}"#);
    write(root, "textures/items/trident.png", b"trident-icon");
    write(root, "attachables/trident.entity.json", br#"{"format_version":"1.10","minecraft:attachable":{"description":{"identifier":"minecraft:trident","materials":{"default":"entity_alphatest"},"textures":{"default":"textures/entity/trident"},"geometry":{"default":"geometry.trident"},"animations":{"wield_first_person":"animation.trident.wield_first_person","wield_third_person":"animation.trident.wield_third_person"},"render_controllers":["controller.render.item_default"]}}}"#);
    write(root, "attachables/diamond_helmet.json", br#"{"format_version":"1.8.0","minecraft:attachable":{"description":{"identifier":"minecraft:diamond_helmet","materials":{"default":"armor"},"textures":{"default":"textures/models/armor/diamond_1"},"geometry":{"default":"geometry.humanoid.armor.helmet"},"render_controllers":["controller.render.armor"]}}}"#);
    write(root, "attachables/diamond_helmet.player.json", br#"{"format_version":"1.10.0","minecraft:attachable":{"description":{"identifier":"minecraft:diamond_helmet.player","item":{"minecraft:diamond_helmet":"query.owner_identifier == 'minecraft:player'"},"materials":{"default":"armor"},"textures":{"default":"textures/models/armor/diamond_1"},"geometry":{"default":"geometry.player.armor.helmet"},"animations":{"offset":"animation.armor.helmet.offset"},"render_controllers":["controller.render.armor"]}}}"#);
    temporary
}

fn visual<'a>(visuals: &'a [ItemVisualDefinition], identifier: &str) -> &'a ItemVisualDefinition {
    visuals
        .iter()
        .find(|visual| visual.key.identifier.as_ref() == identifier && visual.key.metadata == 0)
        .unwrap()
}

#[test]
fn ingests_bindings_populates_transforms_and_round_trips_the_carrier() {
    let compilation = compile_entity_assets_with_report(equipment_pack().path(), MANIFEST).unwrap();
    let bindings = &compilation.equipment_bindings;
    assert_eq!(bindings.len(), 2);

    let trident = bindings
        .iter()
        .find(|binding| binding.identifier.as_ref() == "minecraft:trident")
        .unwrap();
    assert_eq!(trident.category, EquipmentCategory::Held);
    assert_eq!(
        trident.geometry.resolution,
        EntityDependencyResolution::Catalog
    );
    assert_eq!(
        trident.texture.resolution,
        EntityDependencyResolution::Catalog
    );
    let literal = trident.first_person.literal().unwrap();
    assert_eq!(literal.translation.map(|s| s.get()), [-7.0, -3.0, -2.0]);
    assert_eq!(literal.rotation.map(|s| s.get()), [152.0, -9.0, 25.0]);

    let helmet = bindings
        .iter()
        .find(|binding| binding.identifier.as_ref() == "minecraft:diamond_helmet")
        .unwrap();
    assert_eq!(
        helmet.category,
        EquipmentCategory::Armor {
            slot: ArmorSlot::Helmet
        }
    );
    // Player variant won: its geometry is the one collected into the catalog.
    assert_eq!(
        helmet.geometry.identifier.as_ref(),
        "geometry.player.armor.helmet"
    );
    assert_eq!(
        helmet.geometry.resolution,
        EntityDependencyResolution::Catalog
    );
    assert!(matches!(
        helmet.first_person,
        EquipmentTransform::NeedsMeasurement
    ));

    // The metadata-0 item visual carries the trident's literal transform.
    let trident_visual = visual(&compilation.assets.item_visuals, "minecraft:trident");
    assert_eq!(
        trident_visual.third_person.translation.map(|s| s.get()),
        [1.5, -2.5, -10.5]
    );
    // An unrelated item keeps identity transforms.
    let item_visual = visual(&compilation.assets.item_visuals, "minecraft:air");
    assert_eq!(
        item_visual.first_person,
        assets::ItemDisplayTransform::identity()
    );

    // Carrier pins the sibling entity blob and round-trips.
    let entity_blob = encode_entity_blob(&compilation.assets).unwrap();
    let entity_sha: [u8; 32] = Sha256::digest(&entity_blob).into();
    let carrier = encode_equipment_catalog(
        compilation.assets.source_manifest_sha256,
        entity_sha,
        bindings,
    )
    .unwrap();
    let catalog = RuntimeEquipmentCatalog::decode(&carrier).unwrap();
    assert_eq!(catalog.entity_blob_sha256(), entity_sha);
    assert_eq!(
        catalog.source_manifest_sha256(),
        compilation.assets.source_manifest_sha256
    );
    assert_eq!(
        catalog
            .binding("minecraft:trident")
            .unwrap()
            .third_person
            .literal()
            .unwrap()
            .rotation
            .map(|s| s.get()),
        [97.0, -1.5, -49.0]
    );
}

#[test]
fn compilation_is_deterministic() {
    let first = compile_entity_assets_with_report(equipment_pack().path(), MANIFEST).unwrap();
    let second = compile_entity_assets_with_report(equipment_pack().path(), MANIFEST).unwrap();
    assert_eq!(first.equipment_bindings, second.equipment_bindings);
    assert_eq!(
        encode_entity_blob(&first.assets).unwrap(),
        encode_entity_blob(&second.assets).unwrap()
    );
}

#[test]
fn legacy_elytra_resolves_native_armor_texture_slots_and_reference_sidecar() {
    let pack = equipment_pack();
    let geometry_identifier = assets::ELYTRA_GEOMETRY_IDENTIFIER;
    let texture_identifier = "textures/models/armor/elytra";
    write(
        pack.path(),
        assets::LEGACY_ENTITY_GEOMETRY_PATH,
        &serde_json::to_vec(&serde_json::json!({
            "format_version":"1.8.0",
            (geometry_identifier):{
                "texturewidth":64,"textureheight":32,
                "bones":[
                    {"name":"body","pivot":[0,24,0]},
                    {"name":"left_wing","parent":"body","pivot":[0,24,0],
                        "cubes":[{"origin":[-10,0,0],"size":[10,20,2],"uv":[22,0]}]},
                    {"name":"right_wing","parent":"body","pivot":[0,24,0],
                        "mirror":true,"cubes":[{"origin":[0,0,0],"size":[10,20,2],"uv":[22,0]}]}
                ]
            },
            "geometry.obsolete":{"bones":[{"name":"old"}]}
        }))
        .unwrap(),
    );
    write(
        pack.path(),
        "attachables/elytra.json",
        &serde_json::to_vec(&serde_json::json!({
            "format_version":"1.10.0",
            "minecraft:attachable":{"description":{
                "identifier":"minecraft:elytra",
                "materials":{"default":"elytra","enchanted":"elytra_glint"},
                "textures":{"default":texture_identifier,"enchanted":assets::ACTOR_GLINT_TEXTURE_IDENTIFIER},
                "geometry":{"default":geometry_identifier},
                "render_controllers":["controller.render.armor"]
            }}
        }))
        .unwrap(),
    );
    write(
        pack.path(),
        "textures/models/armor/elytra.png",
        &synthetic_raster([200, 40, 40, 255]),
    );
    write(
        pack.path(),
        &format!("{}.png", assets::ACTOR_GLINT_TEXTURE_IDENTIFIER),
        &synthetic_raster([90, 20, 200, 255]),
    );
    write(pack.path(), "render_controllers/armor.json", br#"{
        "format_version":"1.8.0","render_controllers":{"controller.render.armor":{
            "geometry":"Geometry.default",
            "materials":[{"*":"variable.is_enchanted ? Material.enchanted : Material.default"}],
            "textures":["variable.has_trim ? variable.trim_path : Texture.default","Texture.enchanted"]
        }}
    }"#);
    let compilation = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
    let binding = compilation
        .equipment_bindings
        .iter()
        .find(|binding| binding.identifier.as_ref() == "minecraft:elytra")
        .unwrap();
    assert_eq!(binding.category, EquipmentCategory::Elytra);
    assert_eq!(binding.geometry.identifier.as_ref(), geometry_identifier);
    assert_eq!(
        binding.geometry.resolution,
        EntityDependencyResolution::Catalog,
        "chest equipment must resolve the legacy elytra model in the catalog"
    );
    assert_eq!(binding.texture.identifier.as_ref(), texture_identifier);
    assert_eq!(
        binding.texture.resolution,
        EntityDependencyResolution::Catalog
    );
    let geometry = compilation
        .assets
        .geometries
        .iter()
        .find(|geometry| geometry.identifier.as_ref() == geometry_identifier)
        .expect("elytra wings must be available to the equipment renderer");
    assert_eq!((geometry.texture_width, geometry.texture_height), (64, 32));
    assert_eq!(geometry.bones.len(), 3);
    assert_eq!(
        geometry.bones[1].cubes[0].size.map(|value| value.get()),
        [10.0, 20.0, 2.0]
    );
    let rig = compilation
        .assets
        .rig_bindings
        .iter()
        .position(|rig| {
            compilation.assets.symbols[rig.entity_symbol as usize]
                .identifier
                .as_ref()
                == "minecraft:elytra"
        })
        .expect("the native elytra attachable must have a runtime rig");
    let layer = compilation
        .assets
        .render
        .layers
        .iter()
        .find(|layer| layer.rig as usize == rig)
        .expect("the trim texture variable must not discard the native armor render layer");
    assert_eq!(layer.slot_count, 2);
    let slots = &compilation.assets.render.slots[layer.first_slot as usize..][..2];
    let base = &compilation.assets.render.candidates[slots[0].first_candidate as usize];
    assert!(
        base.condition.is_some(),
        "the base image is selected only without a trim"
    );
    assert_eq!(
        compilation.assets.sources[base.source as usize]
            .path
            .as_ref(),
        "textures/models/armor/elytra.png"
    );
    let glint = &compilation.assets.render.candidates[slots[1].first_candidate as usize];
    assert_eq!(
        compilation.assets.sources[glint.source as usize]
            .path
            .as_ref(),
        format!("{}.png", assets::ACTOR_GLINT_TEXTURE_IDENTIFIER)
    );
    let textures = pack_compiler::compile_equipment_textures_for_assets(
        pack.path(),
        &compilation.assets,
        &compilation.equipment_bindings,
    )
    .unwrap();
    assert_eq!(
        textures
            .iter()
            .find(|texture| texture.identifier.as_ref() == assets::ACTOR_GLINT_TEXTURE_IDENTIFIER)
            .expect("equipment must retain the shared enchantment raster")
            .rgba8
            .as_ref(),
        &[90, 20, 200, 255]
    );
    let base_textures = pack_compiler::compile_equipment_textures(
        pack.path(),
        &compilation.assets.sources,
        &compilation.equipment_bindings,
    )
    .unwrap();
    assert!(
        base_textures
            .iter()
            .any(|texture| texture.identifier.as_ref() == assets::ACTOR_GLINT_TEXTURE_IDENTIFIER)
    );
    let refs = pack_compiler::compile_vanilla_entity_refs(pack.path()).unwrap();
    let index = refs.geometry_index[geometry_identifier] as usize;
    let source: serde_json::Value = serde_json::from_str(&refs.geometry_files[index].text).unwrap();
    assert!(source.get(geometry_identifier).is_some());
    assert!(source.get("geometry.obsolete").is_none());
}

/// Encodes one original texel so carrier tests can verify raster ingestion.
fn synthetic_raster(pixel: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(1, 1, image::Rgba(pixel));
    let mut output = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}
