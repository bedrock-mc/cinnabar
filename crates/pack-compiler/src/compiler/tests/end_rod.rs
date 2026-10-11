use super::*;
use assets::{BlockFlags, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Compiles the active end-rod state product with original sprite artwork.
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
            matches!(
                record.name.as_ref(),
                "minecraft:air" | "minecraft:stone" | "minecraft:end_rod"
            )
        })
        .collect::<Vec<_>>();
    if let Some(state) = state {
        records
            .iter_mut()
            .find(|record| record.name.as_ref() == "minecraft:end_rod")
            .unwrap()
            .canonical_state = state.into();
    }
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path().join("blocks.json"),
        r#"{"stone":{"textures":"stone"},"end_rod":{"textures":"end_rod"}}"#,
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"stone":{"textures":"textures/blocks/stone"},"end_rod":{"textures":"textures/blocks/end_rod"}}}"#,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let mut pixels = vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize];
    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            if (x < 2 && y < 15)
                || ((2..4).contains(&x) && y < 2)
                || ((2..6).contains(&x) && (2..7).contains(&y))
            {
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
    write_png(
        directory.path().join("textures/blocks/end_rod.png"),
        TILE_SIZE,
        TILE_SIZE,
        &pixels,
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
        && let Some(path) = std::env::var_os("CINNABAR_END_ROD_CARRIER")
    {
        fs::write(path, assets::encode_blob(&compiled).unwrap()).unwrap();
    }
    (compiled, records, pixels)
}

#[test]
fn end_rods_reject_unsupported_selectors_instead_of_using_the_fallback_envelope() {
    for state in [
        r#"{}"#,
        r#"{"facing_direction":{"type":"int","value":6}}"#,
        r#"{"facing_direction":{"type":"byte","value":1}}"#,
        r#"{"facing_direction":{"type":"int","value":-1}}"#,
        r#"{"facing_direction":{"type":"int","value":1},"extra":{"type":"byte","value":0}}"#,
    ] {
        let (compiled, records, _) = compile_rods_with_state(Some(state));
        let record = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:end_rod")
            .unwrap();
        let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Diagnostic, "{state}");
        assert_eq!(visual.model_template(), None);
    }
}

#[test]
fn end_rod_states_preserve_complete_shaft_base_and_sprite_after_roundtrip() {
    let (compiled, records, pixels) = compile_rods();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let mut directions = Vec::new();
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:end_rod")
    {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let direction = state["facing_direction"]["value"].as_u64().unwrap() as usize;
        directions.push(direction);
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Model);
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Hashed, record.network_hash)
                .model_template(),
            visual.model_template()
        );
        assert!(
            !visual
                .flags()
                .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        );
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(
            quads.len(),
            12,
            "both shaft and base survive for facing {direction}"
        );
        assert_eq!(visual.support(), VisualSupport::Exact);
        assert_eq!(
            quads[assets::BlockFace::West as usize].uvs[0],
            [
                [512, 0],
                [0, 3840],
                [512, 3840],
                [0, 0],
                [512, 0],
                [1024, 0]
            ][direction],
            "shaft side/cap orientation for facing {direction}"
        );
        assert_eq!(
            quads[6 + assets::BlockFace::Up as usize].uvs[0],
            [
                [1536, 1536],
                [512, 512],
                [1536, 1792],
                [512, 1536],
                [1536, 1536],
                [512, 1536]
            ][direction],
            "mounting plate orientation for facing {direction}"
        );
        let axis = [1, 1, 2, 2, 0, 0][direction];
        let base_at_high_end = [true, false, false, true, false, true][direction];
        for (part, cross, low, high) in [
            (
                0,
                (112, 144),
                if base_at_high_end { 0 } else { 16 },
                if base_at_high_end { 240 } else { 256 },
            ),
            (
                1,
                (96, 160),
                if base_at_high_end { 240 } else { 0 },
                if base_at_high_end { 256 } else { 16 },
            ),
        ] {
            let part_quads = &quads[part * 6..part * 6 + 6];
            for coordinate in 0..3 {
                let positions = part_quads
                    .iter()
                    .flat_map(|q| q.positions)
                    .map(|p| p[coordinate])
                    .collect::<Vec<_>>();
                let expected = if coordinate == axis {
                    (low, high)
                } else {
                    cross
                };
                assert_eq!(
                    (
                        positions.iter().min().copied().unwrap(),
                        positions.iter().max().copied().unwrap()
                    ),
                    expected
                );
            }
            for quad in part_quads {
                assert_eq!(quad.flags & assets::MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
                let uv =
                    [0, 1].map(|a| quad.uvs.iter().map(|uv| u32::from(uv[a])).sum::<u32>() / 4);
                let [x, y] = uv.map(|v| v * TILE_SIZE / 4096);
                assert_eq!(
                    pixels[((y * TILE_SIZE + x) * 4 + 3) as usize],
                    255,
                    "facing {direction} part {part} samples its opaque sprite"
                );
                assert_eq!(
                    runtime.material(quad.material).flags,
                    assets::MATERIAL_FLAG_ALPHA_CUTOUT
                );
            }
        }
    }
    directions.sort_unstable();
    assert_eq!(directions, [0, 1, 2, 3, 4, 5]);
}
