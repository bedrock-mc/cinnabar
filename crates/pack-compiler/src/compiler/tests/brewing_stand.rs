use super::*;
use assets::{BlockFace, BlockFlags, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Compiles active brewing-stand identities with independent, observable sprite sheets.
fn compile_stands(state: Option<&str>, red: u8) -> (RuntimeAssets, Vec<RegistryRecord>) {
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
            matches!(
                r.name.as_ref(),
                "minecraft:air" | "minecraft:stone" | "minecraft:brewing_stand"
            )
        })
        .collect();
    if let Some(state) = state {
        records
            .iter_mut()
            .find(|r| r.name.as_ref() == "minecraft:brewing_stand")
            .unwrap()
            .canonical_state = state.into();
    }
    let directory = tempfile::tempdir().unwrap();
    for (name, color) in [
        ("base", [60, 120, 180, 255]),
        ("stand", [red, 120, 60, 255]),
    ] {
        let pixels: Vec<_> = (0..TILE_SIZE)
            .flat_map(|y| {
                (0..TILE_SIZE).flat_map(move |x| {
                    [
                        color[0],
                        x as u8 * 8,
                        y as u8 * 8,
                        if name == "stand" && x == 2 && y == 2 {
                            0
                        } else {
                            255
                        },
                    ]
                })
            })
            .collect();
        write_png(
            directory.path().join(format!("textures/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
    }
    write(
        directory.path().join("blocks.json"),
        r#"{"stone":{"textures":"base"},"brewing_stand":{"textures":{"down":"base","side":"stand","up":"stand"}}}"#,
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"base":{"textures":"textures/base"},"stand":{"textures":"textures/stand"}}}"#,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let count = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &vec![assets::LightProperties::default(); count],
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
fn brewing_stand_retains_post_feet_and_each_bottle_panel() {
    let (runtime, records) = compile_stands(None, 180);
    let mut seen = 0;
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:brewing_stand")
    {
        seen += 1;
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Model);
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Hashed, record.network_hash)
                .model_template(),
            visual.model_template()
        );
        assert_eq!(visual.support(), VisualSupport::Exact);
        assert!(
            !visual
                .flags()
                .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        );
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(quads.len(), 27, "{}", record.canonical_state);
        let base = visual.face(BlockFace::Down).material_id();
        let stand = visual.face(BlockFace::Up).material_id();
        assert_ne!(base, stand);
        assert_eq!(quads[2].material, base);
        assert!(
            quads[..6]
                .iter()
                .enumerate()
                .all(|(index, q)| q.material == if index == 2 { base } else { stand })
        );
        assert_eq!(
            quads[3].uvs,
            [[1792, 1792], [1792, 2304], [2304, 2304], [2304, 1792]]
        );
        assert_eq!(
            quads[9].uvs,
            [[2304, 1280], [2304, 2816], [3840, 2816], [3840, 1280]]
        );
        for material in [base, stand] {
            assert_ne!(
                runtime.material(material).flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT,
                0
            );
        }
        let boxes = [
            ([112, 0, 112], [144, 224, 144]),
            ([144, 0, 80], [240, 32, 176]),
            ([16, 0, 16], [112, 32, 112]),
            ([16, 0, 144], [112, 32, 240]),
        ];
        for (faces, (min, max)) in quads[..24].chunks_exact(6).zip(boxes) {
            for axis in 0..3 {
                let coordinates: Vec<_> = faces
                    .iter()
                    .flat_map(|q| q.positions.iter().map(|p| p[axis]))
                    .collect();
                assert_eq!(*coordinates.iter().min().unwrap(), min[axis]);
                assert_eq!(*coordinates.iter().max().unwrap(), max[axis]);
            }
        }
        assert!(quads[6..24].iter().all(|q| q.material == base));
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        for ((panel, slot), [x, z]) in
            quads[24..]
                .iter()
                .zip(["a", "b", "c"])
                .zip([[256, 128], [37, 37], [37, 219]])
        {
            assert_ne!(panel.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED, 0);
            assert_eq!(panel.material, stand);
            assert_eq!(panel.positions[0], [128, 256, 128]);
            assert_eq!(panel.positions[1], [128, 0, 128]);
            assert_eq!(panel.positions[2], [x, 0, z]);
            assert_eq!(panel.positions[3], [x, 256, z]);
            let occupied = state[format!("brewing_stand_slot_{slot}_bit")]["value"] == 1;
            assert_eq!(
                panel.uvs,
                [
                    [2048, 0],
                    [2048, 4096],
                    [if occupied { 0 } else { 4096 }, 4096],
                    [if occupied { 0 } else { 4096 }, 0]
                ]
            );
        }
    }
    assert_eq!(seen, 8);
}

#[test]
fn brewing_stand_uses_replacement_pixels_and_preserves_alpha() {
    for red in [30, 180] {
        let (runtime, records) = compile_stands(None, red);
        let record = records
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:brewing_stand")
            .unwrap();
        let visual = runtime.resolve(NetworkIdMode::Hashed, record.network_hash);
        let layer = runtime
            .material(visual.face(BlockFace::Up).material_id())
            .texture
            .layer() as usize;
        let start = layer * (TILE_SIZE * TILE_SIZE * 4) as usize;
        let pixel = start + (2 * TILE_SIZE as usize + 2) * 4;
        assert_eq!(
            &runtime.texture_array().mips[0].rgba8[pixel..pixel + 4],
            &[red, 16, 16, 0]
        );
    }
}

#[test]
fn brewing_stand_rejects_unrecognized_or_malformed_selectors() {
    let valid = r#"{"brewing_stand_slot_a_bit":{"type":"byte","value":0},"brewing_stand_slot_b_bit":{"type":"byte","value":0},"brewing_stand_slot_c_bit":{"type":"byte","value":0}}"#;
    for state in [
        "{}".to_owned(),
        valid.replacen("\"byte\"", "\"int\"", 1),
        valid.replacen("\"value\":0", "\"value\":2", 1),
        valid.replacen("\"value\":0", "\"value\":true", 1),
        valid.replacen("slot_a", "slot_d", 1),
    ] {
        let (runtime, records) = compile_stands(Some(&state), 180);
        let record = records
            .iter()
            .find(|r| r.name.as_ref() == "minecraft:brewing_stand")
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
