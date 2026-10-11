use super::*;
use assets::{BlockFace, BlockFlags, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};
use serde_json::json;

/// Compiles all registered coral forms against distinct, original replacement sprites.
fn compile_corals(
    invalid_wall: Option<serde_json::Value>,
) -> (RuntimeAssets, Vec<RegistryRecord>, BTreeMap<String, u8>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let registry = assets::read_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        protocol,
    )
    .unwrap();
    let mut records: Vec<_> = registry
        .into_vec()
        .into_iter()
        .filter(|r| {
            r.name.contains("coral")
                || matches!(r.name.as_ref(), "minecraft:air" | "minecraft:stone")
        })
        .collect();
    if let Some(state) = invalid_wall {
        records
            .iter_mut()
            .find(|r| r.name.as_ref() == "minecraft:brain_coral_wall_fan")
            .unwrap()
            .canonical_state = state.to_string().into();
    }
    let directory = tempfile::tempdir().unwrap();
    let mut blocks = serde_json::Map::new();
    let mut terrain = serde_json::Map::new();
    let mut colors = BTreeMap::new();
    for record in &records {
        let name = record.name.strip_prefix("minecraft:").unwrap();
        if blocks.contains_key(name) || name == "air" {
            continue;
        }
        let color = blocks.len() as u8 + 20;
        colors.insert(record.name.to_string(), color);
        let cutout = name.contains("coral") && !name.ends_with("_coral_block");
        let path = format!("textures/blocks/{name}");
        let key = format!("replacement_{name}");
        blocks.insert(name.into(), json!({"textures":key}));
        terrain.insert(key, json!({"textures":path}));
        let pixels: Vec<_> = (0..TILE_SIZE)
            .flat_map(|y| {
                (0..TILE_SIZE).flat_map(move |x| {
                    [
                        color,
                        x as u8 * 8,
                        y as u8 * 8,
                        if cutout && x == 0 { 0 } else { 255 },
                    ]
                })
            })
            .collect();
        write_png(
            directory.path().join(format!("{path}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
    }
    write(
        directory.path().join("blocks.json"),
        serde_json::to_vec(&blocks).unwrap(),
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::to_vec(&json!({"texture_data":terrain})).unwrap(),
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
        &vec![assets::LightProperties::default(); span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (
        RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap(),
        records,
        colors,
    )
}

#[test]
fn coral_forms_resolve_artwork_and_state_selected_models() {
    let (runtime, records, colors) = compile_corals(None);
    let mut floor_poses = BTreeMap::new();
    let mut wall_poses = BTreeMap::new();
    for record in records.iter().filter(|r| r.name.contains("coral")) {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(
            visual.support(),
            VisualSupport::Exact,
            "{} {}",
            record.name,
            record.canonical_state
        );
        let material = visual.face(BlockFace::Up).material_id();
        let texture = runtime.material(material).texture;
        let pixels = &runtime.texture_array().mips[0].rgba8;
        let pixel = (texture.layer() as usize * TILE_SIZE as usize * TILE_SIZE as usize + 1) * 4;
        assert_eq!(
            pixels[pixel],
            colors[record.name.as_ref()],
            "{} retains its replacement sprite",
            record.name
        );
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Hashed, record.network_hash)
                .kind(),
            visual.kind()
        );
        if record.name.ends_with("_coral_block") {
            assert_eq!(visual.kind(), VisualKind::Cube, "{}", record.name);
            assert!(
                visual
                    .flags()
                    .contains(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
            );
            continue;
        }
        assert!(
            !visual
                .flags()
                .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        );
        let template =
            runtime.model_templates()[visual.model_template().expect("coral template") as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        assert!(quads.iter().all(|q| q.material == material));
        assert_ne!(
            runtime.material(material).flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT,
            0,
            "{} {}",
            record.name,
            record.canonical_state,
        );
        assert!(
            quads
                .iter()
                .all(|q| q.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED != 0)
        );
        if record.name.ends_with("_coral_wall_fan") {
            assert_eq!(quads.len(), 1, "wall fan is one inclined sprite");
            let direction = state["coral_direction"]["value"].as_u64().unwrap();
            let axis = if direction < 2 { 0 } else { 2 };
            let attachment = if direction % 2 == 0 { 3 } else { 253 };
            let roots: Vec<_> = quads[0].positions.iter().filter(|p| p[1] == 128).collect();
            assert_eq!(roots.len(), 2);
            assert!(
                roots.iter().all(|p| p[axis] == attachment),
                "direction {direction} attaches at the correct face"
            );
            assert!(
                quads[0]
                    .positions
                    .iter()
                    .filter(|p| p[1] != 128)
                    .all(|p| p[1] == 166)
            );
            wall_poses.insert(direction, quads[0].positions);
        } else if record.name.ends_with("_coral_fan") {
            assert_eq!(quads.len(), 4, "floor fan spreads in four directions");
            assert!(quads.iter().flat_map(|q| q.positions).all(|p| p[1] < 128));
            floor_poses.insert(
                state["coral_fan_direction"]["value"].as_u64().unwrap(),
                quads.iter().map(|q| q.positions).collect::<Vec<_>>(),
            );
        } else {
            assert_eq!(quads.len(), 2, "plant keeps crossed sprites");
        }
    }
    assert_eq!(floor_poses.len(), 2);
    assert_eq!(floor_poses[&0], floor_poses[&1]);
    assert_eq!(wall_poses.len(), 4);
    let distinct: std::collections::BTreeSet<_> = wall_poses.values().collect();
    assert_eq!(distinct.len(), 4);
}

#[test]
fn coral_wall_fans_reject_malformed_direction_states() {
    for state in [
        json!({"coral_direction":{"type":"byte","value":0}}),
        json!({"coral_direction":{"type":"int","value":4}}),
        json!({"coral_direction":{"type":"int","value":0},"extra":{"type":"int","value":0}}),
    ] {
        let (runtime, records, _) = compile_corals(Some(state));
        let invalid = records
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:brain_coral_wall_fan")
            .unwrap();
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Sequential, invalid.sequential_id)
                .kind(),
            VisualKind::Diagnostic
        );
    }
}
