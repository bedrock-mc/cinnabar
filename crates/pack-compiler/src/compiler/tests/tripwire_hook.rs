use super::*;
use assets::{BlockFace, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Compiles registered hook states against independently identifiable original sprites.
fn compile_hooks(state: Option<&str>, color: u8) -> (RuntimeAssets, Vec<RegistryRecord>) {
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
                "minecraft:air" | "minecraft:stone" | "minecraft:tripwire_hook"
            )
        })
        .collect();
    if let Some(state) = state {
        records
            .iter_mut()
            .find(|r| r.name.as_ref() == "minecraft:tripwire_hook")
            .unwrap()
            .canonical_state = state.into();
    }
    let directory = tempfile::tempdir().unwrap();
    let mut textures = serde_json::Map::new();
    for (index, name) in ["base", "hook", "wire"].into_iter().enumerate() {
        textures.insert(
            name.into(),
            serde_json::json!({"textures": format!("textures/{name}")}),
        );
        let mut pixels = Vec::new();
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                pixels.extend_from_slice(&[
                    color + index as u8 * 60,
                    x as u8 * 8,
                    y as u8 * 8,
                    255,
                ]);
            }
        }
        write_png(
            directory.path().join(format!("textures/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
    }
    write(
        directory.path().join("blocks.json"),
        r#"{"tripwire_hook":{"textures":{"down":"base","up":"hook","north":"hook","south":"wire","east":"wire","west":"wire"}},"stone":{"textures":"base"}}"#,
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
fn tripwire_hooks_keep_mount_shaft_eye_and_attached_wire_in_every_state() {
    let (runtime, records) = compile_hooks(None, 40);
    let mut seen = 0;
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:tripwire_hook")
    {
        seen += 1;
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let attached = state["attached_bit"]["value"].as_u64().unwrap() != 0;
        let powered = state["powered_bit"]["value"].as_u64().unwrap() != 0;
        let direction = state["direction"]["value"].as_u64().unwrap() as usize;
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Model);
        assert_eq!(
            visual.support(),
            VisualSupport::Exact,
            "{}",
            record.canonical_state
        );
        let parts = assets::model_template_parts(
            runtime.model_templates(),
            visual.model_template().unwrap(),
        )
        .unwrap();
        let count: usize = parts.iter().map(|p| p.quad_count as usize).sum();
        let quads = &runtime.model_quads()[parts[0].quad_start as usize..][..count];
        assert_eq!(
            count,
            if attached { 20 } else { 18 },
            "{}",
            record.canonical_state
        );
        let base = visual.face(BlockFace::Down).material_id();
        let hook = visual.face(BlockFace::Up).material_id();
        let wire = visual.face(BlockFace::South).material_id();
        assert!(quads[..6].iter().all(|q| q.material == base));
        assert!(quads[6..18].iter().all(|q| q.material == hook));
        for part in quads[6..18].as_chunks::<6>().0 {
            let center: [f64; 3] = std::array::from_fn(|axis| {
                part.iter()
                    .flat_map(|q| q.positions)
                    .map(|p| f64::from(p[axis]))
                    .sum::<f64>()
                    / 24.0
            });
            for quad in part {
                let [a, b, c, _] = quad.positions.map(|p| p.map(f64::from));
                let ab: [f64; 3] = std::array::from_fn(|axis| b[axis] - a[axis]);
                let ac: [f64; 3] = std::array::from_fn(|axis| c[axis] - a[axis]);
                let normal = [
                    ab[1] * ac[2] - ab[2] * ac[1],
                    ab[2] * ac[0] - ab[0] * ac[2],
                    ab[0] * ac[1] - ab[1] * ac[0],
                ];
                let outward: f64 = (0..3)
                    .map(|axis| normal[axis] * (a[axis] - center[axis]))
                    .sum();
                assert!(outward > 0.0, "inward cap or side: {quad:?}");
            }
        }
        assert!(quads[18..].iter().all(|q| q.material == wire));
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Hashed, record.network_hash)
                .model_template(),
            visual.model_template()
        );
        let bounds: [[i16; 2]; 3] = std::array::from_fn(|axis| {
            [
                quads[..6]
                    .iter()
                    .flat_map(|q| q.positions)
                    .map(|p| p[axis])
                    .min()
                    .unwrap(),
                quads[..6]
                    .iter()
                    .flat_map(|q| q.positions)
                    .map(|p| p[axis])
                    .max()
                    .unwrap(),
            ]
        });
        assert_eq!(
            bounds,
            [
                [[96, 160], [13, 141], [0, 32]],
                [[224, 256], [13, 141], [96, 160]],
                [[96, 160], [13, 141], [224, 256]],
                [[0, 32], [13, 141], [96, 160]],
            ][direction]
        );
        if direction == 0 {
            let pose = if powered {
                2
            } else if attached {
                1
            } else {
                0
            };
            assert_eq!(
                quads[6].positions[0],
                [[140, 83, 29], [140, 84, 16], [140, 83, 18]][pose]
            );
            assert_eq!(
                quads[12].positions[0],
                [[152, 105, 107], [152, 82, 141], [152, 56, 138]][pose]
            );
            if attached {
                assert_eq!(
                    quads[18].positions[2],
                    [132, if powered { 56 } else { 82 }, 128]
                );
                assert_eq!(quads[19].positions[0], [124, 56, 256]);
            }
        }
        assert_eq!(
            quads[6].uvs,
            [[1792, 2816], [2304, 2816], [2304, 2304], [1792, 2304]]
        );
        assert_eq!(
            quads[12].uvs,
            [[1280, 2304], [2816, 2304], [2816, 768], [1280, 768]]
        );
        for material in [base, hook, wire] {
            let flags = runtime.material(material).flags;
            assert_ne!(flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT, 0);
            assert_eq!(flags & assets::MATERIAL_FLAG_ALPHA_BLEND, 0);
        }
    }
    assert_eq!(seen, 16);
}

#[test]
fn tripwire_hooks_reject_missing_mistyped_and_out_of_range_selectors() {
    for state in [
        r#"{"direction":{"type":"int","value":4},"attached_bit":{"type":"byte","value":0},"powered_bit":{"type":"byte","value":0}}"#,
        r#"{"direction":{"type":"int","value":0},"attached_bit":{"type":"byte","value":2},"powered_bit":{"type":"byte","value":0}}"#,
        r#"{"direction":{"type":"int","value":0},"attached_bit":{"type":"byte","value":0},"powered_bit":{"type":"int","value":1}}"#,
        r#"{"direction":{"type":"int","value":0},"attached_bit":{"type":"byte","value":0}}"#,
    ] {
        let (runtime, records) = compile_hooks(Some(state), 40);
        let record = records
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:tripwire_hook")
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

#[test]
fn tripwire_hook_replacement_art_keeps_geometry_and_independent_part_materials() {
    let (first, records) = compile_hooks(None, 40);
    let (replacement, _) = compile_hooks(None, 70);
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:tripwire_hook")
    {
        let visuals = [&first, &replacement]
            .map(|runtime| runtime.resolve(NetworkIdMode::Sequential, record.sequential_id));
        let meshes = [&first, &replacement]
            .into_iter()
            .zip(visuals)
            .map(|(runtime, visual)| {
                let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
                &runtime.model_quads()[template.quad_start as usize..]
                    [..template.quad_count as usize]
            })
            .collect::<Vec<_>>();
        assert_eq!(meshes[0], meshes[1]);
        for (runtime, visual, color) in [(&first, visuals[0], 40), (&replacement, visuals[1], 70)] {
            for (index, face) in [BlockFace::Down, BlockFace::Up, BlockFace::South]
                .into_iter()
                .enumerate()
            {
                let texture = runtime.material(visual.face(face).material_id()).texture;
                let offset = texture.layer() as usize * TILE_SIZE as usize * TILE_SIZE as usize * 4;
                assert_eq!(
                    runtime.texture_array().mips[0].rgba8[offset],
                    color + index as u8 * 60
                );
            }
        }
    }
}
