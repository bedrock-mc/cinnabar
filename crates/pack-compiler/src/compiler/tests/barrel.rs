use super::*;
use assets::{BlockFace, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

const ARTWORK: [&str; 6] = [
    "stone",
    "trapdoor",
    "barrel_top",
    "barrel_top_open",
    "barrel_bottom",
    "barrel_side",
];

/// The original artwork's red channel identifies its sprite independently of atlas ordering.
fn artwork_tag(name: &str) -> u8 {
    20 + ARTWORK.iter().position(|&art| art == name).unwrap() as u8 * 30
}

/// Reads the material's sprite identity through its actual texture page and layer.
fn material_tag(compiled: &assets::CompiledAssets, material: u32) -> u8 {
    let reference = compiled.materials[material as usize].texture;
    let mip = &compiled.texture_pages[reference.page() as usize]
        .texture
        .mips[0];
    mip.rgba8[(reference.layer() * mip.size * mip.size * 4) as usize]
}

/// Compiles registered barrels and trapdoors with distinct original face artwork.
fn compile_barrels() -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    let (compiled, records, _) = compile_barrels_with(None, false, false);
    (compiled, records)
}

/// Exercises typed admission and scalar replacement artwork through the same compiler entry point.
pub(crate) fn compile_barrels_with(
    state: Option<&str>,
    scalar: bool,
    isotropic: bool,
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
                "minecraft:air" | "minecraft:stone" | "minecraft:barrel" | "minecraft:trapdoor"
            )
        })
        .collect::<Vec<_>>();
    if let Some(state) = state {
        records
            .iter_mut()
            .find(|r| r.name.as_ref() == "minecraft:barrel")
            .unwrap()
            .canonical_state = state.into();
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
        "stone":{"textures":"stone"},"trapdoor":{"textures":"trapdoor"},
        "barrel":{"textures":{"north":"barrel_top","south":"barrel_bottom","west":"barrel_side","east":"barrel_side","up":"barrel_side","down":"barrel_side"}}
    }"#,
    );
    if scalar {
        write(
            directory.path().join("blocks.json"),
            r#"{
            "stone":{"textures":"stone"},"trapdoor":{"textures":"trapdoor"},
            "barrel":{"textures":"barrel_top"}
        }"#,
        );
    }
    if isotropic {
        let path = directory.path().join("blocks.json");
        let mut blocks: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        blocks["barrel"]["isotropic"] = true.into();
        write(path, serde_json::to_vec(&blocks).unwrap());
    }
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{
        "stone":{"textures":"textures/blocks/stone"},
        "trapdoor":{"textures":"textures/blocks/trapdoor"},
        "barrel_top":{"textures":["textures/blocks/barrel_top","textures/blocks/barrel_top_open"]},
        "barrel_bottom":{"textures":["textures/blocks/barrel_bottom","textures/blocks/barrel_bottom"]},
        "barrel_side":{"textures":["textures/blocks/barrel_side","textures/blocks/barrel_side"]}
    }}"#,
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

#[test]
fn barrel_quads_keep_grain_on_the_axis_and_materials_owned_by_each_block() {
    let (compiled, records) = compile_barrels();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let mut templates = std::collections::BTreeSet::new();
    for record in records
        .iter()
        .filter(|r| matches!(r.name.as_ref(), "minecraft:barrel" | "minecraft:trapdoor"))
    {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let template_id = visual.model_template().expect("state-owned model");
        let template = runtime.model_templates()[template_id as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(quads.len(), 6);
        for (face, quad) in BlockFace::ALL.into_iter().zip(quads) {
            assert_eq!(quad.material, visual.face(face).material_id());
            assert!(
                quad.uvs
                    .iter()
                    .flatten()
                    .all(|&component| component <= 4096)
            );
            if record.name.as_ref() == "minecraft:trapdoor" {
                assert_eq!(
                    material_tag(&compiled, quad.material),
                    artwork_tag("trapdoor")
                );
                continue;
            }
            assert_ne!(
                material_tag(&compiled, quad.material),
                artwork_tag("trapdoor")
            );
            assert_ne!(quad.flags & assets::MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
            let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
            let direction = state["facing_direction"]["value"].as_u64().unwrap() as usize;
            let axis = [1, 1, 2, 2, 0, 0][direction];
            if material_tag(&compiled, quad.material) == artwork_tag("barrel_side") {
                for a in 0..4 {
                    for b in a + 1..4 {
                        if quad.positions[a][axis] == quad.positions[b][axis] {
                            assert_eq!(quad.uvs[a][1], quad.uvs[b][1], "grain follows barrel axis");
                        }
                    }
                }
            }
        }
        if record.name.as_ref() == "minecraft:barrel" {
            assert!(
                templates.insert(template_id),
                "open and direction states keep separate artwork/UVs"
            );
        }
    }
    assert_eq!(templates.len(), 12);
}

#[test]
fn barrel_scalar_pack_override_keeps_open_variants_on_every_face() {
    let (compiled, records, _) = compile_barrels_with(None, true, false);
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:barrel")
    {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let open = state["open_bit"]["value"].as_u64().unwrap() as u32;
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        for face in BlockFace::ALL {
            assert_eq!(
                material_tag(&compiled, visual.face(face).material_id()),
                artwork_tag(if open == 0 {
                    "barrel_top"
                } else {
                    "barrel_top_open"
                })
            );
        }
    }
}

#[test]
fn malformed_barrel_selectors_stay_diagnostic() {
    for state in [
        r#"{"facing_direction":{"type":"int","value":6},"open_bit":{"type":"byte","value":0}}"#,
        r#"{"facing_direction":{"type":"int","value":1},"open_bit":{"type":"byte","value":2}}"#,
        r#"{"facing_direction":{"type":"byte","value":1},"open_bit":{"type":"byte","value":0}}"#,
        r#"{"facing_direction":{"type":"int","value":1}}"#,
    ] {
        let (compiled, records, _) = compile_barrels_with(Some(state), false, false);
        let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
        let record = records
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:barrel")
            .unwrap();
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Sequential, record.sequential_id)
                .support(),
            VisualSupport::Diagnostic
        );
    }
}

#[test]
fn barrel_faces_retain_pack_authored_isotropy() {
    let (compiled, records, _) = compile_barrels_with(None, false, true);
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:barrel")
    {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        for face in BlockFace::ALL {
            assert_ne!(
                runtime.material(visual.face(face).material_id()).flags
                    & assets::MATERIAL_FLAG_ISOTROPIC,
                0
            );
        }
    }
}

#[test]
fn barrel_facing_and_open_state_select_the_lid_and_opposite_base() {
    let (compiled, records) = compile_barrels();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let mut count = 0;
    let facing_faces = [
        BlockFace::Down,
        BlockFace::Up,
        BlockFace::North,
        BlockFace::South,
        BlockFace::West,
        BlockFace::East,
    ];
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:barrel")
    {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let facing = state["facing_direction"]["value"].as_u64().unwrap() as usize;
        let open = state["open_bit"]["value"].as_u64().unwrap() as usize;
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let lid = visual.face(facing_faces[facing]).material_id();
        let base = visual.face(facing_faces[facing ^ 1]).material_id();
        assert_eq!(
            material_tag(&compiled, lid),
            artwork_tag(if open == 0 {
                "barrel_top"
            } else {
                "barrel_top_open"
            }),
            "{}",
            record.canonical_state
        );
        assert_eq!(
            material_tag(&compiled, base),
            artwork_tag("barrel_bottom"),
            "{}",
            record.canonical_state
        );
        for face in BlockFace::ALL {
            if face != facing_faces[facing] && face != facing_faces[facing ^ 1] {
                assert_eq!(
                    material_tag(&compiled, visual.face(face).material_id()),
                    artwork_tag("barrel_side")
                );
            }
        }
        count += 1;
    }
    assert_eq!(count, 12);
}
