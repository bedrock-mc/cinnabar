use super::support::*;

fn write_stair_pack(root: &Path) {
    write_pack(
        root,
        r#"{"oak_stairs":{"textures":{"down":"stair_down","side":"stair_side","up":"stair_up"}}}"#,
        r#"{"texture_data":{"stair_down":{"textures":"textures/blocks/stair_down"},"stair_side":{"textures":"textures/blocks/stair_side"},"stair_up":{"textures":"textures/blocks/stair_up"}}}"#,
        "[]",
    );
    for (path, colour) in [
        ("stair_down", [17, 37, 57, 255]),
        ("stair_side", [77, 97, 117, 255]),
        ("stair_up", [137, 157, 177, 255]),
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

fn write_asymmetric_stair_pack(root: &Path) {
    write_pack(
        root,
        r#"{"oak_stairs":{"textures":{"west":"stair_west","east":"stair_east","down":"stair_down","up":"stair_up","north":"stair_north","south":"stair_south"}}}"#,
        r#"{"texture_data":{"stair_west":{"textures":"textures/blocks/stair_west"},"stair_east":{"textures":"textures/blocks/stair_east"},"stair_down":{"textures":"textures/blocks/stair_down"},"stair_up":{"textures":"textures/blocks/stair_up"},"stair_north":{"textures":"textures/blocks/stair_north"},"stair_south":{"textures":"textures/blocks/stair_south"}}}"#,
        "[]",
    );
    for (index, path) in ["west", "east", "down", "up", "north", "south"]
        .into_iter()
        .enumerate()
    {
        let mut uv_marker = Vec::with_capacity((TILE_SIZE * TILE_SIZE) as usize);
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                uv_marker.push([17 + index as u8 * 31, x as u8 * 16, y as u8 * 16, 255]);
            }
        }
        write_png(
            root,
            &format!("textures/blocks/stair_{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &uv_marker,
        );
    }
}

fn rotate_stair_position([x, y, z]: [i16; 3], rotation: u32) -> [i16; 3] {
    match rotation & 3 {
        1 => [256 - z, y, x],
        2 => [256 - x, y, 256 - z],
        3 => [z, y, 256 - x],
        _ => [x, y, z],
    }
}

fn rotate_stair_face(face: usize, rotation: u32) -> usize {
    let horizontal = match rotation & 3 {
        1 => [4, 5, 2, 3, 1, 0],
        2 => [1, 0, 2, 3, 5, 4],
        3 => [5, 4, 2, 3, 0, 1],
        _ => [0, 1, 2, 3, 4, 5],
    };
    horizontal[face]
}

#[test]
fn compiler_stair_rotation_preserves_asymmetric_materials_geometry_and_uv_lock_for_all_states() {
    let directory = tempfile::tempdir().expect("create asymmetric stair fixture");
    write_asymmetric_stair_pack(directory.path());
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == "minecraft:oak_stairs")
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| {
        (
            record.model_state.get(ModelStateField::Half).unwrap(),
            record
                .model_state
                .get(ModelStateField::Orientation)
                .unwrap(),
        )
    });
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 31_000 + id as u32;
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile asymmetric stairs");

    for (id, record) in records.iter().enumerate() {
        let visual = compiled.visuals[id];
        let rotation = visual.variant & 3;
        let half = record.model_state.get(ModelStateField::Half).unwrap();
        let orientation = record
            .model_state
            .get(ModelStateField::Orientation)
            .unwrap();
        let template = compiled.model_templates[visual.model_template as usize];
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        let north_id = records
            .iter()
            .position(|candidate| {
                candidate.model_state.get(ModelStateField::Half) == Some(half)
                    && candidate.model_state.get(ModelStateField::Orientation) == Some(2)
            })
            .unwrap();
        let north_visual = compiled.visuals[north_id];
        let north_template = compiled.model_templates[north_visual.model_template as usize];
        let north_quads = &compiled.model_quads[north_template.quad_start as usize
            ..(north_template.quad_start + north_template.quad_count) as usize];
        assert_eq!(quads.len(), north_quads.len());
        let mut high_top_centres = Vec::new();
        for (quad, north_quad) in quads.iter().zip(north_quads) {
            assert_eq!(
                quad.positions, north_quad.positions,
                "orientation must stay canonical"
            );
            assert_eq!(
                quad.uvs, north_quad.uvs,
                "orientation must preserve UV lock"
            );
            assert_eq!(
                quad.flags, north_quad.flags,
                "orientation changed canonical faces"
            );
            let canonical_face =
                [2, 3, 0, 1, 4, 5][((quad.flags & MODEL_QUAD_FLAG_FACE_MASK) - 1) as usize];
            let world_face = rotate_stair_face(canonical_face, rotation);
            assert_eq!(
                quad.material, visual.faces[world_face],
                "half={half} orientation={orientation} canonical_face={canonical_face}"
            );
            let world_positions = quad
                .positions
                .map(|position| rotate_stair_position(position, rotation));
            assert!(
                world_positions
                    .iter()
                    .flatten()
                    .all(|&coordinate| (0..=256).contains(&coordinate))
            );
            assert!(
                quad.uvs
                    .iter()
                    .flatten()
                    .all(|&coordinate| coordinate <= 4096)
            );
            let step_outer_face = if half == 0 {
                BlockFace::Up as usize
            } else {
                BlockFace::Down as usize
            };
            let step_outer_y = if half == 0 { 256 } else { 0 };
            if world_face == step_outer_face
                && world_positions
                    .iter()
                    .all(|position| position[1] == step_outer_y)
            {
                high_top_centres.push([
                    world_positions
                        .iter()
                        .map(|position| i32::from(position[0]))
                        .sum::<i32>()
                        / 4,
                    world_positions
                        .iter()
                        .map(|position| i32::from(position[2]))
                        .sum::<i32>()
                        / 4,
                ]);
            }
        }
        assert!(
            !high_top_centres.is_empty(),
            "half={half} orientation={orientation}"
        );
        assert!(
            high_top_centres.iter().any(|&[x, z]| match orientation {
                0 => z > 128,
                1 => x < 128,
                2 => z < 128,
                3 => x > 128,
                _ => false,
            }),
            "high step lost world-space handedness: half={half} orientation={orientation} centres={high_top_centres:?}"
        );
    }
}

#[test]
fn compiler_stairs_emit_five_contiguous_bounded_exterior_templates_for_every_state() {
    let directory = tempfile::tempdir().expect("create stair fixture");
    write_stair_pack(directory.path());
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == "minecraft:oak_stairs")
    .collect::<Vec<_>>();
    assert_eq!(records.len(), 8);
    records.sort_unstable_by_key(|record| {
        (
            record.model_state.get(ModelStateField::Half).unwrap(),
            record
                .model_state
                .get(ModelStateField::Orientation)
                .unwrap(),
        )
    });
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 30_000 + id as u32;
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile all oak stair states");
    assert_eq!(
        compiled.model_templates.len(),
        10,
        "five shapes per upside state; orientation is compact"
    );
    let mut bases_by_half = [HashSet::new(), HashSet::new()];
    for (id, record) in records.iter().enumerate() {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.canonical_state);
        assert!(
            !visual
                .flags
                .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        );
        assert_eq!(
            visual.variant & 3,
            (record
                .model_state
                .get(ModelStateField::Orientation)
                .unwrap()
                + 2)
                & 3
        );
        assert_eq!(
            (visual.variant >> 2) & 1,
            record.model_state.get(ModelStateField::Half).unwrap()
        );
        let base = visual.model_template as usize;
        assert_eq!(base % 5, 0);
        bases_by_half[record.model_state.get(ModelStateField::Half).unwrap() as usize].insert(base);
        for template in &compiled.model_templates[base..base + 5] {
            assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_STAIR);
            assert!((1..=32).contains(&template.quad_count));
            let quads = &compiled.model_quads[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            assert!(quads.iter().all(|quad| {
                let [a, b, c, _] = quad.positions;
                a != b
                    && b != c
                    && a != c
                    && quad.uvs.iter().all(|uv| uv[0] <= 4096 && uv[1] <= 4096)
                    && quad.flags & !(MODEL_QUAD_FLAG_FACE_MASK | MODEL_QUAD_FLAG_CULL_FACE_MASK)
                        == 0
            }));
        }
    }
    assert_eq!(bases_by_half[0].len(), 1);
    assert_eq!(bases_by_half[1].len(), 1);
    let north_lower = compiled.visuals[records
        .iter()
        .position(|record| {
            record.model_state.get(ModelStateField::Orientation) == Some(2)
                && record.model_state.get(ModelStateField::Half) == Some(0)
        })
        .unwrap()]
    .model_template as usize;
    let straight = compiled.model_templates[north_lower];
    let expected_shape_digests = [
        "2e07913dd24532f98c2e2a2352f4434cee0485f3b80a1b36346543b8f41fb381",
        "65128da5f92158b78301af0bb455f5d5a9a74fc0434e50553787d31c64ac88da",
        "17ed41557ef2ecfd36c077b347aafea71e8deca603f4155fe1001b2992b0deb2",
        "a8362bb0405925933f2a24acf62338455c1f07390d136446f2e6cf34dd2166b5",
        "3836909fd60a7bedb8e51c4b8358a42fdb5e23ad580a53536b3c81492423720b",
        "d18605f16826c3570a3691c95793e25d4be00702d6ae7221ffaa75d75e1efee6",
        "28c0a4d6a13b85633117437f6bb6b8263e7cd03b05ab890b69e294b84d11f990",
        "02a8b452d0d4f1de93c604bca2175dbfac432380ce2c54036b79e6073403eb7f",
        "f06c28159d93b5528ae9aff35580d1bae8676a2e0c72c7264c1b1b3fe046691f",
        "f1f5f7b2c7527ca6105c875dc4e04f0d4239e46e220325147dc27e965c267760",
    ];
    let mut actual_shape_digests = Vec::new();
    for base in [
        *bases_by_half[0].iter().next().unwrap(),
        *bases_by_half[1].iter().next().unwrap(),
    ] {
        for template in &compiled.model_templates[base..base + 5] {
            let quads = &compiled.model_quads[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            actual_shape_digests.push(slab_geometry_digest(quads));
        }
    }
    assert_eq!(actual_shape_digests, expected_shape_digests);
    let straight_quads = &compiled.model_quads
        [straight.quad_start as usize..(straight.quad_start + straight.quad_count) as usize];
    assert!(
        straight_quads
            .iter()
            .any(|quad| quad.positions.iter().all(|p| p[2] == 128)
                && quad.positions.iter().any(|p| p[1] == 128)
                && quad.positions.iter().any(|p| p[1] == 256)),
        "north stair riser"
    );
    assert!(
        straight_quads
            .iter()
            .all(|quad| quad.positions.windows(2).all(|pair| pair[0] != pair[1])),
        "no flat edges"
    );

    let first = encode_blob(&compiled).expect("encode stairs");
    records.reverse();
    let second =
        encode_blob(&compile_pack(directory.path(), &records).expect("compile reversed stairs"))
            .expect("encode reversed stairs");
    assert_eq!(
        first, second,
        "stair blob is deterministic across input order"
    );
    RuntimeAssets::decode(&first).expect("runtime accepts canonical stair groups");
}

#[test]
fn compiler_covers_every_breg_stair_state_with_compact_stable_groups() {
    let directory = tempfile::tempdir().expect("create exhaustive stair fixture");
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed registry")
    .into_iter()
    .filter(|record| record.model_family == ModelFamily::Stair)
    .collect::<Vec<_>>();
    assert_eq!(records.len(), 512);
    assert!(records.iter().all(|record| {
        record
            .model_state
            .get(ModelStateField::Orientation)
            .is_some_and(|value| value < 4)
            && record
                .model_state
                .get(ModelStateField::Half)
                .is_some_and(|value| value < 2)
    }));
    let mut names = records
        .iter()
        .map(|record| record.name.strip_prefix("minecraft:").unwrap().to_owned())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(names.len(), 64);
    for name in &names {
        let selectors = records
            .iter()
            .filter(|record| record.name.strip_prefix("minecraft:") == Some(name.as_str()))
            .map(|record| {
                (
                    record.model_state.get(ModelStateField::Orientation),
                    record.model_state.get(ModelStateField::Half),
                )
            })
            .collect::<HashSet<_>>();
        let expected = (0..4)
            .flat_map(|orientation| (0..2).map(move |half| (Some(orientation), Some(half))))
            .collect::<HashSet<_>>();
        assert_eq!(selectors, expected, "{name} exact stair selector matrix");
    }
    let blocks = names
        .iter()
        .map(|name| format!(r#""{name}":{{"textures":"stair_all"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    write_pack(
        directory.path(),
        &format!("{{{blocks}}}"),
        r#"{"texture_data":{"stair_all":{"textures":"textures/blocks/stair_all"}}}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/stair_all",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [91, 111, 131, 255]),
    );
    let compiled = compile_pack(directory.path(), &records).expect("compile every BREG stair");
    assert!(records.iter().all(|record| {
        let visual = compiled.visuals[record.sequential_id as usize];
        visual.kind == VisualKind::Model
            && visual.model_template != assets::NO_MODEL_TEMPLATE
            && compiled.model_templates[visual.model_template as usize].flags
                == MODEL_TEMPLATE_FLAG_STAIR
    }));
    assert_eq!(
        compiled.model_templates.len(),
        10,
        "one symmetric-material group per half"
    );
    let first = encode_blob(&compiled).expect("encode exhaustive stairs");
    let mut reversed = records.clone();
    reversed.reverse();
    let second = encode_blob(
        &compile_pack(directory.path(), &reversed).expect("compile reversed exhaustive stairs"),
    )
    .expect("encode reversed exhaustive stairs");
    assert_eq!(first, second);
}

#[test]
fn compiler_real_pinned_pack_has_zero_diagnostic_stair_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    assert_eq!(records.len(), 16_913);
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile requested pinned pack");
    let stairs = records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Stair)
        .collect::<Vec<_>>();
    assert_eq!(stairs.len(), 512);
    assert_eq!(
        stairs
            .iter()
            .map(|record| record.name.as_ref())
            .collect::<HashSet<_>>()
            .len(),
        64
    );
    for record in stairs {
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(
            visual.kind,
            VisualKind::Model,
            "{} {}",
            record.name,
            record.canonical_state
        );
        let template = compiled
            .model_templates
            .get(visual.model_template as usize)
            .unwrap_or_else(|| {
                panic!(
                    "missing stair template for {} {}",
                    record.name, record.canonical_state
                )
            });
        assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_STAIR);
        assert!((1..=32).contains(&template.quad_count));
    }
}
