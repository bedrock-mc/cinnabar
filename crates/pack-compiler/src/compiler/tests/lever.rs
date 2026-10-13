use super::*;
use assets::{BlockFace, CompiledBiomeAssets, ModelQuad, NetworkIdMode, RuntimeAssets};

/// Compiles real lever identities with original independent base and handle artwork.
fn compile_levers(size: u32, override_state: Option<&str>) -> (RuntimeAssets, Vec<RegistryRecord>) {
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
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:air" | "minecraft:stone" | "minecraft:cobblestone" | "minecraft:lever"
            )
        })
        .collect();
    if let Some(state) = override_state {
        records
            .iter_mut()
            .find(|record| record.name.as_ref() == "minecraft:lever")
            .unwrap()
            .canonical_state = state.into();
    }
    let directory = tempfile::tempdir().unwrap();
    let mut textures = serde_json::Map::new();
    for (index, name) in ["base", "handle", "unused"].into_iter().enumerate() {
        textures.insert(
            name.into(),
            serde_json::json!({"textures":format!("textures/{name}")}),
        );
        let mut pixels = Vec::new();
        for y in 0..size {
            for x in 0..size {
                pixels.extend_from_slice(&[
                    40 + index as u8 * 60,
                    (x * 255 / size) as u8,
                    (y * 255 / size) as u8,
                    255,
                ]);
            }
        }
        write_png(
            directory.path().join(format!("textures/{name}.png")),
            size,
            size,
            &pixels,
        );
    }
    write(
        directory.path().join("blocks.json"),
        r#"{"stone":{"textures":"base"},"cobblestone":{"textures":"base"},"lever":{"textures":{"down":"handle","up":"unused","north":"unused","south":"unused","west":"unused","east":"unused"}}}"#,
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
    (
        RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap(),
        records,
    )
}

/// Returns the complete compiled model for one registered block state.
fn quads<'a>(runtime: &'a RuntimeAssets, record: &RegistryRecord) -> &'a [ModelQuad] {
    let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
    assert_eq!(visual.kind(), VisualKind::Model);
    let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
    &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize]
}

#[test]
fn levers_have_separate_base_and_powered_handle_in_every_mount() {
    for size in [16, 32] {
        let (runtime, records) = compile_levers(size, None);
        let mut poses = BTreeMap::new();
        let cobble = records
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:cobblestone")
            .unwrap();
        let base_material = runtime
            .resolve(NetworkIdMode::Sequential, cobble.sequential_id)
            .face(BlockFace::Up)
            .material_id();
        for record in records
            .iter()
            .filter(|r| r.name.as_ref() == "minecraft:lever")
        {
            let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
            let direction = state["lever_direction"]["value"].as_str().unwrap();
            let powered = state["open_bit"]["value"].as_u64().unwrap() != 0;
            let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
            let mesh = quads(&runtime, record);
            assert_eq!(
                mesh.len(),
                12,
                "missing base or handle: {}",
                record.canonical_state
            );
            assert!(mesh[..6].iter().all(|q| q.material == base_material));
            assert!(
                mesh[6..]
                    .iter()
                    .all(|q| q.material == visual.face(BlockFace::Down).material_id())
            );
            assert_ne!(base_material, visual.face(BlockFace::Down).material_id());
            assert_eq!(
                runtime
                    .resolve(NetworkIdMode::Hashed, record.network_hash)
                    .model_template(),
                visual.model_template()
            );
            let bounds: [[i16; 2]; 3] = std::array::from_fn(|axis| {
                let mut values = mesh[..6].iter().flat_map(|q| q.positions).map(|p| p[axis]);
                let first = values.next().unwrap();
                values.fold([first; 2], |[min, max], value| {
                    [min.min(value), max.max(value)]
                })
            });
            let expected = match direction {
                "east" => [[0, 48], [64, 192], [80, 176]],
                "west" => [[208, 256], [64, 192], [80, 176]],
                "south" => [[80, 176], [64, 192], [0, 48]],
                "north" => [[80, 176], [64, 192], [208, 256]],
                "up_north_south" => [[80, 176], [0, 48], [64, 192]],
                "up_east_west" => [[64, 192], [0, 48], [80, 176]],
                "down_north_south" => [[80, 176], [208, 256], [64, 192]],
                "down_east_west" => [[64, 192], [208, 256], [80, 176]],
                _ => panic!("unexpected lever direction"),
            };
            assert_eq!(bounds, expected, "{direction}");
            let cap_uv = [[1792, 2048], [2304, 2048], [2304, 1536], [1792, 1536]];
            assert_eq!(mesh[6].uvs, cap_uv);
            assert_eq!(mesh[7].uvs, cap_uv);
            for side in &mesh[8..] {
                assert_eq!(
                    side.uvs,
                    [[1792, 4096], [2304, 4096], [2304, 1536], [1792, 1536]]
                );
            }
            if direction == "up_north_south" && !powered {
                assert_eq!(
                    mesh[6].positions,
                    [
                        [112, 32, 128],
                        [144, 32, 128],
                        [144, 11, 153],
                        [112, 11, 153]
                    ]
                );
                assert_eq!(
                    mesh[7].positions,
                    [
                        [112, 134, 255],
                        [144, 134, 255],
                        [144, 155, 231],
                        [112, 155, 231]
                    ]
                );
            }
            let center: [f64; 3] = std::array::from_fn(|axis| {
                mesh[6..]
                    .iter()
                    .flat_map(|q| q.positions)
                    .map(|p| f64::from(p[axis]))
                    .sum::<f64>()
                    / 24.0
            });
            for quad in &mesh[6..] {
                let [a, b, c, _] = quad.positions.map(|p| p.map(f64::from));
                let ab: [f64; 3] = std::array::from_fn(|axis| b[axis] - a[axis]);
                let ac: [f64; 3] = std::array::from_fn(|axis| c[axis] - a[axis]);
                let normal = [
                    ab[1] * ac[2] - ab[2] * ac[1],
                    ab[2] * ac[0] - ab[0] * ac[2],
                    ab[0] * ac[1] - ab[1] * ac[0],
                ];
                assert!(
                    (0..3)
                        .map(|axis| normal[axis] * (a[axis] - center[axis]))
                        .sum::<f64>()
                        > 0.0,
                    "inward handle face: {direction} {powered}"
                );
            }
            poses.insert((direction.to_owned(), powered), mesh.to_vec());
        }
        assert_eq!(poses.len(), 16);
        for ((direction, powered), mesh) in &poses {
            if *powered {
                continue;
            }
            let other = &poses[&(direction.clone(), true)];
            assert_eq!(&mesh[..6], &other[..6]);
            assert_ne!(
                &mesh[6..],
                &other[6..],
                "powered handle has no motion: {direction}"
            );
        }
    }
}

#[test]
fn levers_reject_invalid_mount_and_power_selectors() {
    for state in [
        r#"{"lever_direction":{"type":"string","value":"invalid"},"open_bit":{"type":"byte","value":0}}"#,
        r#"{"lever_direction":{"type":"int","value":1},"open_bit":{"type":"byte","value":0}}"#,
        r#"{"lever_direction":{"type":"string","value":"east"},"open_bit":{"type":"byte","value":2}}"#,
        r#"{"lever_direction":{"type":"string","value":"east"},"open_bit":{"type":"int","value":1}}"#,
        r#"{"lever_direction":{"type":"string","value":"east"}}"#,
    ] {
        let (runtime, records) = compile_levers(16, Some(state));
        let record = records
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:lever")
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
