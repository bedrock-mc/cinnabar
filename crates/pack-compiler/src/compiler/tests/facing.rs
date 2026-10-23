use super::*;
use assets::{BlockFace, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

const FACES: [&str; 6] = ["west", "east", "down", "up", "north", "south"];

/// Compiles every registered state of `names` with one distinguishable colour per authored face.
fn compile_facing_blocks(
    names: &[&str],
    scalar: bool,
) -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    let protocol = assets::active_content_registry_protocol();
    let records =
        assets::read_registry_for_protocol(assets::pinned_block_registry_bytes(), protocol)
            .unwrap()
            .into_vec()
            .into_iter()
            .filter(|record| {
                record.name.as_ref() == "minecraft:air"
                    || names.contains(&record.name.trim_start_matches("minecraft:"))
            })
            .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    let mut blocks = serde_json::Map::new();
    let mut terrain = serde_json::Map::new();
    for (index, name) in names.iter().enumerate() {
        let mut faces = serde_json::Map::new();
        for (face, tag) in FACES.into_iter().zip(1..=6) {
            let key = format!("{name}_{face}");
            let path = format!("textures/blocks/{key}");
            terrain.insert(key.clone(), serde_json::json!({"textures":path}));
            faces.insert(face.into(), key.clone().into());
            write_png(
                directory.path().join(format!("{path}.png")),
                TILE_SIZE,
                TILE_SIZE,
                &[[tag * 16, index as u8, 120, 255]; (TILE_SIZE * TILE_SIZE) as usize].concat(),
            );
        }
        blocks.insert(
            (*name).into(),
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
    assert_eq!(
        records.len(),
        names.len() * 4 + 1,
        "four facings per block plus air"
    );
    (compiled, records)
}

/// The authored face slot (1-based in west, east, down, up, north, south order) and block index.
fn material_tag(compiled: &assets::CompiledAssets, material: u32) -> (u8, u8) {
    let texture = compiled.materials[material as usize].texture;
    let mip = &compiled.texture_pages[texture.page() as usize].texture.mips[0];
    let offset = (texture.layer() * mip.size * mip.size * 4) as usize;
    (mip.rgba8[offset] / 16, mip.rgba8[offset + 1])
}

fn cardinal_front(record: &RegistryRecord) -> BlockFace {
    let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
    match state["minecraft:cardinal_direction"]["value"]
        .as_str()
        .unwrap()
    {
        "west" => BlockFace::West,
        "east" => BlockFace::East,
        "north" => BlockFace::North,
        "south" => BlockFace::South,
        other => panic!("unexpected direction {other}"),
    }
}

/// Slot number of an authored face in the fixture's west, east, down, up, north, south order.
fn slot(face: BlockFace) -> u8 {
    face as u8 + 1
}

fn assert_cube_faces(names: &[&str], expected: impl Fn(&RegistryRecord, BlockFace) -> BlockFace) {
    let (compiled, records) = compile_facing_blocks(names, false);
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() != "minecraft:air")
    {
        let index = names
            .iter()
            .position(|name| record.name.trim_start_matches("minecraft:") == *name)
            .unwrap() as u8;
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        for face in BlockFace::ALL {
            assert_eq!(
                material_tag(&compiled, visual.face(face).material_id()),
                (slot(expected(record, face)), index),
                "{} {}, face {face:?}",
                record.name,
                record.canonical_state,
            );
        }
    }
}

#[test]
fn furnace_family_fronts_follow_cardinal_direction_and_lit_slot() {
    let names = [
        "furnace",
        "lit_furnace",
        "blast_furnace",
        "lit_blast_furnace",
        "smoker",
        "lit_smoker",
    ];
    assert_cube_faces(&names, |record, face| match face {
        BlockFace::Down | BlockFace::Up => face,
        _ if face == cardinal_front(record) => {
            if record.name.contains("lit_") {
                BlockFace::East
            } else {
                BlockFace::South
            }
        }
        _ => BlockFace::North,
    });
}

#[test]
fn pumpkin_faces_follow_cardinal_direction_with_both_caps_from_the_top_slot() {
    assert_cube_faces(
        &["pumpkin", "carved_pumpkin", "lit_pumpkin"],
        |record, face| match face {
            BlockFace::Down | BlockFace::Up => BlockFace::Up,
            _ if face == cardinal_front(record) => BlockFace::South,
            _ => BlockFace::North,
        },
    );
}

#[test]
fn loom_turns_its_north_authored_faces_and_caps_to_its_direction() {
    let (compiled, records) = compile_facing_blocks(&["loom"], false);
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let mut cap_turns = std::collections::BTreeSet::new();
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:loom")
    {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let direction = state["direction"]["value"].as_i64().unwrap();
        // Directions run south, west, north, east; the authored front is the north slot.
        let front = [
            BlockFace::South,
            BlockFace::West,
            BlockFace::North,
            BlockFace::East,
        ][direction as usize];
        let back = match front {
            BlockFace::South => BlockFace::North,
            BlockFace::North => BlockFace::South,
            BlockFace::West => BlockFace::East,
            _ => BlockFace::West,
        };
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Model);
        let template = compiled.model_templates
            [compiled.visuals[record.sequential_id as usize].model_template as usize];
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        for (face, quad) in BlockFace::ALL.into_iter().zip(quads) {
            let (tag, _) = material_tag(&compiled, quad.material);
            let expected = match face {
                BlockFace::Down | BlockFace::Up => face,
                _ if face == front => BlockFace::North,
                _ if face == back => BlockFace::South,
                // Seen from above, the authored east side stays clockwise of the front.
                _ if face == clockwise(front) => BlockFace::East,
                _ => BlockFace::West,
            };
            assert_eq!(
                tag,
                slot(expected),
                "loom direction {direction}, face {face:?}"
            );
        }
        assert!(
            cap_turns.insert(quads[BlockFace::Up as usize].uvs),
            "the top turns with the loom"
        );
    }
    assert_eq!(cap_turns.len(), 4);
}

/// The horizontal face a quarter turn clockwise from `face`, seen from above.
fn clockwise(face: BlockFace) -> BlockFace {
    match face {
        BlockFace::North => BlockFace::East,
        BlockFace::East => BlockFace::South,
        BlockFace::South => BlockFace::West,
        _ => BlockFace::North,
    }
}

#[test]
fn furnace_scalar_replacement_artwork_covers_every_face_and_state() {
    let (compiled, records) = compile_facing_blocks(&["furnace", "lit_furnace"], true);
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() != "minecraft:air")
    {
        let index = u8::from(record.name.as_ref() == "minecraft:lit_furnace");
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        for face in BlockFace::ALL {
            assert_eq!(
                material_tag(&compiled, visual.face(face).material_id()),
                (slot(BlockFace::Up), index)
            );
        }
    }
}

/// Against the pinned vanilla pack, every exact block with a facing state and distinct
/// authored sides draws differently in some facing.
#[test]
fn vanilla_blocks_with_directional_sides_turn_with_their_facing() {
    let Some(root) = std::env::var_os("CINNABAR_VANILLA_ROOT").map(std::path::PathBuf::from) else {
        eprintln!("skipping missing fixture: CINNABAR_VANILLA_ROOT is not set");
        return;
    };
    let blocks: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("blocks.json")).unwrap()).unwrap();
    let protocol = assets::active_content_registry_protocol();
    let records =
        assets::read_registry_for_protocol(assets::pinned_block_registry_bytes(), protocol)
            .unwrap();
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        &root,
        &records,
        &vec![assets::LightProperties::default(); span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let facing_properties = [
        assets::facts::BlockProperty::Direction,
        assets::facts::BlockProperty::FacingDirection,
        assets::facts::BlockProperty::MinecraftCardinalDirection,
        assets::facts::BlockProperty::MinecraftFacingDirection,
    ];
    let mut looks = BTreeMap::<&str, (bool, std::collections::BTreeSet<_>)>::new();
    for record in records.iter().filter(|record| {
        record
            .properties
            .iter()
            .any(|(property, _)| facing_properties.contains(property))
    }) {
        let Some(faces) =
            blocks[record.name.trim_start_matches("minecraft:")]["textures"].as_object()
        else {
            continue;
        };
        let sides = ["north", "south", "east", "west"]
            .map(|side| faces.get(side).or_else(|| faces.get("side")));
        if sides.iter().all(|side| *side == sides[0]) {
            continue;
        }
        let visual = compiled.visuals[record.sequential_id as usize];
        let entry = looks
            .entry(&record.name)
            .or_insert((true, Default::default()));
        entry.0 &= visual.support == VisualSupport::Exact && visual.kind != VisualKind::Invisible;
        entry
            .1
            .insert((visual.faces, visual.model_template, visual.variant));
    }
    let static_blocks = looks
        .into_iter()
        .filter(|(_, (exact, variants))| *exact && variants.len() == 1)
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    assert!(static_blocks.is_empty(), "{static_blocks:?}");
}
