use super::*;
use assets::{BlockFace, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

const ARTWORK: [&str; 8] = [
    "furnace_top",
    "furnace_side",
    "furnace_front_off",
    "furnace_front_on",
    "loom_top",
    "loom_bottom",
    "loom_side",
    "loom_front",
];

/// The original artwork's red channel identifies its sprite independently of atlas ordering.
fn artwork_tag(name: &str) -> u8 {
    20 + ARTWORK.iter().position(|&art| art == name).unwrap() as u8 * 25
}

/// Reads the material's sprite identity through its actual texture page and layer.
fn material_tag(compiled: &assets::CompiledAssets, material: u32) -> u8 {
    let reference = compiled.materials[material as usize].texture;
    let mip = &compiled.texture_pages[reference.page() as usize]
        .texture
        .mips[0];
    mip.rgba8[(reference.layer() * mip.size * mip.size * 4) as usize]
}

/// Compiles every registered furnace and loom state with the pack's single-facing face layout.
fn compile_facing_cubes(
    loom_state: Option<&str>,
) -> (
    assets::CompiledAssets,
    Vec<RegistryRecord>,
    tempfile::TempDir,
) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let registry_bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let registry = assets::read_registry_for_protocol(&registry_bytes, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &registry_bytes,
        registry.len(),
        protocol,
    )
    .unwrap();
    let mut records = registry
        .into_vec()
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:air" | "minecraft:furnace" | "minecraft:lit_furnace" | "minecraft:loom"
            )
        })
        .collect::<Vec<_>>();
    if let Some(state) = loom_state {
        for record in records
            .iter_mut()
            .filter(|r| r.name.as_ref() == "minecraft:loom")
        {
            record.canonical_state = state.into();
        }
    }
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path().join("blocks.json"),
        r#"{
        "furnace":{"textures":{"down":"furnace_top","east":"furnace_side","north":"furnace_side","south":"furnace_front_off","up":"furnace_top","west":"furnace_side"}},
        "lit_furnace":{"textures":{"down":"furnace_top","east":"furnace_front_on","north":"furnace_side","south":"furnace_side","up":"furnace_top","west":"furnace_side"}},
        "loom":{"textures":{"down":"loom_bottom","east":"loom_side","north":"loom_front","south":"loom_side","up":"loom_top","west":"loom_side"}}
    }"#,
    );
    let texture_data = ARTWORK
        .iter()
        .map(|name| format!(r#""{name}":{{"textures":"textures/blocks/{name}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    write(
        directory.path().join("textures/terrain_texture.json"),
        format!(r#"{{"texture_data":{{{texture_data}}}}}"#),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    for name in ARTWORK {
        let pixels = (0..TILE_SIZE * TILE_SIZE)
            .flat_map(|pixel| {
                [
                    artwork_tag(name),
                    (pixel % TILE_SIZE) as u8 * 12,
                    (pixel / TILE_SIZE) as u8 * 12,
                    255,
                ]
            })
            .collect::<Vec<_>>();
        write_png(
            directory.path().join(format!("textures/blocks/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
    }
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (compiled, records, directory)
}

/// The world face a registered `minecraft:cardinal_direction` or loom `direction` points at.
fn facing(record: &RegistryRecord) -> BlockFace {
    let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
    let index = match state["minecraft:cardinal_direction"]["value"].as_str() {
        Some(name) => ["south", "west", "north", "east"]
            .iter()
            .position(|&candidate| candidate == name)
            .unwrap(),
        None => state["direction"]["value"].as_u64().unwrap() as usize,
    };
    [
        BlockFace::South,
        BlockFace::West,
        BlockFace::North,
        BlockFace::East,
    ][index]
}

#[test]
fn furnace_front_faces_its_cardinal_direction_in_both_lit_states() {
    let (compiled, records, _) = compile_facing_cubes(None);
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let mut fronts = std::collections::BTreeSet::new();
    for record in records.iter().filter(|r| r.name.contains("furnace")) {
        let lit = record.name.as_ref() == "minecraft:lit_furnace";
        let facing = facing(record);
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        for face in BlockFace::ALL {
            let expected = match face {
                BlockFace::Down | BlockFace::Up => "furnace_top",
                _ if face == facing && lit => "furnace_front_on",
                _ if face == facing => "furnace_front_off",
                _ => "furnace_side",
            };
            assert_eq!(
                material_tag(&compiled, visual.face(face).material_id()),
                artwork_tag(expected),
                "{} {} {face:?}",
                record.name,
                record.canonical_state
            );
        }
        fronts.insert((lit, facing as u8));
    }
    assert_eq!(fronts.len(), 8, "both lit states in all four directions");
}

/// Up and down UVs at the face's four corners, keyed by centred world X and Z.
fn vertical_uvs(
    runtime: &RuntimeAssets,
    record: &RegistryRecord,
    face: BlockFace,
) -> Vec<([i32; 2], [u16; 2])> {
    let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
    let template = runtime.model_templates()[visual.model_template().expect("loom model") as usize];
    let quads =
        &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
    let quad = quads[face as usize];
    assert_eq!(quad.material, visual.face(face).material_id());
    quad.positions
        .iter()
        .zip(quad.uvs)
        .map(|(position, uv)| {
            (
                [
                    i32::from(position[0]) * 2 - 256,
                    i32::from(position[2]) * 2 - 256,
                ],
                uv,
            )
        })
        .collect()
}

#[test]
fn loom_front_and_top_artwork_turn_with_its_direction() {
    let (compiled, records, _) = compile_facing_cubes(None);
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let looms: Vec<_> = records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:loom")
        .collect();
    assert_eq!(looms.len(), 4);
    let south = looms
        .iter()
        .find(|record| facing(record) == BlockFace::South)
        .unwrap();
    for record in &looms {
        let facing = facing(record);
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        for face in BlockFace::ALL {
            let expected = match face {
                BlockFace::Down => "loom_bottom",
                BlockFace::Up => "loom_top",
                _ if face == facing => "loom_front",
                _ => "loom_side",
            };
            assert_eq!(
                material_tag(&compiled, visual.face(face).material_id()),
                artwork_tag(expected),
                "{} {face:?}",
                record.canonical_state
            );
        }
        // Quarter turns about the vertical axis that carry the south-facing loom to this one.
        let turns = match facing {
            BlockFace::South => 0,
            BlockFace::West => 1,
            BlockFace::North => 2,
            _ => 3,
        };
        for face in [BlockFace::Down, BlockFace::Up] {
            let reference = vertical_uvs(&runtime, south, face);
            for (corner, uv) in vertical_uvs(&runtime, record, face) {
                // Undo this loom's turns; each turn carries +Z to -X.
                let mut source = corner;
                for _ in 0..turns {
                    source = [source[1], -source[0]];
                }
                let expected = reference
                    .iter()
                    .find(|(position, _)| *position == source)
                    .unwrap()
                    .1;
                assert_eq!(
                    uv, expected,
                    "{face:?} artwork turns rigidly with {}",
                    record.canonical_state
                );
            }
        }
    }
}

#[test]
fn malformed_loom_directions_stay_diagnostic() {
    for state in [
        r#"{"direction":{"type":"int","value":4}}"#,
        r#"{"direction":{"type":"byte","value":1}}"#,
        r#"{}"#,
    ] {
        let (compiled, records, _) = compile_facing_cubes(Some(state));
        let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
        let record = records
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:loom")
            .unwrap();
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Sequential, record.sequential_id)
                .support(),
            VisualSupport::Diagnostic,
            "{state}"
        );
    }
}
