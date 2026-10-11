use super::*;
use assets::{BlockFace, BlockFlags, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Compiles the shipped sea pickle identities against observable original sprite pixels.
fn compile_pickles(state: Option<&str>, color: u8) -> (RuntimeAssets, Vec<RegistryRecord>) {
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
                "minecraft:air" | "minecraft:stone" | "minecraft:sea_pickle" | "minecraft:water"
            )
        })
        .collect();
    if let Some(state) = state {
        records
            .iter_mut()
            .find(|r| r.name.as_ref() == "minecraft:sea_pickle")
            .unwrap()
            .canonical_state = state.into();
    }
    let directory = tempfile::tempdir().unwrap();
    let pixels: Vec<_> = (0..TILE_SIZE)
        .flat_map(|y| {
            (0..TILE_SIZE).flat_map(move |x| {
                [
                    color,
                    x as u8 * 8,
                    y as u8 * 8,
                    if (5..=6).contains(&x) && (2..=3).contains(&y) {
                        0
                    } else {
                        255
                    },
                ]
            })
        })
        .collect();
    write_png(
        directory.path().join("textures/pickle.png"),
        TILE_SIZE,
        TILE_SIZE,
        &pixels,
    );
    write(
        directory.path().join("blocks.json"),
        r#"{"sea_pickle":{"textures":"pickle"},"stone":{"textures":"pickle"},"water":{"textures":"water"}}"#,
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"pickle":{"textures":"textures/pickle"},"water":{"textures":"textures/pickle"}}}"#,
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

#[test]
fn sea_pickle_clusters_keep_individual_bodies_and_only_live_tips() {
    let (runtime, records) = compile_pickles(None, 40);
    let mut seen = 0;
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:sea_pickle")
    {
        seen += 1;
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let count = state["cluster_count"]["value"].as_u64().unwrap() as usize + 1;
        let dry = state["dead_bit"]["value"].as_u64().unwrap() != 0;
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Model);
        assert_eq!(
            visual.light_properties().emission(),
            if dry { 0 } else { 3 * count as u8 + 3 }
        );
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
        let parts = assets::model_template_parts(
            runtime.model_templates(),
            visual.model_template().unwrap(),
        )
        .unwrap();
        assert!(
            parts
                .iter()
                .all(|p| p.quad_count as usize <= assets::MAX_MODEL_TEMPLATE_QUADS)
        );
        let template = parts[0];
        let quad_count: usize = parts.iter().map(|p| p.quad_count as usize).sum();
        let quads = &runtime.model_quads()[template.quad_start as usize..][..quad_count];
        assert_eq!(
            quads.len(),
            count * if dry { 7 } else { 9 },
            "{count}/{dry}"
        );
        assert_eq!(
            quads
                .iter()
                .filter(|q| q.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED != 0)
                .count(),
            if dry { 0 } else { count * 2 }
        );
        let material = visual.face(BlockFace::Up).material_id();
        assert!(quads.iter().all(|q| q.material == material));
        let texture = runtime.material(material).texture;
        let pixels = &runtime.texture_array().mips[0].rgba8;
        let alpha = |x: usize, y: usize| {
            pixels[(texture.layer() as usize * TILE_SIZE as usize * TILE_SIZE as usize
                + y * TILE_SIZE as usize
                + x)
                * 4
                + 3]
        };
        assert_eq!(alpha(6, 3), 0, "upper cap has a transparent opening");
        assert_eq!(alpha(10, 3), 255, "inset cap samples the opaque interior");
        assert_ne!(
            runtime.material(material).flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT,
            0
        );
        assert_eq!(
            runtime.material(material).flags & assets::MATERIAL_FLAG_ALPHA_BLEND,
            0
        );
        for body in quads.chunks(if dry { 7 } else { 9 }) {
            let bounds: [[i16; 2]; 3] = std::array::from_fn(|axis| {
                [
                    body[..6]
                        .iter()
                        .flat_map(|q| q.positions)
                        .map(|p| p[axis])
                        .min()
                        .unwrap(),
                    body[..6]
                        .iter()
                        .flat_map(|q| q.positions)
                        .map(|p| p[axis])
                        .max()
                        .unwrap(),
                ]
            });
            assert_eq!(bounds[0][1] - bounds[0][0], 64);
            assert_eq!(bounds[2][1] - bounds[2][0], 64);
            assert_eq!(bounds[1][0], 0);
            assert!((62..=111).contains(&bounds[1][1]));
            if count == 1 {
                assert_eq!(bounds, [[96, 160], [0, 96], [96, 160]]);
            }
            assert_eq!(
                body[BlockFace::Up as usize]
                    .uvs
                    .iter()
                    .map(|uv| uv[0])
                    .min(),
                Some(1024)
            );
            assert_eq!(
                body[BlockFace::Up as usize]
                    .uvs
                    .iter()
                    .map(|uv| uv[0])
                    .max(),
                Some(2048)
            );
            assert_eq!(
                body[BlockFace::Down as usize]
                    .uvs
                    .iter()
                    .map(|uv| uv[0])
                    .min(),
                Some(2048)
            );
            assert_eq!(
                body[BlockFace::Down as usize]
                    .uvs
                    .iter()
                    .map(|uv| uv[0])
                    .max(),
                Some(3072)
            );
            let cap = &body[6];
            let inset_y = cap.positions[0][1];
            assert!(cap.positions.iter().all(|p| p[1] == inset_y));
            assert!((12..=13).contains(&(bounds[1][1] - inset_y)));
            assert_eq!(cap.flags & assets::MODEL_QUAD_FLAG_FACE_MASK, 2);
            assert_eq!(cap.uvs.iter().map(|uv| uv[0]).min(), Some(2048));
            assert_eq!(cap.uvs.iter().map(|uv| uv[0]).max(), Some(3072));
            if !dry {
                assert!(
                    body[7..]
                        .iter()
                        .flat_map(|q| q.positions)
                        .any(|p| p[1] > bounds[1][1])
                );
                assert!(body[7..].iter().flat_map(|q| q.uvs).all(|uv| uv[1] <= 1280));
            }
        }
    }
    assert_eq!(seen, 8);
    for water in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:water")
    {
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Sequential, water.sequential_id)
                .kind(),
            VisualKind::Liquid
        );
    }
}

#[test]
fn sea_pickle_replacements_retain_geometry_and_sample_new_pixels() {
    let (first, records) = compile_pickles(None, 40);
    let (replacement, _) = compile_pickles(None, 90);
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:sea_pickle")
    {
        let a = first.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let b = replacement.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(
            a.face(BlockFace::Up).material_id(),
            b.face(BlockFace::Up).material_id()
        );
        assert_ne!(
            first.texture_array().mips[0].rgba8,
            replacement.texture_array().mips[0].rgba8
        );
        assert_eq!(a.model_template(), b.model_template());
    }
}

#[test]
fn sea_pickles_reject_malformed_count_and_liveness_states() {
    for state in [
        r#"{"cluster_count":{"type":"int","value":4},"dead_bit":{"type":"byte","value":0}}"#,
        r#"{"cluster_count":{"type":"byte","value":1},"dead_bit":{"type":"byte","value":0}}"#,
        r#"{"cluster_count":{"type":"int","value":1},"dead_bit":{"type":"int","value":0}}"#,
        r#"{"cluster_count":{"type":"int","value":1},"dead_bit":{"type":"byte","value":2}}"#,
        r#"{"cluster_count":{"type":"int","value":-1},"dead_bit":{"type":"byte","value":0}}"#,
        r#"{"cluster_count":{"type":"int","value":1}}"#,
    ] {
        let (runtime, records) = compile_pickles(Some(state), 40);
        let record = records
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:sea_pickle")
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
