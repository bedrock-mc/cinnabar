use super::*;
use assets::{BlockFace, BlockFlags, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

#[test]
fn current_anvil_states_keep_four_pieces_and_damage_art_through_the_carrier() {
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
    let names = [
        "minecraft:anvil",
        "minecraft:chipped_anvil",
        "minecraft:damaged_anvil",
    ];
    let records = registry
        .into_vec()
        .into_iter()
        .filter(|record| {
            matches!(record.name.as_ref(), "minecraft:air" | "minecraft:stone")
                || names.contains(&record.name.as_ref())
        })
        .collect::<Vec<_>>();
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut blocks = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            (
                name.trim_start_matches("minecraft:").to_owned(),
                serde_json::json!({
                    "textures": {"down": "base", "side": "base", "up": format!("top{index}")}
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    blocks.insert("stone".into(), serde_json::json!({"textures":"base"}));
    write(
        directory.path().join("blocks.json"),
        serde_json::to_vec(&blocks).unwrap(),
    );
    let mut textures = serde_json::Map::new();
    for (index, name) in ["base", "top0", "top1", "top2"].into_iter().enumerate() {
        textures.insert(
            name.into(),
            serde_json::json!({"textures": format!("textures/blocks/{name}")}),
        );
        write_png(
            directory.path().join(format!("textures/blocks/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &[40 + index as u8 * 40, 80, 120, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
        );
    }
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::to_vec(&serde_json::json!({"texture_data": textures})).unwrap(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let mut states = Vec::new();
    let mut damage_materials = BTreeMap::new();
    for record in records
        .iter()
        .filter(|record| names.contains(&record.name.as_ref()))
    {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let direction = state["minecraft:cardinal_direction"]["value"]
            .as_str()
            .unwrap();
        states.push((record.name.as_ref(), direction.to_owned()));
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(
            visual.kind(),
            VisualKind::Model,
            "{} {}",
            record.name,
            record.canonical_state
        );
        assert_eq!(visual.support(), VisualSupport::VanillaFallback);
        let top_material = visual.face(BlockFace::Up).material_id();
        if let Some(previous) = damage_materials.insert(record.name.as_ref(), top_material) {
            assert_eq!(top_material, previous);
        }
        assert!(
            !visual
                .flags()
                .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        );
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Hashed, record.network_hash)
                .model_template(),
            visual.model_template()
        );
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(quads.len(), 24, "four anvil pieces");
        let head_top = quads[18 + BlockFace::Up as usize];
        let top_uvs = match direction {
            "north" => [[768, 0], [768, 4096], [3328, 4096], [3328, 0]],
            "south" => [[3328, 4096], [3328, 0], [768, 0], [768, 4096]],
            "west" => [[3328, 0], [768, 0], [768, 4096], [3328, 4096]],
            "east" => [[768, 4096], [3328, 4096], [3328, 0], [768, 0]],
            _ => unreachable!(),
        };
        assert_eq!(head_top.uvs, top_uvs);
        let bounds = [
            ([32, 0, 32], [224, 64, 224]),
            ([64, 64, 48], [192, 80, 208]),
            ([96, 80, 64], [160, 160, 192]),
            ([48, 160, 0], [208, 256, 256]),
        ];
        let sideways = matches!(direction, "east" | "west");
        for (part, (faces, (mut min, mut max))) in quads.chunks_exact(6).zip(bounds).enumerate() {
            if sideways {
                min.swap(0, 2);
                max.swap(0, 2);
            }
            for axis in 0..3 {
                let positions = faces
                    .iter()
                    .flat_map(|face| face.positions.map(|p| p[axis]))
                    .collect::<Vec<_>>();
                assert_eq!(positions.iter().min().copied(), Some(min[axis]));
                assert_eq!(positions.iter().max().copied(), Some(max[axis]));
            }
            for (face, quad) in BlockFace::ALL.into_iter().zip(faces) {
                let expected_material = if part != 0 && face == BlockFace::Up {
                    visual.face(BlockFace::Up).material_id()
                } else {
                    visual.face(BlockFace::Down).material_id()
                };
                assert_eq!(quad.material, expected_material);
                assert!(
                    quad.uvs
                        .iter()
                        .flatten()
                        .all(|&coordinate| coordinate <= 4096)
                );
            }
        }
        assert_ne!(
            visual.face(BlockFace::Down).material_id(),
            visual.face(BlockFace::Up).material_id()
        );
    }
    states.sort_unstable();
    assert_eq!(states.len(), 12);
    let distinct_tops = damage_materials
        .values()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(distinct_tops.len(), names.len());
    for name in names {
        assert_eq!(
            states
                .iter()
                .filter(|(state_name, _)| *state_name == name)
                .map(|(_, direction)| direction.as_str())
                .collect::<Vec<_>>(),
            ["east", "north", "south", "west"]
        );
    }
}
