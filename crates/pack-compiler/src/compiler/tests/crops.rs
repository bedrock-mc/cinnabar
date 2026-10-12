use super::*;
use assets::{CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Compiles every farmland crop state with distinct original stage artwork.
fn compile_crops() -> (RuntimeAssets, Vec<RegistryRecord>) {
    let protocol = assets::active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(
        include_bytes!("../../../../../crates/assets/data/block-registry-v2193.bin"),
        protocol,
    )
    .unwrap()
    .into_vec()
    .into_iter()
    .filter(|record| {
        matches!(
            record.name.as_ref(),
            "minecraft:air"
                | "minecraft:wheat"
                | "minecraft:carrots"
                | "minecraft:potatoes"
                | "minecraft:beetroot"
                | "minecraft:poppy"
        )
    })
    .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    let mut blocks = serde_json::Map::new();
    let mut terrain = serde_json::Map::new();
    for (family, stages) in [
        ("wheat", 8),
        ("carrots", 4),
        ("potatoes", 4),
        ("beetroot", 4),
        ("poppy", 1),
    ] {
        blocks.insert(family.into(), serde_json::json!({"textures":family}));
        let mut paths = Vec::new();
        for stage in 0..stages {
            let path = format!("textures/{family}_{stage}");
            write_png(
                directory.path().join(format!("{path}.png")),
                TILE_SIZE,
                TILE_SIZE,
                &[30 + stage * 20, 80, 120, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
            );
            paths.push(path);
        }
        terrain.insert(family.into(), serde_json::json!({"textures":paths}));
    }
    write(
        directory.path().join("blocks.json"),
        serde_json::to_vec(&blocks).unwrap(),
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::to_vec(&serde_json::json!({"texture_data":terrain})).unwrap(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
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
fn farmland_crops_keep_stage_art_on_four_rows_that_meet_the_soil() {
    let (runtime, records) = compile_crops();
    let mut crops = 0;
    for record in &records {
        let Some(name) = record.name.strip_prefix("minecraft:") else {
            continue;
        };
        if !matches!(name, "wheat" | "carrots" | "potatoes" | "beetroot") {
            continue;
        }
        crops += 1;
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let growth = state["growth"]["value"].as_u64().unwrap() as usize;
        let stage = if name == "wheat" {
            growth
        } else {
            [0, 0, 1, 1, 2, 2, 2, 3][growth]
        };
        for (mode, id) in [
            (NetworkIdMode::Sequential, record.sequential_id),
            (NetworkIdMode::Hashed, record.network_hash),
        ] {
            let visual = runtime.resolve(mode, id);
            let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
            let quads = &runtime.model_quads()[template.quad_start as usize..]
                [..template.quad_count as usize];
            assert_eq!(quads.len(), 4, "{} growth {growth}", record.name);
            assert_eq!(
                visual.variant(),
                stage as u32,
                "{} growth {growth}",
                record.name
            );
            let mut rows = Vec::new();
            for quad in quads {
                assert_ne!(quad.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED, 0);
                assert_eq!(quad.positions.iter().map(|p| p[1]).min(), Some(-16));
                assert_eq!(quad.positions.iter().map(|p| p[1]).max(), Some(240));
                let axis = if quad.positions.iter().all(|p| p[0] == quad.positions[0][0]) {
                    0
                } else {
                    2
                };
                assert!(
                    quad.positions
                        .iter()
                        .all(|p| p[axis] == quad.positions[0][axis]),
                    "crop plane must be axis aligned"
                );
                rows.push((axis, quad.positions[0][axis]));
                assert_eq!(
                    quad.material,
                    visual.face(assets::BlockFace::Down).material_id()
                );
                assert_eq!(
                    runtime.materials()[quad.material as usize].flags
                        & assets::MATERIAL_FLAG_TINT_MASK,
                    0
                );
                let texture = runtime.materials()[quad.material as usize].texture;
                let mip = &runtime.texture_pages()[texture.page() as usize]
                    .texture
                    .mips[0];
                let start = texture.layer() as usize * (mip.size * mip.size * 4) as usize;
                assert_eq!(
                    &mip.rgba8[start..start + 4],
                    &[30 + stage as u8 * 20, 80, 120, 255]
                );
                for (position, uv) in quad.positions.iter().zip(quad.uvs) {
                    assert_eq!(
                        uv[1],
                        if position[1] == 240 { 0 } else { 4096 },
                        "stage art must stay upright"
                    );
                }
            }
            rows.sort_unstable();
            assert_eq!(rows, [(0, 64), (0, 192), (2, 64), (2, 192)]);
        }
    }
    assert_eq!(crops, 32);
    let flower = records
        .iter()
        .find(|r| r.name.as_ref() == "minecraft:poppy")
        .unwrap();
    let template = runtime
        .resolve(NetworkIdMode::Sequential, flower.sequential_id)
        .model_template()
        .unwrap();
    assert_eq!(runtime.model_templates()[template as usize].quad_count, 2);
}
