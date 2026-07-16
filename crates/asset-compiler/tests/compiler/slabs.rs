use super::support::*;

fn generated_slab_record(
    sequential_id: u32,
    network_hash: u32,
    name: &str,
    half: u32,
) -> RegistryRecord {
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let mut record = records
        .into_iter()
        .find(|record| {
            record.model_family == ModelFamily::Slab
                && record.model_state.get(ModelStateField::Half) == Some(half)
        })
        .unwrap_or_else(|| panic!("missing generated slab half={half}"));
    record.sequential_id = sequential_id;
    record.network_hash = network_hash;
    record.name = name.into();
    record.canonical_state = "{}".into();
    record
}

fn slab_record_with_replaced_half(mut record: RegistryRecord, half: u32) -> RegistryRecord {
    record.collision_seed = CollisionSeed::default();
    record.provenance = RegistryProvenance::DRAGONFLY;
    let mut bytes = registry_bytes(std::slice::from_ref(&record));
    const REGISTRY_HEADER_BYTES: usize = 8 + 7 * 4;
    const RECORD_FIXED_PREFIX_BYTES: usize = 24;
    const HALF_VALUE_OFFSET: usize = REGISTRY_HEADER_BYTES + RECORD_FIXED_PREFIX_BYTES + 4;
    assert_ne!(bytes[REGISTRY_HEADER_BYTES + 11] & (1 << 1), 0);
    bytes[HALF_VALUE_OFFSET..HALF_VALUE_OFFSET + 4].copy_from_slice(&half.to_le_bytes());
    read_registry(&bytes)
        .expect("decode half-mutated slab fixture")
        .into_iter()
        .next()
        .expect("one half-mutated slab fixture")
}

fn write_slab_pack(root: &Path) {
    write_pack(
        root,
        r#"{
            "test_slab":{"textures":{"down":"slab_down","side":"slab_side","up":"slab_up"}},
            "test_double_slab":{"textures":{"down":"slab_down","side":"slab_side","up":"slab_up"}}
        }"#,
        r#"{"texture_data":{
            "slab_down":{"textures":"textures/blocks/slab_down"},
            "slab_side":{"textures":"textures/blocks/slab_side"},
            "slab_up":{"textures":"textures/blocks/slab_up"}
        }}"#,
        "[]",
    );
    for (path, colour) in [
        ("slab_down", [21, 41, 61, 255]),
        ("slab_side", [81, 101, 121, 255]),
        ("slab_up", [141, 161, 181, 255]),
    ] {
        write_png(
            root,
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, colour),
        );
    }
}

fn expected_slab_quads(materials: [u32; 6], min_y: i16, max_y: i16) -> [ModelQuad; 6] {
    let min_v = u16::try_from(4096 - i32::from(min_y) * 16).expect("bounded slab min V");
    let max_v = u16::try_from(4096 - i32::from(max_y) * 16).expect("bounded slab max V");
    let vertical_standard = [[0, min_v], [4096, min_v], [4096, max_v], [0, max_v]];
    let vertical_transposed = [[0, min_v], [0, max_v], [4096, max_v], [4096, min_v]];
    let horizontal_standard = [[0, 0], [4096, 0], [4096, 4096], [0, 4096]];
    let horizontal_transposed = [[0, 0], [0, 4096], [4096, 4096], [4096, 0]];
    let flagged = |face: u32, boundary: bool| face | (u32::from(boundary) * (face << 4));
    [
        ModelQuad {
            positions: [
                [0, min_y, 0],
                [0, min_y, 256],
                [0, max_y, 256],
                [0, max_y, 0],
            ],
            uvs: vertical_standard,
            material: materials[BlockFace::West as usize],
            flags: flagged(3, true),
        },
        ModelQuad {
            positions: [
                [256, min_y, 0],
                [256, max_y, 0],
                [256, max_y, 256],
                [256, min_y, 256],
            ],
            uvs: vertical_transposed,
            material: materials[BlockFace::East as usize],
            flags: flagged(4, true),
        },
        ModelQuad {
            positions: [
                [0, min_y, 0],
                [256, min_y, 0],
                [256, min_y, 256],
                [0, min_y, 256],
            ],
            uvs: horizontal_standard,
            material: materials[BlockFace::Down as usize],
            flags: flagged(1, min_y == 0),
        },
        ModelQuad {
            positions: [
                [0, max_y, 0],
                [0, max_y, 256],
                [256, max_y, 256],
                [256, max_y, 0],
            ],
            uvs: horizontal_transposed,
            material: materials[BlockFace::Up as usize],
            flags: flagged(2, max_y == 256),
        },
        ModelQuad {
            positions: [
                [0, min_y, 0],
                [0, max_y, 0],
                [256, max_y, 0],
                [256, min_y, 0],
            ],
            uvs: vertical_transposed,
            material: materials[BlockFace::North as usize],
            flags: flagged(5, true),
        },
        ModelQuad {
            positions: [
                [0, min_y, 256],
                [256, min_y, 256],
                [256, max_y, 256],
                [0, max_y, 256],
            ],
            uvs: vertical_standard,
            material: materials[BlockFace::South as usize],
            flags: flagged(6, true),
        },
    ]
}

#[test]
fn compiler_slab_templates_match_exact_exterior_positions_uvs_materials_and_flags() {
    let directory = tempfile::tempdir().expect("create slab geometry fixture");
    write_slab_pack(directory.path());
    let records = [
        generated_slab_record(0, 20_000, "minecraft:test_slab", 0),
        generated_slab_record(1, 20_001, "minecraft:test_slab", 1),
        generated_slab_record(2, 20_002, "minecraft:test_double_slab", 2),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile exact slab geometry");
    assert_eq!(compiled.materials.len(), 4, "diagnostic plus down/side/up");
    let expected_digests = [
        "3b7f0f1e69d4254dee7b6454a76e3aac55c91208b014c174e6627a5980ff2d57",
        "3e687b84eebc0b0c72d2454918f0112aa04301702570abe9d59cdd6e2be84c21",
        "f50037c8ed2c82dad3727accf4be0b17de464f3432810573955bdd81b0b6837c",
    ];
    for (id, bounds) in [(0, (0, 128)), (1, (128, 256)), (2, (0, 256))] {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "slab id={id}");
        assert!(
            !visual
                .flags
                .intersects(BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL)
        );
        assert_eq!(
            visual.flags.contains(BlockFlags::OCCLUDES_FULL_FACE),
            id == 2,
            "only the full slab is a full-face occluder"
        );
        let actual = compiled_model_quads(&compiled, id);
        let expected = expected_slab_quads(visual.faces, bounds.0, bounds.1);
        assert_eq!(actual, expected, "slab id={id} exact quad contract");
        assert_eq!(actual.len(), 6);
        assert_eq!(
            actual.iter().map(|quad| quad.flags).collect::<Vec<_>>(),
            match id {
                0 => vec![0x33, 0x44, 0x11, 0x02, 0x55, 0x66],
                1 => vec![0x33, 0x44, 0x01, 0x22, 0x55, 0x66],
                2 => vec![0x33, 0x44, 0x11, 0x22, 0x55, 0x66],
                _ => unreachable!(),
            },
            "only block-boundary faces carry cull-face flags"
        );
        assert_eq!(
            actual[0].uvs,
            match id {
                0 => [[0, 4096], [4096, 4096], [4096, 2048], [0, 2048]],
                1 => [[0, 2048], [4096, 2048], [4096, 0], [0, 0]],
                2 => [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
                _ => unreachable!(),
            },
            "west side uses the vanilla lower/upper/full vertical crop"
        );
        assert_eq!(
            actual[1].uvs,
            match id {
                0 => [[0, 4096], [0, 2048], [4096, 2048], [4096, 4096]],
                1 => [[0, 2048], [0, 0], [4096, 0], [4096, 2048]],
                2 => [[0, 4096], [0, 0], [4096, 0], [4096, 4096]],
                _ => unreachable!(),
            },
            "east side preserves the transposed cube-face orientation"
        );
        assert_eq!(actual[2].uvs, [[0, 0], [4096, 0], [4096, 4096], [0, 4096]]);
        assert_eq!(actual[3].uvs, [[0, 0], [0, 4096], [4096, 4096], [4096, 0]]);
        assert_eq!(
            slab_geometry_digest(actual),
            expected_digests[id],
            "slab id={id} position/UV/flag digest"
        );
        assert_eq!(slab_geometry_digest(&expected), expected_digests[id]);
        for (face, quad) in actual.iter().enumerate() {
            assert_eq!(quad.material, visual.faces[face]);
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
            assert!((1..=6).contains(&(quad.flags & MODEL_QUAD_FLAG_FACE_MASK)));
            assert_eq!(
                quad.flags & !(MODEL_QUAD_FLAG_FACE_MASK | MODEL_QUAD_FLAG_CULL_FACE_MASK),
                0
            );
            assert_eq!(compiled.materials[quad.material as usize].flags, 0);
        }
        assert_eq!(
            visual.faces[BlockFace::West as usize],
            visual.faces[BlockFace::East as usize]
        );
        assert_eq!(
            visual.faces[BlockFace::West as usize],
            visual.faces[BlockFace::North as usize]
        );
        assert_eq!(
            visual.faces[BlockFace::West as usize],
            visual.faces[BlockFace::South as usize]
        );
        assert_ne!(
            visual.faces[BlockFace::Down as usize],
            visual.faces[BlockFace::Up as usize]
        );
        assert_ne!(
            visual.faces[BlockFace::Down as usize],
            visual.faces[BlockFace::West as usize]
        );
        assert_ne!(
            visual.faces[BlockFace::Up as usize],
            visual.faces[BlockFace::West as usize]
        );
    }
}

#[test]
fn compiler_slab_half_is_typed_fail_closed_and_ignores_collision_only_boxes() {
    let directory = tempfile::tempdir().expect("create slab half fixture");
    write_slab_pack(directory.path());
    let baseline = generated_slab_record(0, 21_000, "minecraft:test_slab", 0);
    let mut collision_only = generated_slab_record(1, 21_001, "minecraft:test_slab", 0);
    collision_only.collision_seed = CollisionSeed {
        shape_id: 65_000,
        confidence: CollisionConfidence::CollisionOnly,
        boxes: vec![CollisionBox {
            min_x: 25_000_000,
            min_y: 25_000_000,
            min_z: 25_000_000,
            max_x: 75_000_000,
            max_y: 75_000_000,
            max_z: 75_000_000,
        }]
        .into_boxed_slice(),
    };
    let missing = model_record(
        2,
        21_002,
        "minecraft:test_slab",
        r#"{"vertical_half":{"type":"int","value":0}}"#,
        ModelFamily::Slab,
    );
    let malformed = slab_record_with_replaced_half(
        generated_slab_record(3, 21_003, "minecraft:test_slab", 0),
        3,
    );

    let compiled = compile_pack(
        directory.path(),
        &[baseline, collision_only, missing, malformed],
    )
    .expect("compile fail-closed slab half fixture");
    assert_eq!(
        compiled.visuals[0].model_template, compiled.visuals[1].model_template,
        "collision-only boxes must not select render geometry"
    );
    for id in [2, 3] {
        assert_eq!(compiled.visuals[id].kind, VisualKind::Diagnostic, "id={id}");
        assert_eq!(
            compiled.visuals[id].model_template,
            assets::NO_MODEL_TEMPLATE
        );
    }
}

#[test]
fn compiler_covers_all_272_breg_slab_states_with_three_deduplicated_stable_templates() {
    let directory = tempfile::tempdir().expect("create exhaustive slab fixture");
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.model_family == ModelFamily::Slab)
    .collect::<Vec<_>>();
    assert_eq!(records.len(), 272);
    let mut half_counts = [0_usize; 3];
    let mut blocks = serde_json::Map::new();
    for record in &records {
        let half = record
            .model_state
            .get(ModelStateField::Half)
            .expect("every generated slab has typed half");
        half_counts[half as usize] += 1;
        blocks.insert(
            record
                .name
                .strip_prefix("minecraft:")
                .unwrap_or(&record.name)
                .to_owned(),
            serde_json::json!({"textures":"slab_all"}),
        );
    }
    assert_eq!(half_counts, [68, 68, 136]);
    write_pack(
        directory.path(),
        &serde_json::Value::Object(blocks).to_string(),
        r#"{"texture_data":{"slab_all":{"textures":"textures/blocks/slab_all"}}}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/slab_all",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [73, 109, 151, 255]),
    );

    let compiled = compile_pack(directory.path(), &records).expect("compile all BREG slabs");
    assert_eq!(compiled.model_templates.len(), 3);
    assert_eq!(compiled.model_quads.len(), 18);
    let mut template_by_half = [HashSet::new(), HashSet::new(), HashSet::new()];
    for record in &records {
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.name);
        assert_ne!(visual.model_template, assets::NO_MODEL_TEMPLATE);
        assert!(
            visual
                .faces
                .into_iter()
                .all(|material| material != DIAGNOSTIC_MATERIAL),
            "{} retained a diagnostic face",
            record.name
        );
        let half = record.model_state.get(ModelStateField::Half).unwrap() as usize;
        template_by_half[half].insert(visual.model_template);
    }
    assert!(
        template_by_half
            .iter()
            .all(|templates| templates.len() == 1)
    );
    assert_eq!(template_by_half[2].len(), 1, "all double slabs deduplicate");

    let baseline = encode_blob(&compiled).expect("encode exhaustive slab baseline");
    let reversed = records.iter().cloned().rev().collect::<Vec<_>>();
    let reversed = compile_pack(directory.path(), &reversed).expect("compile reversed BREG slabs");
    assert_eq!(
        encode_blob(&reversed).expect("encode reversed slabs"),
        baseline
    );
    let runtime = RuntimeAssets::decode(&baseline).expect("decode exhaustive slab blob");
    assert_eq!(runtime.model_templates(), compiled.model_templates.as_ref());
    assert_eq!(runtime.model_quads(), compiled.model_quads.as_ref());
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_slab_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile requested pinned pack");
    let slabs = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Slab)
        .collect::<Vec<_>>();
    assert_eq!(slabs.len(), 272);
    let diagnostic = slabs
        .iter()
        .filter(|record| {
            compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
        })
        .map(|record| (record.name.as_ref(), record.canonical_state.as_ref()))
        .collect::<Vec<_>>();
    assert!(
        diagnostic.is_empty(),
        "pinned pack retained diagnostic slabs: {diagnostic:?}"
    );
    assert_eq!(
        slabs
            .iter()
            .map(|record| compiled.visuals[record.sequential_id as usize].model_template)
            .collect::<HashSet<_>>()
            .len(),
        189,
        "pinned slab material/half templates"
    );
}
