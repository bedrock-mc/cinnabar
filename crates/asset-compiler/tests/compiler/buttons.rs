use super::support::*;

const BUTTON_PRESSED_FLAG: u32 = 1 << 1;

fn generated_button_records() -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.model_family == ModelFamily::Button)
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 97_000 + id as u32;
    }
    records
}

fn write_button_pack(root: &Path) {
    let mappings = [
        ("acacia_button", "acacia_planks", "planks_acacia"),
        ("bamboo_button", "bamboo_planks", "bamboo_planks"),
        ("birch_button", "birch_planks", "planks_birch"),
        ("cherry_button", "cherry_planks", "cherry_planks"),
        (
            "crimson_button",
            "crimson_planks",
            "huge_fungus/crimson_planks",
        ),
        ("dark_oak_button", "dark_oak_planks", "planks_big_oak"),
        ("jungle_button", "jungle_planks", "planks_jungle"),
        ("mangrove_button", "mangrove_planks", "mangrove_planks"),
        ("pale_oak_button", "pale_oak_planks", "pale_oak_planks"),
        (
            "polished_blackstone_button",
            "polished_blackstone",
            "polished_blackstone",
        ),
        ("spruce_button", "spruce_planks", "planks_spruce"),
        ("stone_button", "stone", "stone"),
        (
            "warped_button",
            "warped_planks",
            "huge_fungus/warped_planks",
        ),
        ("wooden_button", "planks", "planks_oak"),
    ];
    let blocks = mappings
        .iter()
        .map(|(name, key, _)| format!(r#""{name}":{{"textures":"{key}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let terrain = mappings
        .iter()
        .map(|(_, key, path)| {
            if matches!(*key, "planks" | "stone") {
                format!(
                    r#""{key}":{{"textures":["textures/blocks/{path}","textures/blocks/button_unused_variant"]}}"#
                )
            } else {
                format!(r#""{key}":{{"textures":"textures/blocks/{path}"}}"#)
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    write_pack(
        root,
        &format!("{{{blocks}}}"),
        &format!(r#"{{"texture_data":{{{terrain}}}}}"#),
        "[]",
    );
    for (index, (_, _, path)) in mappings.into_iter().enumerate() {
        write_png(
            root,
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 71, 113, 255]),
        );
    }
    write_png(
        root,
        "textures/blocks/button_unused_variant",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [255, 0, 255, 255]),
    );
}

fn button_selector(record: &RegistryRecord) -> (u32, bool) {
    assert_eq!(record.model_state.mask(), 0x81);
    let orientation = record
        .model_state
        .get(ModelStateField::Orientation)
        .expect("button orientation");
    let flags = record
        .model_state
        .get(ModelStateField::Flags)
        .expect("button flags");
    assert!(orientation <= 5);
    assert!(matches!(flags, 0 | BUTTON_PRESSED_FLAG));
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .expect("parse button canonical state");
    assert_eq!(state.len(), 2);
    let pressed = carpet_state_value(&state, "button_pressed_bit", "byte")
        .as_u64()
        .expect("byte pressed bit");
    let facing = carpet_state_value(&state, "facing_direction", "int")
        .as_u64()
        .expect("integer facing direction");
    assert_eq!(facing, u64::from(orientation));
    assert_eq!(pressed == 1, flags == BUTTON_PRESSED_FLAG);
    (orientation, pressed == 1)
}

fn button_expected_bounds(orientation: u32, pressed: bool) -> ([i16; 3], [i16; 3]) {
    let h = if pressed { 16 } else { 32 };
    match orientation {
        0 => ([80, 256 - h, 96], [176, 256, 160]),
        1 => ([80, 0, 96], [176, h, 160]),
        2 => ([80, 96, 256 - h], [176, 160, 256]),
        3 => ([80, 96, 0], [176, 160, h]),
        4 => ([256 - h, 96, 80], [256, 160, 176]),
        5 => ([0, 96, 80], [h, 160, 176]),
        _ => panic!("invalid button orientation {orientation}"),
    }
}

fn button_face_positions(
    face: BlockFace,
    [min_x, min_y, min_z]: [i16; 3],
    [max_x, max_y, max_z]: [i16; 3],
) -> [[i16; 3]; 4] {
    match face {
        BlockFace::West => [
            [min_x, min_y, min_z],
            [min_x, min_y, max_z],
            [min_x, max_y, max_z],
            [min_x, max_y, min_z],
        ],
        BlockFace::East => [
            [max_x, min_y, min_z],
            [max_x, max_y, min_z],
            [max_x, max_y, max_z],
            [max_x, min_y, max_z],
        ],
        BlockFace::Down => [
            [min_x, min_y, min_z],
            [max_x, min_y, min_z],
            [max_x, min_y, max_z],
            [min_x, min_y, max_z],
        ],
        BlockFace::Up => [
            [min_x, max_y, min_z],
            [min_x, max_y, max_z],
            [max_x, max_y, max_z],
            [max_x, max_y, min_z],
        ],
        BlockFace::North => [
            [min_x, min_y, min_z],
            [min_x, max_y, min_z],
            [max_x, max_y, min_z],
            [max_x, min_y, min_z],
        ],
        BlockFace::South => [
            [min_x, min_y, max_z],
            [max_x, min_y, max_z],
            [max_x, max_y, max_z],
            [min_x, max_y, max_z],
        ],
    }
}

fn button_face_uvs(face: BlockFace, [u1, v1, u2, v2]: [u16; 4]) -> [[u16; 2]; 4] {
    let [u1, v1, u2, v2] = [u1, v1, u2, v2].map(|value| value * 256);
    match face {
        BlockFace::West | BlockFace::South => [[u1, v2], [u2, v2], [u2, v1], [u1, v1]],
        BlockFace::East | BlockFace::North => [[u1, v2], [u1, v1], [u2, v1], [u2, v2]],
        BlockFace::Down => [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        BlockFace::Up => [[u1, v1], [u1, v2], [u2, v2], [u2, v1]],
    }
}

fn button_source_rect(face: BlockFace, pressed: bool) -> [u16; 4] {
    match face {
        BlockFace::Down | BlockFace::Up => [5, 6, 11, 10],
        BlockFace::North | BlockFace::South => [5, 14, 11, if pressed { 15 } else { 16 }],
        BlockFace::West | BlockFace::East => [6, 14, 10, if pressed { 15 } else { 16 }],
    }
}

fn expected_button_uvlock_rect(
    face: BlockFace,
    [min_x, min_y, min_z]: [i16; 3],
    [max_x, max_y, max_z]: [i16; 3],
) -> [u16; 4] {
    let [min_x, min_y, min_z, max_x, max_y, max_z] =
        [min_x, min_y, min_z, max_x, max_y, max_z].map(|value| value as u16 / 16);
    match face {
        BlockFace::West | BlockFace::East => [min_z, 16 - max_y, max_z, 16 - min_y],
        BlockFace::North | BlockFace::South => [min_x, 16 - max_y, max_x, 16 - min_y],
        BlockFace::Down | BlockFace::Up => [min_x, min_z, max_x, max_z],
    }
}

fn button_rotated_face(face: BlockFace, orientation: u32) -> BlockFace {
    let after_x90 = match face {
        BlockFace::West => BlockFace::West,
        BlockFace::East => BlockFace::East,
        BlockFace::Down => BlockFace::South,
        BlockFace::Up => BlockFace::North,
        BlockFace::North => BlockFace::Down,
        BlockFace::South => BlockFace::Up,
    };
    let yaw = |face, turns| {
        let mut face = face;
        for _ in 0..turns {
            face = match face {
                BlockFace::North => BlockFace::East,
                BlockFace::East => BlockFace::South,
                BlockFace::South => BlockFace::West,
                BlockFace::West => BlockFace::North,
                vertical => vertical,
            };
        }
        face
    };
    match orientation {
        0 => match face {
            BlockFace::Down => BlockFace::Up,
            BlockFace::Up => BlockFace::Down,
            BlockFace::North => BlockFace::South,
            BlockFace::South => BlockFace::North,
            horizontal => horizontal,
        },
        1 => face,
        2 => after_x90,
        3 => yaw(after_x90, 2),
        4 => yaw(after_x90, 3),
        5 => yaw(after_x90, 1),
        _ => panic!("invalid button orientation {orientation}"),
    }
}

fn button_rotate_position([x, y, z]: [i16; 3], orientation: u32) -> [i16; 3] {
    match orientation {
        0 => [x, 256 - y, 256 - z],
        1 => [x, y, z],
        2 => [x, z, 256 - y],
        3 => [256 - x, z, y],
        4 => [256 - y, z, 256 - x],
        5 => [y, z, x],
        _ => panic!("invalid button orientation {orientation}"),
    }
}

type ExpectedButtonQuad = (u32, [[i16; 3]; 4], [[u16; 2]; 4]);

fn expected_button_quads(orientation: u32, pressed: bool) -> [ExpectedButtonQuad; 6] {
    let h = if pressed { 16 } else { 32 };
    let source_min = [80, 0, 96];
    let source_max = [176, h, 160];
    let (target_min, target_max) = button_expected_bounds(orientation, pressed);
    BlockFace::ALL.map(|source_face| {
        let target_face = button_rotated_face(source_face, orientation);
        let source_positions = button_face_positions(source_face, source_min, source_max);
        let positions = if orientation <= 1 {
            source_positions.map(|position| button_rotate_position(position, orientation))
        } else {
            button_face_positions(target_face, target_min, target_max)
        };
        let source_uvs = button_face_uvs(source_face, button_source_rect(source_face, pressed));
        let uvs = if orientation <= 1 {
            source_uvs
        } else {
            button_face_uvs(
                target_face,
                expected_button_uvlock_rect(target_face, target_min, target_max),
            )
        };
        (target_face as u32, positions, uvs)
    })
}

fn wall_button_uv_golden(orientation: u32, pressed: bool) -> [[[u16; 2]; 4]; 6] {
    match (orientation, pressed) {
        (2, false) => [
            [[3584, 2560], [4096, 2560], [4096, 1536], [3584, 1536]],
            [[3584, 2560], [3584, 1536], [4096, 1536], [4096, 2560]],
            [[1280, 3584], [2816, 3584], [2816, 4096], [1280, 4096]],
            [[1280, 3584], [1280, 4096], [2816, 4096], [2816, 3584]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
        ],
        (2, true) => [
            [[3840, 2560], [4096, 2560], [4096, 1536], [3840, 1536]],
            [[3840, 2560], [3840, 1536], [4096, 1536], [4096, 2560]],
            [[1280, 3840], [2816, 3840], [2816, 4096], [1280, 4096]],
            [[1280, 3840], [1280, 4096], [2816, 4096], [2816, 3840]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
        ],
        (3, false) => [
            [[0, 2560], [512, 2560], [512, 1536], [0, 1536]],
            [[0, 2560], [0, 1536], [512, 1536], [512, 2560]],
            [[1280, 0], [2816, 0], [2816, 512], [1280, 512]],
            [[1280, 0], [1280, 512], [2816, 512], [2816, 0]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
        ],
        (3, true) => [
            [[0, 2560], [256, 2560], [256, 1536], [0, 1536]],
            [[0, 2560], [0, 1536], [256, 1536], [256, 2560]],
            [[1280, 0], [2816, 0], [2816, 256], [1280, 256]],
            [[1280, 0], [1280, 256], [2816, 256], [2816, 0]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
        ],
        (4, false) => [
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[3584, 1280], [4096, 1280], [4096, 2816], [3584, 2816]],
            [[3584, 1280], [3584, 2816], [4096, 2816], [4096, 1280]],
            [[3584, 2560], [3584, 1536], [4096, 1536], [4096, 2560]],
            [[3584, 2560], [4096, 2560], [4096, 1536], [3584, 1536]],
        ],
        (4, true) => [
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[3840, 1280], [4096, 1280], [4096, 2816], [3840, 2816]],
            [[3840, 1280], [3840, 2816], [4096, 2816], [4096, 1280]],
            [[3840, 2560], [3840, 1536], [4096, 1536], [4096, 2560]],
            [[3840, 2560], [4096, 2560], [4096, 1536], [3840, 1536]],
        ],
        (5, false) => [
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[0, 1280], [512, 1280], [512, 2816], [0, 2816]],
            [[0, 1280], [0, 2816], [512, 2816], [512, 1280]],
            [[0, 2560], [0, 1536], [512, 1536], [512, 2560]],
            [[0, 2560], [512, 2560], [512, 1536], [0, 1536]],
        ],
        (5, true) => [
            [[1280, 2560], [2816, 2560], [2816, 1536], [1280, 1536]],
            [[1280, 2560], [1280, 1536], [2816, 1536], [2816, 2560]],
            [[0, 1280], [256, 1280], [256, 2816], [0, 2816]],
            [[0, 1280], [0, 2816], [256, 2816], [256, 1280]],
            [[0, 2560], [0, 1536], [256, 1536], [256, 2560]],
            [[0, 2560], [256, 2560], [256, 1536], [0, 1536]],
        ],
        _ => panic!("wall button golden requires orientation 2..=5"),
    }
}

#[test]
fn compiler_button_wall_uvlock_matches_independent_target_space_goldens() {
    let directory = tempfile::tempdir().expect("create button UV-lock fixture");
    write_button_pack(directory.path());
    let generated = generated_button_records();
    let selectors = (2..=5)
        .flat_map(|orientation| [false, true].map(move |pressed| (orientation, pressed)))
        .collect::<Vec<_>>();
    let mut records = selectors
        .iter()
        .map(|selector| {
            generated
                .iter()
                .find(|record| {
                    record.name.as_ref() == "minecraft:stone_button"
                        && button_selector(record) == *selector
                })
                .expect("requested wall button selector")
                .clone()
        })
        .collect::<Vec<_>>();
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 97_500 + id as u32;
    }
    let compiled =
        compile_pack(directory.path(), &records).expect("compile button UV-lock fixture");
    for (id, &(orientation, pressed)) in selectors.iter().enumerate() {
        let quads = compiled_model_quads(&compiled, id);
        assert_eq!(quads.len(), 6);
        let golden = wall_button_uv_golden(orientation, pressed);
        for face in BlockFace::ALL {
            let quad = quads
                .iter()
                .find(|quad| quad.flags & MODEL_QUAD_FLAG_FACE_MASK == face as u32)
                .expect("one quad for each target wall face");
            assert_eq!(
                quad.uvs, golden[face as usize],
                "orientation {orientation} pressed {pressed} target face {face:?}"
            );
        }
    }
}

#[test]
fn generated_button_registry_has_exact_names_and_selector_matrix() {
    let records = generated_button_records();
    assert_eq!(records.len(), 168);
    let expected_names = [
        "minecraft:acacia_button",
        "minecraft:bamboo_button",
        "minecraft:birch_button",
        "minecraft:cherry_button",
        "minecraft:crimson_button",
        "minecraft:dark_oak_button",
        "minecraft:jungle_button",
        "minecraft:mangrove_button",
        "minecraft:pale_oak_button",
        "minecraft:polished_blackstone_button",
        "minecraft:spruce_button",
        "minecraft:stone_button",
        "minecraft:warped_button",
        "minecraft:wooden_button",
    ];
    for name in expected_names {
        let selected = records
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 12, "{name}");
        let selectors = selected
            .into_iter()
            .map(button_selector)
            .collect::<HashSet<_>>();
        assert_eq!(
            selectors,
            (0..6)
                .flat_map(|facing| [false, true].map(move |pressed| (facing, pressed)))
                .collect()
        );
    }
}

#[test]
fn compiler_covers_all_button_states_with_exact_geometry_uvs_materials_and_determinism() {
    let directory = tempfile::tempdir().expect("create button fixture");
    write_button_pack(directory.path());
    let records = generated_button_records();
    let compiled = compile_pack(directory.path(), &records).expect("compile all buttons");
    assert_eq!(
        compiled.materials.len(),
        15,
        "diagnostic plus fourteen button materials"
    );
    for (id, record) in records.iter().enumerate() {
        let (orientation, pressed) = button_selector(record);
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.canonical_state);
        assert_eq!(record.face_coverage, 0);
        assert!(!visual.flags.intersects(
            BlockFlags::AIR
                | BlockFlags::CUBE_GEOMETRY
                | BlockFlags::OCCLUDES_FULL_FACE
                | BlockFlags::LEAF_MODEL
        ));
        let quads = compiled_model_quads(&compiled, id);
        assert_eq!(quads.len(), 6);
        assert_eq!(
            model_bounds(quads),
            button_expected_bounds(orientation, pressed)
        );
        for (quad, (face, positions, uvs)) in quads
            .iter()
            .zip(expected_button_quads(orientation, pressed))
        {
            assert_eq!(quad.flags, face, "{}", record.canonical_state);
            assert_eq!(quad.positions, positions, "{}", record.canonical_state);
            assert_eq!(quad.uvs, uvs, "{}", record.canonical_state);
            assert_eq!(compiled.materials[quad.material as usize].flags, 0);
            assert_ne!(quad.material, DIAGNOSTIC_MATERIAL);
        }
    }
    let baseline = encode_blob(&compiled).expect("encode buttons");
    let mut reversed = records.clone();
    reversed.reverse();
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &reversed).unwrap()).unwrap(),
        baseline,
        "button compilation depends on registry ordering"
    );
    let mut without_collision = records;
    for record in &mut without_collision {
        record.collision_seed = CollisionSeed::default();
    }
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &without_collision).unwrap()).unwrap(),
        baseline,
        "collision-only seeds changed button render geometry"
    );
}

#[test]
fn compiler_button_selectors_fail_closed_when_missing_invalid_extra_or_mismatched() {
    let directory = tempfile::tempdir().expect("create invalid button fixture");
    write_button_pack(directory.path());
    let valid = generated_button_records()
        .into_iter()
        .find(|record| button_selector(record) == (2, false))
        .expect("north unpressed button");
    let typed = |fields: &[(ModelStateField, u32)]| {
        encoded_model_record(0, 1, "minecraft:stone_button", ModelFamily::Button, fields)
            .model_state
    };
    let mut records = Vec::new();
    for fields in [
        vec![(ModelStateField::Orientation, 2)],
        vec![(ModelStateField::Flags, 0)],
        vec![
            (ModelStateField::Orientation, 2),
            (ModelStateField::Flags, 1),
        ],
        vec![
            (ModelStateField::Orientation, 2),
            (ModelStateField::Flags, 3),
        ],
        vec![
            (ModelStateField::Orientation, 6),
            (ModelStateField::Flags, 0),
        ],
        vec![
            (ModelStateField::Orientation, 2),
            (ModelStateField::Flags, 0),
            (ModelStateField::Half, 0),
        ],
    ] {
        let mut invalid = valid.clone();
        invalid.model_state = typed(&fields);
        records.push(invalid);
    }
    for state in [
        r#"{"facing_direction":{"type":"int","value":2}}"#,
        r#"{"button_pressed_bit":{"type":"byte","value":0},"extra":{"type":"byte","value":0},"facing_direction":{"type":"int","value":2}}"#,
        r#"{"button_pressed_bit":{"type":"int","value":0},"facing_direction":{"type":"int","value":2}}"#,
        r#"{"button_pressed_bit":{"type":"byte","value":2},"facing_direction":{"type":"int","value":2}}"#,
        r#"{"button_pressed_bit":{"type":"byte","value":0},"facing_direction":{"type":"int","value":6}}"#,
        r#"{"button_pressed_bit":{"type":"byte","value":1},"facing_direction":{"type":"int","value":2}}"#,
        r#"{"button_pressed_bit":{"type":"byte","value":0},"facing_direction":{"type":"int","value":3}}"#,
    ] {
        let mut invalid = valid.clone();
        invalid.canonical_state = state.into();
        records.push(invalid);
    }
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 98_000 + id as u32;
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile invalid buttons");
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic)
    );
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_button_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let mut records = generated_button_records();
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned buttons");
    assert_eq!(records.len(), 168);
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Model)
    );
    assert!(
        compiled
            .model_quads
            .iter()
            .all(|quad| quad.material != DIAGNOSTIC_MATERIAL)
    );
    let baseline = encode_blob(&compiled).expect("encode pinned buttons");
    records.reverse();
    let reversed = compile_pack(Path::new(&pack), &records).expect("compile reversed buttons");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}
