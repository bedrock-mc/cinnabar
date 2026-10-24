use super::*;
use assets::{BlockFace, BlockFlags, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Compiles real stonecutter identities against six independently observable original sprites.
fn compile_stonecutters(state: Option<&str>) -> (RuntimeAssets, Vec<RegistryRecord>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let registry = assets::read_registry_for_protocol(&bytes, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &bytes,
        registry.len(),
        protocol,
    )
    .unwrap();
    let mut records: Vec<_> = registry
        .into_vec()
        .into_iter()
        .filter(|r| {
            matches!(
                r.name.as_ref(),
                "minecraft:air" | "minecraft:stone" | "minecraft:stonecutter_block"
            )
        })
        .collect();
    if let Some(state) = state {
        records
            .iter_mut()
            .find(|r| r.name.as_ref() == "minecraft:stonecutter_block")
            .unwrap()
            .canonical_state = state.into();
    }
    let directory = tempfile::tempdir().unwrap();
    let faces = ["west", "east", "down", "up", "north", "south"];
    let mut definitions = serde_json::Map::new();
    let mut textures = serde_json::Map::new();
    for (index, face) in faces.into_iter().enumerate() {
        definitions.insert(face.into(), serde_json::json!(face));
        textures.insert(
            face.into(),
            serde_json::json!({"textures":format!("textures/{face}")}),
        );
        let mut pixels = Vec::new();
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                pixels.extend_from_slice(&[30 + index as u8 * 20, x as u8 * 8, y as u8 * 8, 255]);
            }
        }
        write_png(
            directory.path().join(format!("textures/{face}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
    }
    write(
        directory.path().join("blocks.json"),
        serde_json::to_vec(&serde_json::json!({
            "stonecutter_block":{"textures":definitions}, "stone":{"textures":"west"}
        }))
        .unwrap(),
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::to_vec(&serde_json::json!({"texture_data":textures})).unwrap(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let bytes = assets::encode_blob(&compiled).unwrap();
    (RuntimeAssets::decode(&bytes).unwrap(), records)
}

#[test]
fn stonecutter_saw_and_top_turn_with_the_cardinal_direction() {
    let (runtime, records) = compile_stonecutters(None);
    let mut tops = Vec::new();
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:stonecutter_block")
    {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let direction = state["minecraft:cardinal_direction"]["value"]
            .as_str()
            .unwrap()
            .to_owned();
        let along_z = matches!(direction.as_str(), "east" | "west");
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Model, "{direction}");
        assert!(
            !visual
                .flags()
                .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        );
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(quads.len(), 7, "body faces and one saw plane: {direction}");
        let materials = BlockFace::ALL.map(|face| visual.face(face).material_id());
        let (body, saw) = quads.split_at(6);
        for (face, quad) in BlockFace::ALL.into_iter().zip(body) {
            let expected = match face {
                BlockFace::Down | BlockFace::Up => materials[face as usize],
                // Every side shows the north art; the saw art never lands on the body.
                _ => materials[BlockFace::North as usize],
            };
            assert_eq!(quad.material, expected, "{direction}/{face:?}");
            assert!(quad.positions.iter().all(|p| p[1] <= 144), "{direction}");
            assert_eq!(
                quad.flags & assets::MODEL_QUAD_FLAG_CULL_FACE_MASK == 0,
                face == BlockFace::Up,
                "only the inset top skips neighbour culling: {direction}/{face:?}"
            );
        }
        assert!(body.iter().flat_map(|q| q.positions).any(|p| p[1] == 144));
        let saw = saw[0];
        assert_eq!(saw.material, materials[BlockFace::West as usize]);
        assert_ne!(saw.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED, 0);
        let (plane_axis, span_axis) = if along_z { (0, 2) } else { (2, 0) };
        assert!(
            saw.positions.iter().all(|p| p[plane_axis] == 128),
            "{direction}"
        );
        for (position, uv) in saw.positions.iter().zip(saw.uvs) {
            assert_eq!(uv[0], position[span_axis] as u16 * 16, "{direction}");
            let expected_v = match position[1] {
                128 => 4096,
                256 => 9 * 256,
                other => panic!("saw height {other}"),
            };
            assert_eq!(uv[1], expected_v, "{direction}");
        }
        tops.push((along_z, body[BlockFace::Up as usize]));
    }
    assert_eq!(tops.len(), 4);
    let (_, north_south) = tops.iter().find(|(along_z, _)| !along_z).unwrap();
    for (along_z, top) in &tops {
        assert_eq!(top.positions, north_south.positions);
        let expected = if *along_z {
            north_south.uvs.map(|[u, v]| [v, 4096 - u])
        } else {
            north_south.uvs
        };
        assert_eq!(
            top.uvs, expected,
            "east and west turn the top art a quarter turn"
        );
    }
}

#[test]
fn stonecutters_reject_invalid_direction_selectors_without_the_envelope() {
    for state in [
        "{}",
        r#"{"minecraft:cardinal_direction":{"type":"string","value":"up"}}"#,
        r#"{"minecraft:cardinal_direction":{"type":"int","value":0}}"#,
    ] {
        let (runtime, records) = compile_stonecutters(Some(state));
        let record = records
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:stonecutter_block")
            .unwrap();
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Sequential, record.sequential_id)
                .kind(),
            VisualKind::Diagnostic,
            "{state}"
        );
    }
}
