use super::*;
use assets::{BlockFace, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Compiles every registered furnace state with original, distinguishable face artwork.
fn compile_furnaces(scalar: bool) -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    let protocol = assets::active_content_registry_protocol();
    let records =
        assets::read_registry_for_protocol(assets::pinned_block_registry_bytes(), protocol)
            .unwrap()
            .into_vec()
            .into_iter()
            .filter(|record| {
                matches!(
                    record.name.as_ref(),
                    "minecraft:air" | "minecraft:furnace" | "minecraft:lit_furnace"
                )
            })
            .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    let mut blocks = serde_json::Map::new();
    let mut terrain = serde_json::Map::new();
    for (name, offset) in [("furnace", 0), ("lit_furnace", 6)] {
        let mut faces = serde_json::Map::new();
        for (face, tag) in ["west", "east", "down", "up", "north", "south"]
            .into_iter()
            .zip(1..=6)
        {
            let key = format!("{name}_{face}");
            let path = format!("textures/blocks/{key}");
            terrain.insert(key.clone(), serde_json::json!({"textures":path}));
            faces.insert(face.into(), key.clone().into());
            write_png(
                directory.path().join(format!("{path}.png")),
                TILE_SIZE,
                TILE_SIZE,
                &[[(offset + tag) * 16, 80, 120, 255]; (TILE_SIZE * TILE_SIZE) as usize].concat(),
            );
        }
        blocks.insert(
            name.into(),
            if scalar {
                serde_json::json!({"textures":format!("{name}_up")})
            } else {
                serde_json::json!({"textures":faces})
            },
        );
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
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &vec![assets::LightProperties::default(); span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (compiled, records)
}

/// Reads a solid fixture colour through the compiled material and texture atlas.
fn material_tag(compiled: &assets::CompiledAssets, material: u32) -> u8 {
    let texture = compiled.materials[material as usize].texture;
    let mip = &compiled.texture_pages[texture.page() as usize].texture.mips[0];
    mip.rgba8[(texture.layer() * mip.size * mip.size * 4) as usize]
}

#[test]
fn furnace_front_and_side_materials_follow_all_cardinal_and_lit_states() {
    let (compiled, records) = compile_furnaces(false);
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() != "minecraft:air")
    {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let facing = state["minecraft:cardinal_direction"]["value"]
            .as_str()
            .unwrap();
        let front = match facing {
            "west" => BlockFace::West,
            "east" => BlockFace::East,
            "north" => BlockFace::North,
            "south" => BlockFace::South,
            _ => panic!("unexpected furnace direction {facing}"),
        };
        let lit = record.name.as_ref() == "minecraft:lit_furnace";
        assert!(seen.insert((lit, facing.to_owned())));
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Cube);
        for face in BlockFace::ALL {
            let tag = match face {
                BlockFace::Down => 3,
                BlockFace::Up => 4,
                _ if face == front => {
                    if lit {
                        2
                    } else {
                        6
                    }
                }
                _ => 5,
            } + if lit { 6 } else { 0 };
            assert_eq!(
                material_tag(&compiled, visual.face(face).material_id()),
                tag * 16,
                "{} facing {facing}, face {face:?}",
                record.name,
            );
        }
    }
    assert_eq!(
        seen.len(),
        8,
        "four directions for each furnace lighting state"
    );
}

#[test]
fn furnace_scalar_replacement_artwork_covers_every_face_and_state() {
    let (compiled, records) = compile_furnaces(true);
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() != "minecraft:air")
    {
        let expected = if record.name.as_ref() == "minecraft:lit_furnace" {
            10
        } else {
            4
        } * 16;
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        for face in BlockFace::ALL {
            assert_eq!(
                material_tag(&compiled, visual.face(face).material_id()),
                expected
            );
        }
    }
}
