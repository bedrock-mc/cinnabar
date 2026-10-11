use super::*;
use assets::{BlockFace, BlockFlags, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Compiles real registry identities against six independently observable original sprites.
fn compile_grindstones(state: Option<&str>, color: u8) -> (RuntimeAssets, Vec<RegistryRecord>) {
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
                "minecraft:air" | "minecraft:stone" | "minecraft:grindstone"
            )
        })
        .collect();
    if let Some(state) = state {
        records
            .iter_mut()
            .find(|r| r.name.as_ref() == "minecraft:grindstone")
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
                pixels.extend_from_slice(&[
                    color + index as u8 * 20,
                    x as u8 * 8,
                    y as u8 * 8,
                    255,
                ]);
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
            "grindstone":{"textures":definitions}, "stone":{"textures":"west"}
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
    if state.is_none()
        && let Some(path) = std::env::var_os("CINNABAR_GRINDSTONE_CARRIER")
    {
        fs::write(path, &bytes).unwrap();
    }
    (RuntimeAssets::decode(&bytes).unwrap(), records)
}

#[test]
fn grindstones_keep_separate_supports_pivots_and_wheel_in_every_mount_pose() {
    let (runtime, records) = compile_grindstones(None, 30);
    let mut count = 0;
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:grindstone")
    {
        count += 1;
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let direction = state["direction"]["value"].as_u64().unwrap() as usize;
        let attachment = state["attachment"]["value"].as_str().unwrap();
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Model, "{attachment}/{direction}");
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Hashed, record.network_hash)
                .model_template(),
            visual.model_template()
        );
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(
            quads.len(),
            30,
            "two supports, two pivots and wheel: {attachment}/{direction}"
        );
        let corner = match (attachment, direction) {
            ("standing", 1) => Some((BlockFace::Up, [3072, 1536])),
            ("side", 0) => Some((BlockFace::East, [2560, 2304])),
            ("side", 1) => Some((BlockFace::North, [1536, 4096])),
            ("side", 2) => Some((BlockFace::Down, [1024, 2304])),
            _ => None,
        };
        if let Some((face, uv)) = corner {
            assert_eq!(
                quads[face as usize].uvs[0], uv,
                "mounting sprite corner: {attachment}/{direction}"
            );
        }
        assert!(
            !visual
                .flags()
                .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        );
        let materials = BlockFace::ALL.map(|face| visual.face(face).material_id());
        for material in materials {
            let flags = runtime.material(material).flags;
            assert_ne!(material, assets::DIAGNOSTIC_MATERIAL);
            assert_ne!(flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT, 0);
            assert_eq!(flags & assets::MATERIAL_FLAG_ALPHA_BLEND, 0);
        }
        for (face, expected) in [
            (BlockFace::Down, 12),
            (BlockFace::North, 12),
            (BlockFace::Up, 4),
            (BlockFace::South, 2),
        ] {
            assert_eq!(
                quads
                    .iter()
                    .filter(|q| q.material == materials[face as usize])
                    .count(),
                expected,
                "part materials: {attachment}/{direction}/{face:?}"
            );
        }
        let support_axis = match attachment {
            "side" if direction % 2 == 0 => 2,
            "side" => 0,
            _ => 1,
        };
        let support_high =
            attachment == "hanging" || (attachment == "side" && matches!(direction, 1 | 2));
        let support_edge = if support_high { 256 } else { 0 };
        let wheel_edge = if support_high { 0 } else { 256 };
        assert!(
            quads[..12]
                .iter()
                .flat_map(|q| q.positions)
                .any(|p| p[support_axis] == support_edge)
        );
        assert!(
            quads[24..]
                .iter()
                .flat_map(|q| q.positions)
                .any(|p| p[support_axis] == wheel_edge)
        );
        assert!(
            quads[24..]
                .iter()
                .flat_map(|q| q.positions)
                .all(|p| p[support_axis] != support_edge)
        );
        assert!(
            quads
                .iter()
                .all(|q| q.flags & assets::MODEL_QUAD_FLAG_CULL_FACE_MASK == 0)
        );
        assert!(
            quads
                .iter()
                .flat_map(|q| q.uvs)
                .flatten()
                .all(|uv| uv <= 4096)
        );
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Sequential, record.sequential_id)
                .support(),
            VisualSupport::Exact
        );
    }
    assert_eq!(count, 16);
}

#[test]
fn grindstones_reject_invalid_attachment_selectors_without_reusing_the_envelope() {
    for state in [
        "{}",
        r#"{"attachment":{"type":"string","value":"side"},"direction":{"type":"int","value":4}}"#,
        r#"{"attachment":{"type":"string","value":"unknown"},"direction":{"type":"int","value":0}}"#,
        r#"{"attachment":{"type":"string","value":"standing"},"direction":{"type":"byte","value":0}}"#,
    ] {
        let (runtime, records) = compile_grindstones(Some(state), 30);
        let record = records
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:grindstone")
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

#[test]
fn grindstone_replacement_sprites_keep_their_own_materials() {
    let (first, records) = compile_grindstones(None, 30);
    let (replacement, _) = compile_grindstones(None, 90);
    let record = records
        .iter()
        .find(|r| r.name.as_ref() == "minecraft:grindstone")
        .unwrap();
    let before = first.resolve(NetworkIdMode::Sequential, record.sequential_id);
    let after = replacement.resolve(NetworkIdMode::Sequential, record.sequential_id);
    assert_eq!(
        BlockFace::ALL.map(|face| before.face(face).material_id()),
        BlockFace::ALL.map(|face| after.face(face).material_id())
    );
    assert_ne!(
        first.texture_array().mips[0].rgba8,
        replacement.texture_array().mips[0].rgba8
    );
}
