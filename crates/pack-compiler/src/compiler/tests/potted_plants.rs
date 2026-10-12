use super::*;
use assets::{CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Compiles plants against distinct original world, potted and stone artwork.
fn compile_plants() -> (RuntimeAssets, Vec<RegistryRecord>) {
    compile_plants_with_sparse_flower(false)
}

/// Optionally omits unused flower selectors to exercise sparse replacement packs.
fn compile_plants_with_sparse_flower(sparse: bool) -> (RuntimeAssets, Vec<RegistryRecord>) {
    let protocol = assets::active_content_registry_protocol();
    let registry = assets::read_registry_for_protocol(
        include_bytes!("../../../../../crates/assets/data/block-registry-v2193.bin"),
        protocol,
    )
    .unwrap();
    let records: Vec<_> = registry
        .into_vec()
        .into_iter()
        .filter(|r| {
            matches!(
                r.name.as_ref(),
                "minecraft:air"
                    | "minecraft:stone"
                    | "minecraft:poppy"
                    | "minecraft:crimson_roots"
                    | "minecraft:oak_sapling"
                    | "minecraft:cactus"
                    | "minecraft:bamboo"
                    | "minecraft:azalea"
                    | "minecraft:flowering_azalea"
            )
        })
        .collect();
    let directory = tempfile::tempdir().unwrap();
    for (name, color) in [
        ("world", [80, 160, 20, 255]),
        ("pot", [200, 50, 40, 255]),
        ("stone", [120, 120, 120, 255]),
        ("blocks/cactus_side", [40, 180, 60, 255]),
        ("blocks/cactus_top", [80, 190, 40, 255]),
        ("blocks/cactus_bottom", [130, 90, 40, 255]),
    ] {
        write_png(
            directory.path().join(format!("textures/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &color.repeat((TILE_SIZE * TILE_SIZE) as usize),
        );
    }
    write(
        directory.path().join("blocks.json"),
        r#"{"stone":{"textures":"stone"},"poppy":{"textures":"world"},"oak_sapling":{"textures":"world"},"cactus":{"textures":"world"},"bamboo":{"textures":{"north":"world","east":"pot","side":"world","up":"world","down":"world"}},"azalea":{"textures":{"west":"world","south":"pot","down":"stone","side":"world","up":"world"}},"flowering_azalea":{"textures":{"west":"pot","south":"world","down":"stone","side":"world","up":"world"}},"crimson_roots":{"textures":{"side":"world","up":"world","down":"world","south":"pot"}}}"#,
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"world":{"textures":"textures/world"},"pot":{"textures":"textures/pot"},"stone":{"textures":"textures/stone"}}}"#,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let mut blocks: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.path().join("blocks.json")).unwrap()).unwrap();
    blocks["cactus"]["textures"] =
        serde_json::json!({"side":"cactus_side","up":"cactus_top","down":"cactus_bottom"});
    if sparse {
        blocks["poppy"]["textures"] = serde_json::json!({"down":"world"});
    }
    write(
        directory.path().join("blocks.json"),
        serde_json::to_vec(&blocks).unwrap(),
    );
    let mut terrain: serde_json::Value = serde_json::from_slice(
        &fs::read(directory.path().join("textures/terrain_texture.json")).unwrap(),
    )
    .unwrap();
    for key in ["cactus_side", "cactus_top", "cactus_bottom"] {
        terrain["texture_data"][key] =
            serde_json::json!({"textures":format!("textures/blocks/{key}")});
    }
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::to_vec(&terrain).unwrap(),
    );
    let count = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &vec![assets::LightProperties::default(); count],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (
        RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap(),
        records,
    )
}

#[test]
fn potted_flower_uses_its_required_selector_without_unrelated_faces() {
    let (runtime, records) = compile_plants_with_sparse_flower(true);
    let flower = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:poppy")
        .unwrap();
    let quads = contents(&runtime, flower);
    assert_eq!(quads.len(), 2);
    assert_eq!(
        quads[0].material,
        runtime
            .resolve(NetworkIdMode::Sequential, flower.sequential_id)
            .face(assets::BlockFace::Down)
            .material_id()
    );
}

/// Borrows a registered plant's distinct pot-content surfaces.
fn contents<'a>(runtime: &'a RuntimeAssets, record: &RegistryRecord) -> &'a [assets::ModelQuad] {
    let template = runtime
        .potted_model_template(NetworkIdMode::Hashed, record.network_hash)
        .unwrap_or_else(|| {
            panic!(
                "{} has no potted model: {:?}",
                record.name,
                runtime.resolve(NetworkIdMode::Hashed, record.network_hash)
            )
        });
    let template = runtime.model_templates()[template as usize];
    &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize]
}

#[test]
fn potted_roots_select_pot_art_without_changing_terrain_art() {
    let (runtime, records) = compile_plants();
    let root = records
        .iter()
        .find(|r| r.name.as_ref() == "minecraft:crimson_roots")
        .unwrap();
    let world = runtime.resolve(NetworkIdMode::Hashed, root.network_hash);
    let quads = contents(&runtime, root);
    assert_ne!(
        quads[0].material,
        world.face(assets::BlockFace::Down).material_id()
    );
    let layer = runtime.material(quads[0].material).texture.layer() as usize;
    let first = layer * (TILE_SIZE * TILE_SIZE * 4) as usize;
    assert_eq!(
        &runtime.texture_array().mips[0].rgba8[first..first + 4],
        &[200, 50, 40, 255]
    );
}

#[test]
fn potted_cactus_bamboo_and_azalea_keep_their_parts_and_sprite_regions() {
    let (runtime, records) = compile_plants();
    for name in [
        "minecraft:cactus",
        "minecraft:bamboo",
        "minecraft:azalea",
        "minecraft:flowering_azalea",
    ] {
        let record = records
            .iter()
            .find(|r| r.name.as_ref() == name)
            .expect("active plant identity");
        let quads = contents(&runtime, record);
        match name {
            "minecraft:cactus" => {
                assert_eq!(quads.len(), 6);
                assert_eq!(
                    quads.iter().flat_map(|q| q.positions).map(|p| p[1]).min(),
                    Some(64)
                );
                assert_eq!(
                    quads.iter().flat_map(|q| q.positions).map(|p| p[1]).max(),
                    Some(256)
                );
                assert!(
                    quads
                        .iter()
                        .flat_map(|q| q.positions)
                        .all(|p| (96..=160).contains(&p[0]) && (96..=160).contains(&p[2]))
                );
                assert_eq!(quads[0].uvs.iter().map(|uv| uv[1]).min(), Some(1024));
                assert_eq!(quads[0].uvs.iter().map(|uv| uv[1]).max(), Some(4096));
            }
            "minecraft:bamboo" => {
                assert_eq!(quads.len(), 7);
                assert!(
                    quads[..6]
                        .iter()
                        .flat_map(|q| q.positions)
                        .all(|p| (112..=144).contains(&p[0]) && (112..=144).contains(&p[2]))
                );
                assert_ne!(quads[0].material, quads[6].material);
                assert_eq!(
                    quads[6].positions,
                    [
                        [144, 288, 128],
                        [240, 288, 128],
                        [240, 32, 128],
                        [144, 32, 128]
                    ]
                );
                assert_eq!(
                    quads[6].uvs,
                    [[1792, 0], [256, 0], [256, 4096], [1792, 4096]]
                );
            }
            _ => {
                assert_eq!(quads.len(), 7);
                assert_ne!(quads[0].material, quads[2].material);
                assert_ne!(quads[2].material, quads[4].material);
                assert_eq!(quads[4].positions.iter().map(|p| p[1]).max(), Some(256));
                assert!(
                    quads[2..]
                        .iter()
                        .flat_map(|q| q.positions)
                        .all(|p| (64..=192).contains(&p[0]) && (64..=192).contains(&p[2]))
                );
            }
        }
    }
}

#[test]
fn potted_plants_retain_world_models_and_compile_contents_at_soil_height() {
    let (runtime, records) = compile_plants();
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:poppy")
    {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let world = visual.model_template().unwrap();
        let pot = runtime
            .potted_model_template(NetworkIdMode::Sequential, record.sequential_id)
            .expect("plants have separate pot-content geometry");
        let template = runtime.model_templates()[pot as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(quads.len(), 2);
        let positions: Vec<_> = quads.iter().flat_map(|q| q.positions).collect();
        assert_eq!(positions.iter().map(|p| p[1]).min(), Some(64));
        assert_eq!(positions.iter().map(|p| p[1]).max(), Some(256));
        assert_eq!(positions.iter().map(|p| p[0]).min(), Some(36));
        assert_eq!(positions.iter().map(|p| p[0]).max(), Some(220));
        let template = runtime.model_templates()[world as usize];
        let world_quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(
            world_quads
                .iter()
                .flat_map(|q| q.positions)
                .map(|p| p[1])
                .min(),
            Some(0)
        );
    }
}
