use super::*;
use assets::{BlockFlags, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Compiles the active lightning-rod state product with original sprite artwork.
fn compile_rods() -> (assets::CompiledAssets, Vec<RegistryRecord>, Vec<u8>) {
    compile_rods_with_state(None)
}

/// Replaces one selector to exercise admission without inventing a network identity.
fn compile_rods_with_state(
    state: Option<&str>,
) -> (assets::CompiledAssets, Vec<RegistryRecord>, Vec<u8>) {
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
            matches!(record.name.as_ref(), "minecraft:air" | "minecraft:stone")
                || record.name.ends_with("lightning_rod")
        })
        .collect::<Vec<_>>();
    if let Some(state) = state {
        records
            .iter_mut()
            .find(|record| record.name.ends_with("lightning_rod"))
            .unwrap()
            .canonical_state = state.into();
    }
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let blocks = records
        .iter()
        .filter(|r| r.name.as_ref() != "minecraft:air")
        .map(|r| {
            let name = r.name.strip_prefix("minecraft:").unwrap();
            (
                name,
                serde_json::json!({"textures": name.strip_prefix("waxed_").unwrap_or(name)}),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let textures = blocks
        .values()
        .map(|block| {
            let key = block["textures"].as_str().unwrap();
            (
                key,
                if key.ends_with("lightning_rod") {
                    serde_json::json!({"textures": [format!("textures/blocks/{key}"), "textures/blocks/lightning_rod_powered"]})
                } else {
                    serde_json::json!({"textures": format!("textures/blocks/{key}")})
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    write(
        directory.path().join("blocks.json"),
        serde_json::to_vec(&blocks).unwrap(),
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::to_vec(&serde_json::json!({"texture_data":textures})).unwrap(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let mut pixels = vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize];
    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            if (x < 4 && y < 4) || (x < 2 && y >= 4) {
                let offset = ((y * TILE_SIZE + x) * 4) as usize;
                pixels[offset..offset + 4].copy_from_slice(&[
                    80 + x as u8 * 20,
                    210 - y as u8 * 8,
                    120,
                    255,
                ]);
            }
        }
    }
    for (variant, name) in textures
        .keys()
        .filter(|name| name.ends_with("lightning_rod"))
        .enumerate()
    {
        let mut artwork = pixels.clone();
        for pixel in artwork
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .filter(|p| p[3] != 0)
        {
            pixel[2] = 50 + variant as u8 * 40;
        }
        write_png(
            directory.path().join(format!("textures/blocks/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &artwork,
        );
    }
    let mut powered_art = pixels.clone();
    for pixel in powered_art
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .filter(|p| p[3] != 0)
    {
        pixel[0] = 230;
        pixel[2] = 230;
    }
    write_png(
        directory
            .path()
            .join("textures/blocks/lightning_rod_powered.png"),
        TILE_SIZE,
        TILE_SIZE,
        &powered_art,
    );
    write_png(
        directory.path().join("textures/blocks/stone.png"),
        TILE_SIZE,
        TILE_SIZE,
        &[80, 80, 80, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
    );
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    if state.is_none()
        && let Some(path) = std::env::var_os("CINNABAR_LIGHTNING_ROD_CARRIER")
    {
        fs::write(path, assets::encode_blob(&compiled).unwrap()).unwrap();
    }
    (compiled, records, pixels)
}

#[test]
fn lightning_rod_states_keep_complete_geometry_after_roundtrip() {
    let (compiled, records, pixels) = compile_rods();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let mut family = BTreeMap::<String, Vec<usize>>::new();
    let mut materials = BTreeMap::new();
    for record in records.iter().filter(|r| r.name.ends_with("lightning_rod")) {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let direction = state["facing_direction"]["value"].as_u64().unwrap() as usize;
        family
            .entry(record.name.to_string())
            .or_default()
            .push(direction);
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Model);
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        assert_eq!(
            template.quad_count,
            if direction < 2 { 11 } else { 12 },
            "{} {direction}",
            record.name
        );
        assert_eq!(visual.support(), VisualSupport::Exact);
        assert!(
            !visual
                .flags()
                .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        );
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        let shaft_faces = quads.len() - 6;
        let axis = [1, 1, 2, 2, 0, 0][direction];
        for (part, cross, axis_bounds) in [
            (
                &quads[..shaft_faces],
                (112, 144),
                if direction % 2 == 0 {
                    (64, 256)
                } else {
                    (0, 192)
                },
            ),
            (
                &quads[shaft_faces..],
                (96, 160),
                if direction % 2 == 0 {
                    (0, 64)
                } else {
                    (192, 256)
                },
            ),
        ] {
            for coordinate in 0..3 {
                let values = part
                    .iter()
                    .flat_map(|q| q.positions)
                    .map(|p| p[coordinate])
                    .collect::<Vec<_>>();
                assert_eq!(
                    (*values.iter().min().unwrap(), *values.iter().max().unwrap()),
                    if coordinate == axis {
                        axis_bounds
                    } else {
                        cross
                    }
                );
            }
            for quad in part {
                assert_eq!(quad.flags & assets::MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
                assert_eq!(
                    runtime.material(quad.material).flags,
                    assets::MATERIAL_FLAG_ALPHA_CUTOUT
                );
                let uv =
                    [0, 1].map(|a| quad.uvs.iter().map(|uv| u32::from(uv[a])).sum::<u32>() / 4);
                let [x, y] = uv.map(|v| v * TILE_SIZE / 4096);
                assert_eq!(
                    pixels[((y * TILE_SIZE + x) * 4 + 3) as usize],
                    255,
                    "{} {direction}",
                    record.name
                );
            }
        }
        let sprite_name = if state["powered_bit"]["value"].as_u64().unwrap() == 1 {
            "lightning_rod_powered"
        } else {
            record
                .name
                .strip_prefix("minecraft:")
                .unwrap()
                .strip_prefix("waxed_")
                .unwrap_or(record.name.strip_prefix("minecraft:").unwrap())
        };
        if let Some(previous) = materials.insert(sprite_name, quads[0].material) {
            assert_eq!(
                previous, quads[0].material,
                "waxing and facing retain the selected sprite"
            );
        }
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Hashed, record.network_hash)
                .model_template(),
            visual.model_template()
        );
    }
    assert_eq!(family.len(), 8);
    let mut sprite_materials = materials.into_values().collect::<Vec<_>>();
    sprite_materials.sort_unstable();
    sprite_materials.dedup();
    assert_eq!(
        sprite_materials.len(),
        5,
        "each oxidation stage and the shared powered variant retain their pack sprite overrides"
    );
    for states in family.values_mut() {
        states.sort_unstable();
        assert_eq!(states.as_slice(), [0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5]);
    }
}

#[test]
fn lightning_rods_reject_unsupported_selectors() {
    for state in [
        r#"{}"#,
        r#"{"facing_direction":{"type":"int","value":6},"powered_bit":{"type":"byte","value":0}}"#,
        r#"{"facing_direction":{"type":"byte","value":1},"powered_bit":{"type":"byte","value":0}}"#,
        r#"{"facing_direction":{"type":"int","value":1},"powered_bit":{"type":"int","value":0}}"#,
        r#"{"facing_direction":{"type":"int","value":1},"powered_bit":{"type":"byte","value":2}}"#,
        r#"{"facing_direction":{"type":"int","value":1},"powered_bit":{"type":"byte","value":0},"extra":{"type":"byte","value":0}}"#,
    ] {
        let (compiled, records, _) = compile_rods_with_state(Some(state));
        let record = records
            .iter()
            .find(|r| r.name.ends_with("lightning_rod"))
            .unwrap();
        let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Sequential, record.sequential_id)
                .kind(),
            VisualKind::Diagnostic,
            "{state}"
        );
    }
}
