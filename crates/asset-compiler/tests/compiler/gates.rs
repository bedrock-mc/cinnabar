use super::support::*;

fn generated_gate_records(name: &str) -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == name && record.model_family == ModelFamily::Gate)
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        (
            record.model_state.get(ModelStateField::Flags).unwrap(),
            record.model_state.get(ModelStateField::Open).unwrap(),
            record
                .model_state
                .get(ModelStateField::Orientation)
                .unwrap(),
        )
    });
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 93_000 + id as u32;
    }
    records
}

fn write_gate_pack(root: &Path) {
    write_pack(
        root,
        r#"{"fence_gate":{"textures":{
            "west":"gate_west","east":"gate_east","down":"gate_down",
            "up":"gate_up","north":"gate_north","south":"gate_south"
        }}}"#,
        r#"{"texture_data":{
            "gate_west":{"textures":"textures/blocks/gate_west"},
            "gate_east":{"textures":"textures/blocks/gate_east"},
            "gate_down":{"textures":"textures/blocks/gate_down"},
            "gate_up":{"textures":"textures/blocks/gate_up"},
            "gate_north":{"textures":"textures/blocks/gate_north"},
            "gate_south":{"textures":"textures/blocks/gate_south"}
        }}"#,
        "[]",
    );
    for (index, path) in ["west", "east", "down", "up", "north", "south"]
        .into_iter()
        .enumerate()
    {
        write_png(
            root,
            &format!("textures/blocks/gate_{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 * 29 + 1, 73, 113, 255]),
        );
    }
}

fn write_bamboo_gate_pack(root: &Path) {
    write_pack(
        root,
        r#"{"bamboo_fence_gate":{"textures":"bamboo_fence_gate"}}"#,
        r#"{"texture_data":{"bamboo_fence_gate":{"textures":"textures/blocks/bamboo_fence_gate"}}}"#,
        "[]",
    );
    let mut pixels = solid(TILE_SIZE, TILE_SIZE, [211, 173, 77, 255]);
    pixels[0][3] = 0;
    write_png(
        root,
        "textures/blocks/bamboo_fence_gate",
        TILE_SIZE,
        TILE_SIZE,
        &pixels,
    );
}

fn rotate_gate_face(face: usize, orientation: u32) -> usize {
    const ROTATED: [[usize; 6]; 4] = [
        [0, 1, 2, 3, 4, 5],
        [4, 5, 2, 3, 1, 0],
        [1, 0, 2, 3, 5, 4],
        [5, 4, 2, 3, 0, 1],
    ];
    ROTATED[orientation as usize][face]
}

fn rotate_gate_position([x, y, z]: [i16; 3], orientation: u32) -> [i16; 3] {
    match orientation {
        0 => [x, y, z],
        1 => [256 - z, y, x],
        2 => [256 - x, y, 256 - z],
        3 => [z, y, 256 - x],
        _ => unreachable!(),
    }
}

fn gate_face_from_flags(flags: u32) -> usize {
    match flags & MODEL_QUAD_FLAG_FACE_MASK {
        3 => BlockFace::West as usize,
        4 => BlockFace::East as usize,
        1 => BlockFace::Down as usize,
        2 => BlockFace::Up as usize,
        5 => BlockFace::North as usize,
        6 => BlockFace::South as usize,
        face => panic!("invalid gate face {face}"),
    }
}

fn expected_gate_uvs(face: usize, u_min: u16, v_min: u16, u_max: u16, v_max: u16) -> [[u16; 2]; 4] {
    match face {
        0 | 5 => [
            [u_min, v_max],
            [u_max, v_max],
            [u_max, v_min],
            [u_min, v_min],
        ],
        1 | 4 => [
            [u_min, v_max],
            [u_min, v_min],
            [u_max, v_min],
            [u_max, v_max],
        ],
        2 => [
            [u_min, v_min],
            [u_max, v_min],
            [u_max, v_max],
            [u_min, v_max],
        ],
        3 => [
            [u_min, v_min],
            [u_min, v_max],
            [u_max, v_max],
            [u_max, v_min],
        ],
        _ => unreachable!(),
    }
}

fn expected_gate_boxes(open: u32, in_wall: bool, orientation: u32) -> Vec<([i16; 3], [i16; 3])> {
    let source = if open == 0 {
        vec![
            ([0, 80, 112], [32, 256, 144]),
            ([224, 80, 112], [256, 256, 144]),
            ([96, 96, 112], [128, 240, 144]),
            ([128, 96, 112], [160, 240, 144]),
            ([32, 96, 112], [96, 144, 144]),
            ([32, 192, 112], [96, 240, 144]),
            ([160, 96, 112], [224, 144, 144]),
            ([160, 192, 112], [224, 240, 144]),
        ]
    } else {
        vec![
            ([0, 80, 112], [32, 256, 144]),
            ([224, 80, 112], [256, 256, 144]),
            ([0, 96, 208], [32, 240, 240]),
            ([224, 96, 208], [256, 240, 240]),
            ([0, 96, 144], [32, 144, 208]),
            ([0, 192, 144], [32, 240, 208]),
            ([224, 96, 144], [256, 144, 208]),
            ([224, 192, 144], [256, 240, 208]),
        ]
    };
    source
        .into_iter()
        .map(|(mut min, mut max)| {
            if in_wall {
                min[1] -= 48;
                max[1] -= 48;
            }
            let corners = [
                [min[0], min[1], min[2]],
                [min[0], min[1], max[2]],
                [min[0], max[1], min[2]],
                [min[0], max[1], max[2]],
                [max[0], min[1], min[2]],
                [max[0], min[1], max[2]],
                [max[0], max[1], min[2]],
                [max[0], max[1], max[2]],
            ]
            .map(|position| rotate_gate_position(position, orientation));
            let rotated_min = [0, 1, 2].map(|axis| corners.iter().map(|p| p[axis]).min().unwrap());
            let rotated_max = [0, 1, 2].map(|axis| corners.iter().map(|p| p[axis]).max().unwrap());
            (rotated_min, rotated_max)
        })
        .collect()
}

#[test]
fn compiler_routes_all_gate_selectors_to_exact_uv_locked_vanilla_templates() {
    const IN_WALL: u32 = 1 << 6;
    let directory = tempfile::tempdir().expect("create gate fixture");
    write_gate_pack(directory.path());
    let records = generated_gate_records("minecraft:fence_gate");
    assert_eq!(records.len(), 16);
    let selectors = records
        .iter()
        .map(|record| {
            (
                record
                    .model_state
                    .get(ModelStateField::Orientation)
                    .unwrap(),
                record.model_state.get(ModelStateField::Open).unwrap(),
                record.model_state.get(ModelStateField::Flags).unwrap(),
            )
        })
        .collect::<HashSet<_>>();
    assert_eq!(selectors.len(), 16);

    let compiled = compile_pack(directory.path(), &records).expect("compile all gate states");
    assert_eq!(
        compiled.materials.len(),
        7,
        "diagnostic plus six opaque faces"
    );
    assert_eq!(compiled.model_templates.len(), 32);
    for (id, record) in records.iter().enumerate() {
        let orientation = record
            .model_state
            .get(ModelStateField::Orientation)
            .unwrap();
        let open = record.model_state.get(ModelStateField::Open).unwrap();
        let flags = record.model_state.get(ModelStateField::Flags).unwrap();
        let in_wall = flags == IN_WALL;
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.canonical_state);
        assert!(!visual.flags.intersects(
            BlockFlags::AIR
                | BlockFlags::CUBE_GEOMETRY
                | BlockFlags::OCCLUDES_FULL_FACE
                | BlockFlags::LEAF_MODEL
        ));
        assert_eq!(record.face_coverage, 0);
        let quads = compiled_compound_model_quads(&compiled, id);
        assert_eq!(quads.len(), 40);
        let expected_boxes = expected_gate_boxes(open, in_wall, orientation);
        let mut start = 0;
        for (element, expected) in expected_boxes.into_iter().enumerate() {
            let count = if element < 4 { 6 } else { 4 };
            assert_eq!(model_bounds(&quads[start..start + count]), expected);
            start += count;
        }
        assert_eq!(start, quads.len());
        let base = records
            .iter()
            .position(|candidate| {
                candidate.model_state.get(ModelStateField::Orientation) == Some(0)
                    && candidate.model_state.get(ModelStateField::Open) == Some(open)
                    && candidate.model_state.get(ModelStateField::Flags) == Some(flags)
            })
            .unwrap();
        let base_quads = compiled_compound_model_quads(&compiled, base);
        for (quad, base_quad) in quads.iter().zip(base_quads) {
            let source_face = gate_face_from_flags(base_quad.flags);
            let target_face = rotate_gate_face(source_face, orientation);
            assert_eq!(gate_face_from_flags(quad.flags), target_face);
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
            assert_eq!(quad.material, visual.faces[target_face]);
            assert_eq!(compiled.materials[quad.material as usize].flags, 0);
            let mut expected_positions = base_quad
                .positions
                .map(|position| rotate_gate_position(position, orientation));
            let mut actual_positions = quad.positions;
            expected_positions.sort_unstable();
            actual_positions.sort_unstable();
            assert_eq!(actual_positions, expected_positions);
            let u_min = base_quad.uvs.iter().map(|uv| uv[0]).min().unwrap();
            let u_max = base_quad.uvs.iter().map(|uv| uv[0]).max().unwrap();
            let v_min = base_quad.uvs.iter().map(|uv| uv[1]).min().unwrap();
            let v_max = base_quad.uvs.iter().map(|uv| uv[1]).max().unwrap();
            assert_eq!(
                quad.uvs,
                expected_gate_uvs(target_face, u_min, v_min, u_max, v_max),
                "orientation={orientation} open={open} in_wall={in_wall}"
            );
        }
    }

    let baseline = encode_blob(&compiled).expect("encode exhaustive gates");
    let mut reversed = records.clone();
    reversed.reverse();
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &reversed).unwrap()).unwrap(),
        baseline,
        "gate compilation depends on registry ordering"
    );
    let mut without_collision = records;
    for record in &mut without_collision {
        record.collision_seed = CollisionSeed::default();
    }
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &without_collision).unwrap()).unwrap(),
        baseline,
        "collision-only seeds changed typed gate render geometry"
    );
}

#[test]
fn compiler_bamboo_gate_uses_custom_missing_faces_and_reversed_rotated_uvs() {
    let directory = tempfile::tempdir().expect("create bamboo gate fixture");
    write_bamboo_gate_pack(directory.path());
    let mut records = generated_gate_records("minecraft:bamboo_fence_gate")
        .into_iter()
        .filter(|record| {
            record.model_state.get(ModelStateField::Orientation) == Some(0)
                && record.model_state.get(ModelStateField::Flags) == Some(0)
        })
        .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| record.model_state.get(ModelStateField::Open));
    assert_eq!(records.len(), 2);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 94_500 + id as u32;
    }

    let compiled = compile_pack(directory.path(), &records).expect("compile custom bamboo gates");
    let closed = compiled_compound_model_quads(&compiled, 0);
    let open = compiled_compound_model_quads(&compiled, 1);
    assert_eq!(
        closed.len(),
        38,
        "closed bamboo omits two hidden inner faces"
    );
    assert_eq!(open.len(), 40);
    assert_eq!(compiled.model_templates.len(), 4);
    assert_eq!(compiled.model_templates[0].quad_count, 22);
    assert_eq!(compiled.model_templates[1].quad_count, 16);
    assert_eq!(compiled.model_templates[2].quad_count, 24);
    assert_eq!(compiled.model_templates[3].quad_count, 16);
    assert_eq!(
        compiled.model_templates[0].flags,
        assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT | assets::MODEL_TEMPLATE_FLAG_GATE_AXIS_Z
    );
    assert_eq!(compiled.model_templates[1].flags, 0);

    assert_eq!(
        gate_face_from_flags(closed[12].flags),
        BlockFace::West as usize,
        "closed inner-left keeps west but omits east"
    );
    assert_eq!(
        gate_face_from_flags(closed[17].flags),
        BlockFace::East as usize,
        "closed inner-right keeps east but omits west"
    );
    assert_eq!(
        closed[2].uvs,
        [[4096, 3328], [3584, 3328], [3584, 3840], [4096, 3840]],
        "left post down face preserves the reversed 16..14 U range"
    );
    assert_eq!(
        open[27].uvs,
        [[512, 768], [1536, 768], [1536, 256], [512, 256]],
        "open left bar up face applies the vanilla 270-degree UV rotation"
    );
    assert_eq!(
        open[36].uvs,
        [[3584, 1536], [2560, 1536], [2560, 768], [3584, 768]],
        "upper-right west face preserves the intentional reversed 14..10 U range"
    );
}

#[test]
fn compiler_gate_selectors_fail_closed_when_missing_or_out_of_range() {
    let directory = tempfile::tempdir().expect("create invalid gate fixture");
    write_gate_pack(directory.path());
    let mut records = vec![model_record(
        0,
        94_000,
        "minecraft:fence_gate",
        "{}",
        ModelFamily::Gate,
    )];
    for (field, value) in [
        (ModelStateField::Orientation, 4),
        (ModelStateField::Open, 2),
        (ModelStateField::Flags, 1),
        (ModelStateField::Flags, 65),
        (ModelStateField::Flags, 128),
    ] {
        let id = records.len() as u32;
        let mut fields = vec![
            (ModelStateField::Orientation, 0),
            (ModelStateField::Open, 0),
            (ModelStateField::Flags, 0),
        ];
        fields.iter_mut().find(|entry| entry.0 == field).unwrap().1 = value;
        records.push(encoded_model_record(
            id,
            94_000 + id,
            "minecraft:fence_gate",
            ModelFamily::Gate,
            &fields,
        ));
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile invalid gate states");
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
fn compiler_gate_requires_the_exact_typed_selector_mask() {
    let directory = tempfile::tempdir().expect("create exact-mask gate fixture");
    write_gate_pack(directory.path());
    let valid = encoded_model_record(
        0,
        94_400,
        "minecraft:fence_gate",
        ModelFamily::Gate,
        &[
            (ModelStateField::Orientation, 0),
            (ModelStateField::Open, 0),
            (ModelStateField::Flags, 0),
        ],
    );
    assert_eq!(valid.model_state.mask(), 0x85);
    let unexpected = encoded_model_record(
        1,
        94_401,
        "minecraft:fence_gate",
        ModelFamily::Gate,
        &[
            (ModelStateField::Orientation, 0),
            (ModelStateField::Half, 0),
            (ModelStateField::Open, 0),
            (ModelStateField::Flags, 0),
        ],
    );
    assert_eq!(unexpected.model_state.mask(), 0x87);

    let compiled = compile_pack(directory.path(), &[valid, unexpected])
        .expect("compile exact and over-specified gate selectors");
    assert_eq!(compiled.visuals[0].kind, VisualKind::Model);
    assert_eq!(compiled.visuals[1].kind, VisualKind::Diagnostic);
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_gate_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.model_family == ModelFamily::Gate)
    .collect::<Vec<_>>();
    assert_eq!(records.len(), 192);
    assert_eq!(
        records
            .iter()
            .map(|record| record.name.as_ref())
            .collect::<HashSet<_>>()
            .len(),
        12
    );
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 95_000 + id as u32;
    }
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned gates");
    for (id, record) in records.iter().enumerate() {
        assert_eq!(
            compiled.visuals[id].kind,
            VisualKind::Model,
            "{}",
            record.name
        );
        assert_eq!(record.face_coverage, 0);
        let quads = compiled_compound_model_quads(&compiled, id);
        let expected_count = if record.name.as_ref() == "minecraft:bamboo_fence_gate"
            && record.model_state.get(ModelStateField::Open) == Some(0)
        {
            38
        } else {
            40
        };
        assert_eq!(quads.len(), expected_count);
        assert!(quads.iter().all(|quad| {
            quad.material != DIAGNOSTIC_MATERIAL
                && quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK == 0
                && compiled.materials[quad.material as usize].flags == 0
        }));
    }
    let baseline = encode_blob(&compiled).expect("encode pinned gates");
    records.reverse();
    let reversed = compile_pack(Path::new(&pack), &records).expect("compile reversed gates");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}
