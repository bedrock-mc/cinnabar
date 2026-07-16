use super::support::*;

fn generated_wall_records(name: &str) -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == name && record.model_family == ModelFamily::Wall)
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        record
            .model_state
            .get(ModelStateField::Connections)
            .expect("wall connections")
    });
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 70_000 + id as u32;
    }
    records
}

fn write_wall_pack(root: &Path) {
    write_pack(
        root,
        r#"{"cobblestone_wall":{"textures":{
            "west":"wall_west","east":"wall_east","down":"wall_down",
            "up":"wall_up","north":"wall_north","south":"wall_south"
        }}}"#,
        r#"{"texture_data":{
            "wall_west":{"textures":"textures/blocks/wall_west"},
            "wall_east":{"textures":"textures/blocks/wall_east"},
            "wall_down":{"textures":"textures/blocks/wall_down"},
            "wall_up":{"textures":"textures/blocks/wall_up"},
            "wall_north":{"textures":"textures/blocks/wall_north"},
            "wall_south":{"textures":"textures/blocks/wall_south"}
        }}"#,
        "[]",
    );
    for (index, path) in ["west", "east", "down", "up", "north", "south"]
        .into_iter()
        .enumerate()
    {
        write_png(
            root,
            &format!("textures/blocks/wall_{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 * 31 + 1, 50, 90, 255]),
        );
    }
}

fn expected_wall_boxes(connections: u32) -> Vec<([i16; 3], [i16; 3])> {
    assert_eq!(connections & !0x1ff, 0);
    let north = connections & 3;
    let east = (connections >> 2) & 3;
    let south = (connections >> 4) & 3;
    let west = (connections >> 6) & 3;
    let post = (connections >> 8) & 1;
    assert!(
        [north, east, south, west]
            .into_iter()
            .all(|connection| connection <= 2)
    );
    let height = |connection| match connection {
        1 => 224,
        2 => 256,
        _ => unreachable!(),
    };
    // The local vanilla template_wall_{post,side,side_tall}.json files are the
    // visible render oracle. Dragonfly collision BBoxes are intentionally not
    // used for these extents.
    let mut boxes = Vec::with_capacity(5);
    if post != 0 {
        boxes.push(([64, 0, 64], [192, 256, 192]));
    }
    if north != 0 {
        boxes.push(([80, 0, 0], [176, height(north), 128]));
    }
    if east != 0 {
        boxes.push(([128, 0, 80], [256, height(east), 176]));
    }
    if south != 0 {
        boxes.push(([80, 0, 128], [176, height(south), 256]));
    }
    if west != 0 {
        boxes.push(([0, 0, 80], [128, height(west), 176]));
    }
    boxes
}

#[test]
fn compiler_routes_all_generated_wall_connections_to_exact_compact_cuboids() {
    let directory = tempfile::tempdir().expect("create wall fixture");
    write_wall_pack(directory.path());
    let records = generated_wall_records("minecraft:cobblestone_wall");
    assert_eq!(records.len(), 162, "3^4 connection heights times post bit");
    let selectors = records
        .iter()
        .map(|record| {
            record
                .model_state
                .get(ModelStateField::Connections)
                .expect("typed wall connections")
        })
        .collect::<HashSet<_>>();
    assert_eq!(selectors.len(), 162);

    let compiled = compile_pack(directory.path(), &records).expect("compile all wall states");
    assert_eq!(compiled.materials.len(), 7, "diagnostic plus six faces");
    assert_eq!(compiled.model_templates.len(), 162);
    for (id, record) in records.iter().enumerate() {
        let connections = record
            .model_state
            .get(ModelStateField::Connections)
            .unwrap();
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.canonical_state);
        assert!(!visual.flags.intersects(
            BlockFlags::AIR
                | BlockFlags::CUBE_GEOMETRY
                | BlockFlags::OCCLUDES_FULL_FACE
                | BlockFlags::LEAF_MODEL
        ));
        assert_eq!(record.face_coverage, 0);
        let template = compiled.model_templates[visual.model_template as usize];
        let expected = expected_wall_boxes(connections);
        assert_eq!(template.quad_count as usize, expected.len() * 6);
        assert!(template.quad_count <= 30);
        let quads = compiled_model_quads(&compiled, id);
        for (cuboid, bounds) in quads.chunks_exact(6).zip(expected) {
            assert_eq!(model_bounds(cuboid), bounds, "{}", record.canonical_state);
            for (face, quad) in cuboid.iter().enumerate() {
                assert_eq!(quad.material, visual.faces[face]);
                assert_eq!(quad.flags, [3, 4, 1, 2, 5, 6][face]);
                assert_eq!(quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
                assert!(quad.uvs.iter().flatten().all(|value| *value <= 4096));
            }
        }
    }

    let baseline = encode_blob(&compiled).expect("encode exhaustive walls");
    let mut reversed = records.clone();
    reversed.reverse();
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &reversed).unwrap()).unwrap(),
        baseline,
        "wall compilation depends on registry ordering"
    );
    let mut without_collision = records;
    for record in &mut without_collision {
        record.collision_seed = CollisionSeed::default();
    }
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &without_collision).unwrap()).unwrap(),
        baseline,
        "collision-only seeds changed typed wall render geometry"
    );
}

#[test]
fn compiler_wall_connections_fail_closed_when_missing_or_out_of_range() {
    let directory = tempfile::tempdir().expect("create invalid wall fixture");
    write_wall_pack(directory.path());
    let mut records = vec![model_record(
        0,
        71_000,
        "minecraft:cobblestone_wall",
        "{}",
        ModelFamily::Wall,
    )];
    for connections in [3, 3 << 2, 3 << 4, 3 << 6, 1 << 9] {
        let id = records.len() as u32;
        records.push(encoded_model_record(
            id,
            71_000 + id,
            "minecraft:cobblestone_wall",
            ModelFamily::Wall,
            &[(ModelStateField::Connections, connections)],
        ));
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile invalid wall states");
    assert!(
        compiled
            .visuals
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic)
    );
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
}

fn write_connected_model_pack(root: &Path) {
    write_pack(
        root,
        r#"{
            "glass_pane":{"textures":{
                "west":"pane_body","east":"pane_edge","down":"pane_edge",
                "up":"pane_edge","north":"pane_body","south":"pane_body"
            }},
            "oak_fence":{"textures":"oak_fence"},
            "nether_brick_fence":{"textures":"nether_fence"}
        }"#,
        r#"{"texture_data":{
            "pane_body":{"textures":"textures/blocks/pane_body"},
            "pane_edge":{"textures":"textures/blocks/pane_edge"},
            "oak_fence":{"textures":"textures/blocks/oak_fence"},
            "nether_fence":{"textures":"textures/blocks/nether_fence"}
        }}"#,
        "[]",
    );
    for (path, colour) in [
        ("pane_body", [30, 60, 90, 0]),
        ("pane_edge", [60, 90, 120, 255]),
        ("oak_fence", [100, 70, 30, 255]),
        ("nether_fence", [70, 10, 20, 255]),
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

#[test]
fn compiler_emits_all_sixteen_exact_pane_connection_templates() {
    let directory = tempfile::tempdir().expect("create pane fixture");
    write_connected_model_pack(directory.path());
    let record = model_record(0, 72_000, "minecraft:glass_pane", "{}", ModelFamily::Pane);
    let compiled = compile_pack(directory.path(), &[record]).expect("compile pane");
    let visual = compiled.visuals[0];
    assert_eq!(visual.kind, VisualKind::Model);
    assert!(!visual.flags.intersects(
        BlockFlags::AIR
            | BlockFlags::CUBE_GEOMETRY
            | BlockFlags::OCCLUDES_FULL_FACE
            | BlockFlags::LEAF_MODEL
    ));
    let base = visual.model_template;
    assert_eq!(compiled.model_templates.len(), 16);
    for mask in 0_u32..16 {
        let template = compiled.model_templates[(base + mask) as usize];
        assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_PANE, "mask={mask:#06b}");
        assert_eq!(
            template.quad_count,
            6 + mask.count_ones() * 4,
            "post and arms omit both faces at every internal join, mask={mask:#06b}"
        );
        assert!(template.quad_count <= 26);
        let quads = template_quads(&compiled, base + mask);
        if mask == 0 {
            assert_eq!(model_bounds(quads), ([112, 0, 112], [144, 256, 144]));
        }
        for (bit, axis, coordinate) in [(1, 2, 112), (2, 0, 144), (4, 2, 144), (8, 0, 112)] {
            if mask & bit != 0 {
                let span_axis = if axis == 0 { 2 } else { 0 };
                assert!(
                    quads.iter().all(|quad| {
                        !quad.positions.iter().all(|position| {
                            position[axis] == coordinate
                                && (112..=144).contains(&position[span_axis])
                        })
                    }),
                    "internal pane join remains for mask={mask:#06b} bit={bit:#06b}"
                );
            }
        }
        assert!(
            quads
                .iter()
                .all(|quad| quad.uvs.iter().flatten().all(|uv| *uv <= 4096))
        );
    }
    assert!(compiled.materials.iter().skip(1).all(|material| {
        material.flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0
            && material.flags & MATERIAL_FLAG_ALPHA_BLEND == 0
    }));
}

#[test]
fn compiler_emits_bounded_seventeen_template_fence_groups_by_connection_class() {
    let directory = tempfile::tempdir().expect("create fence fixture");
    write_connected_model_pack(directory.path());
    let records = [
        model_record(0, 73_000, "minecraft:oak_fence", "{}", ModelFamily::Fence),
        model_record(
            1,
            73_001,
            "minecraft:nether_brick_fence",
            "{}",
            ModelFamily::Fence,
        ),
    ];
    let compiled = compile_pack(directory.path(), &records).expect("compile fences");
    assert_eq!(compiled.model_templates.len(), 34);
    for (id, expected_flag) in [
        (0, MODEL_TEMPLATE_FLAG_FENCE_WOOD),
        (1, MODEL_TEMPLATE_FLAG_FENCE_NETHER),
    ] {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model);
        let base = visual.model_template;
        let post = compiled.model_templates[base as usize];
        assert_eq!(post.flags, expected_flag);
        assert_eq!(post.quad_count, 6);
        assert_eq!(
            model_bounds(template_quads(&compiled, base)),
            ([96, 0, 96], [160, 256, 160])
        );
        for mask in 0_u32..16 {
            let arms = compiled.model_templates[(base + 1 + mask) as usize];
            assert_eq!(arms.flags, expected_flag, "id={id} mask={mask:#06b}");
            assert_eq!(arms.quad_count, mask.count_ones() * 8);
            assert!(arms.quad_count <= 32);
            for rail in template_quads(&compiled, base + 1 + mask).chunks_exact(4) {
                let (min, max) = model_bounds(rail);
                assert!(matches!((min[1], max[1]), (96, 144) | (192, 240)));
            }
        }
    }
    assert!(compiled.materials.iter().all(|material| {
        material.flags & (MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_ALPHA_BLEND) == 0
    }));
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_pane_and_fence_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode connected-family registry")
    .into_iter()
    .filter(|record| matches!(record.model_family, ModelFamily::Pane | ModelFamily::Fence))
    .collect::<Vec<_>>();
    assert_eq!(
        records
            .iter()
            .filter(|record| record.model_family == ModelFamily::Pane)
            .count(),
        43
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.model_family == ModelFamily::Fence)
            .count(),
        13
    );
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 74_000 + id as u32;
    }
    let sources = read_pack(Path::new(&pack)).expect("read pinned connected pack");
    for record in records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Pane)
    {
        for face in [BlockFace::North, BlockFace::East] {
            let key = resolve_texture_key(&sources.blocks, record, face)
                .key
                .unwrap_or_else(|| panic!("{} missing {face:?} key", record.name));
            assert!(
                sources.terrain.get(&key).is_some(),
                "{} {face:?} terrain key {key} is missing",
                record.name
            );
        }
    }
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned panes/fences");
    let diagnostics = records
        .iter()
        .enumerate()
        .filter(|(id, _)| compiled.visuals[*id].kind == VisualKind::Diagnostic)
        .map(|(_, record)| record.name.as_ref())
        .collect::<Vec<_>>();
    assert!(
        diagnostics.is_empty(),
        "diagnostic connected states: {diagnostics:?}"
    );
    for (id, record) in records.iter().enumerate() {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.name);
        let template = compiled.model_templates[visual.model_template as usize];
        match record.model_family {
            ModelFamily::Pane => {
                assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_PANE, "{}", record.name);
                let expected_alpha = if record.name.contains("stained_glass_pane") {
                    MATERIAL_FLAG_ALPHA_BLEND
                } else {
                    MATERIAL_FLAG_ALPHA_CUTOUT
                };
                for template in &compiled.model_templates
                    [visual.model_template as usize..visual.model_template as usize + 16]
                {
                    let quads = &compiled.model_quads[template.quad_start as usize
                        ..(template.quad_start + template.quad_count) as usize];
                    assert!(quads.iter().all(|quad| {
                        compiled.materials[quad.material as usize].flags & expected_alpha != 0
                    }));
                }
            }
            ModelFamily::Fence => {
                assert!(matches!(
                    template.flags,
                    MODEL_TEMPLATE_FLAG_FENCE_WOOD | MODEL_TEMPLATE_FLAG_FENCE_NETHER
                ));
                let expected_alpha =
                    u32::from(record.name.contains("bamboo")) * MATERIAL_FLAG_ALPHA_CUTOUT;
                for template in &compiled.model_templates
                    [visual.model_template as usize..visual.model_template as usize + 17]
                {
                    let quads = &compiled.model_quads[template.quad_start as usize
                        ..(template.quad_start + template.quad_count) as usize];
                    assert!(quads.iter().all(|quad| {
                        compiled.materials[quad.material as usize].flags
                            & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT)
                            == expected_alpha
                    }));
                }
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_wall_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.model_family == ModelFamily::Wall)
    .collect::<Vec<_>>();
    assert_eq!(records.len(), 5_184);
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 80_000 + id as u32;
    }
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned walls");
    for (id, record) in records.iter().enumerate() {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.name);
        assert_eq!(record.face_coverage, 0);
        let template = compiled.model_templates[visual.model_template as usize];
        assert!(template.quad_count <= 30);
        assert_eq!(template.quad_count % 6, 0);
        assert!(compiled_model_quads(&compiled, id).iter().all(|quad| {
            quad.material != DIAGNOSTIC_MATERIAL && quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK == 0
        }));
    }
    let baseline = encode_blob(&compiled).expect("encode pinned walls");
    records.reverse();
    let reversed = compile_pack(Path::new(&pack), &records).expect("compile reversed pinned walls");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}
