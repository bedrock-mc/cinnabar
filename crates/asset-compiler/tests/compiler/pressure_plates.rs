use super::support::*;

fn generated_pressure_plate_records(name: &str) -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| {
        record.name.as_ref() == name && record.model_family == ModelFamily::PressurePlate
    })
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| record.canonical_state.clone());
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 90_000 + id as u32;
    }
    records
}

fn write_pressure_plate_pack(root: &Path) {
    write_pack(
        root,
        r#"{"wooden_pressure_plate":{"textures":{
            "west":"plate_west","east":"plate_east","down":"plate_down",
            "up":"plate_up","north":"plate_north","south":"plate_south"
        }}}"#,
        r#"{"texture_data":{
            "plate_west":{"textures":"textures/blocks/plate_west"},
            "plate_east":{"textures":"textures/blocks/plate_east"},
            "plate_down":{"textures":"textures/blocks/plate_down"},
            "plate_up":{"textures":"textures/blocks/plate_up"},
            "plate_north":{"textures":"textures/blocks/plate_north"},
            "plate_south":{"textures":"textures/blocks/plate_south"}
        }}"#,
        "[]",
    );
    for (index, path) in ["west", "east", "down", "up", "north", "south"]
        .into_iter()
        .enumerate()
    {
        write_png(
            root,
            &format!("textures/blocks/plate_{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 * 31 + 1, 70, 110, 255]),
        );
    }
}

#[test]
fn compiler_routes_all_generated_pressure_plate_states_to_exact_opaque_cuboids() {
    const PRESSED: u32 = 1 << 1;
    let directory = tempfile::tempdir().expect("create pressure-plate fixture");
    write_pressure_plate_pack(directory.path());
    let records = generated_pressure_plate_records("minecraft:wooden_pressure_plate");
    assert_eq!(records.len(), 16, "redstone signal 0..15");
    assert_eq!(
        records
            .iter()
            .filter(|record| record.model_state.get(ModelStateField::Flags) == Some(0))
            .count(),
        1,
        "only signal zero is unpressed"
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| record.model_state.get(ModelStateField::Flags) == Some(PRESSED))
            .count(),
        15,
        "signals 1..15 are pressed"
    );

    let compiled = compile_pack(directory.path(), &records).expect("compile all pressure plates");
    assert_eq!(
        compiled.materials.len(),
        7,
        "diagnostic plus six opaque faces"
    );
    assert_eq!(compiled.model_templates.len(), 2, "up and down templates");
    for (id, record) in records.iter().enumerate() {
        let flags = record.model_state.get(ModelStateField::Flags).unwrap();
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.canonical_state);
        assert!(!visual.flags.intersects(
            BlockFlags::AIR
                | BlockFlags::CUBE_GEOMETRY
                | BlockFlags::OCCLUDES_FULL_FACE
                | BlockFlags::LEAF_MODEL
        ));
        assert_eq!(record.face_coverage, 0);
        let quads = compiled_model_quads(&compiled, id);
        assert_eq!(quads.len(), 6);
        assert_eq!(
            model_bounds(quads),
            if flags == 0 {
                ([16, 0, 16], [240, 16, 240])
            } else {
                ([16, 0, 16], [240, 8, 240])
            },
            "{}",
            record.canonical_state
        );
        let side_bottom_v = if flags == 0 { 4096 } else { 3968 };
        let side_top_v = 3840;
        let expected_uvs = [
            [
                [256, side_bottom_v],
                [3840, side_bottom_v],
                [3840, side_top_v],
                [256, side_top_v],
            ],
            [
                [256, side_bottom_v],
                [256, side_top_v],
                [3840, side_top_v],
                [3840, side_bottom_v],
            ],
            [[256, 256], [3840, 256], [3840, 3840], [256, 3840]],
            [[256, 256], [256, 3840], [3840, 3840], [3840, 256]],
            [
                [256, side_bottom_v],
                [256, side_top_v],
                [3840, side_top_v],
                [3840, side_bottom_v],
            ],
            [
                [256, side_bottom_v],
                [3840, side_bottom_v],
                [3840, side_top_v],
                [256, side_top_v],
            ],
        ];
        for (face, quad) in quads.iter().enumerate() {
            assert_eq!(quad.material, visual.faces[face]);
            assert_eq!(quad.flags, [3, 4, 1, 2, 5, 6][face]);
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
            assert_eq!(quad.uvs, expected_uvs[face]);
            assert_eq!(compiled.materials[quad.material as usize].flags, 0);
        }
    }

    let baseline = encode_blob(&compiled).expect("encode exhaustive pressure plates");
    let mut reversed = records.clone();
    reversed.reverse();
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &reversed).unwrap()).unwrap(),
        baseline,
        "pressure-plate compilation depends on registry ordering"
    );
    let mut without_collision = records;
    for record in &mut without_collision {
        record.collision_seed = CollisionSeed::default();
    }
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &without_collision).unwrap()).unwrap(),
        baseline,
        "collision-only seeds changed typed pressure-plate render geometry"
    );
}

#[test]
fn compiler_pressure_plate_selector_fails_closed_when_missing_or_out_of_range() {
    let directory = tempfile::tempdir().expect("create invalid pressure-plate fixture");
    write_pressure_plate_pack(directory.path());
    let mut records = vec![model_record(
        0,
        91_000,
        "minecraft:wooden_pressure_plate",
        "{}",
        ModelFamily::PressurePlate,
    )];
    for flags in [1, 3, 4, u32::MAX] {
        let id = records.len() as u32;
        records.push(encoded_model_record(
            id,
            91_000 + id,
            "minecraft:wooden_pressure_plate",
            ModelFamily::PressurePlate,
            &[(ModelStateField::Flags, flags)],
        ));
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile invalid selectors");
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
fn compiler_real_pinned_pack_has_zero_diagnostic_pressure_plate_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.model_family == ModelFamily::PressurePlate)
    .collect::<Vec<_>>();
    assert_eq!(records.len(), 256);
    assert_eq!(
        records
            .iter()
            .map(|record| record.name.as_ref())
            .collect::<HashSet<_>>()
            .len(),
        16
    );
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 92_000 + id as u32;
    }
    let compiled =
        compile_pack(Path::new(&pack), &records).expect("compile pinned pressure plates");
    for (id, record) in records.iter().enumerate() {
        assert_eq!(
            compiled.visuals[id].kind,
            VisualKind::Model,
            "{}",
            record.name
        );
        assert_eq!(record.face_coverage, 0);
        assert!(compiled_model_quads(&compiled, id).iter().all(|quad| {
            quad.material != DIAGNOSTIC_MATERIAL
                && quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK == 0
                && compiled.materials[quad.material as usize].flags == 0
        }));
    }
    let baseline = encode_blob(&compiled).expect("encode pinned pressure plates");
    records.reverse();
    let reversed = compile_pack(Path::new(&pack), &records).expect("compile reversed plates");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}
