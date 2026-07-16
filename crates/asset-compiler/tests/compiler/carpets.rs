use super::support::*;

fn generated_carpet_records() -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.model_family == ModelFamily::Carpet)
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| record.sequential_id);
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 95_000 + id as u32;
    }
    records
}

fn write_carpet_pack(root: &Path) {
    let colours = [
        "black",
        "blue",
        "brown",
        "cyan",
        "gray",
        "green",
        "light_blue",
        "silver",
        "lime",
        "magenta",
        "orange",
        "pink",
        "purple",
        "red",
        "white",
        "yellow",
    ];
    let blocks = [
        ("black_carpet", "wool_colored_black"),
        ("blue_carpet", "wool_colored_blue"),
        ("brown_carpet", "wool_colored_brown"),
        ("cyan_carpet", "wool_colored_cyan"),
        ("gray_carpet", "wool_colored_gray"),
        ("green_carpet", "wool_colored_green"),
        ("light_blue_carpet", "wool_colored_light_blue"),
        ("light_gray_carpet", "wool_colored_silver"),
        ("lime_carpet", "wool_colored_lime"),
        ("magenta_carpet", "wool_colored_magenta"),
        ("moss_carpet", "moss_block"),
        ("orange_carpet", "wool_colored_orange"),
        ("pale_moss_carpet", "pale_moss_block"),
        ("pink_carpet", "wool_colored_pink"),
        ("purple_carpet", "wool_colored_purple"),
        ("red_carpet", "wool_colored_red"),
        ("white_carpet", "wool_colored_white"),
        ("yellow_carpet", "wool_colored_yellow"),
    ];
    let block_json = format!(
        "{{{}}}",
        blocks
            .into_iter()
            .map(|(name, key)| format!(r#""{name}":{{"textures":"{key}"}}"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut terrain = colours
        .into_iter()
        .map(|colour| {
            format!(
                r#""wool_colored_{colour}":{{"textures":"textures/blocks/wool_colored_{colour}"}}"#
            )
        })
        .collect::<Vec<_>>();
    terrain.extend([
        r#""moss_block":{"textures":"textures/blocks/moss_block"}"#.to_owned(),
        r#""pale_moss_block":{"textures":"textures/blocks/pale_moss_block"}"#.to_owned(),
        r#""pale_moss_carpet_side":{"textures":["textures/blocks/pale_moss_carpet_side_base","textures/blocks/pale_moss_carpet_side_tip"]}"#.to_owned(),
    ]);
    write_pack(
        root,
        &block_json,
        &format!(r#"{{"texture_data":{{{}}}}}"#, terrain.join(",")),
        "[]",
    );
    for (index, colour) in colours.into_iter().enumerate() {
        write_png(
            root,
            &format!("textures/blocks/wool_colored_{colour}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 50, 90, 255]),
        );
    }
    for (index, path) in ["moss_block", "pale_moss_block"].into_iter().enumerate() {
        write_png(
            root,
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [80 + index as u8, 120, 40, 255]),
        );
    }
    for (index, path) in ["pale_moss_carpet_side_base", "pale_moss_carpet_side_tip"]
        .into_iter()
        .enumerate()
    {
        let mut pixels = solid(TILE_SIZE, TILE_SIZE, [30 + index as u8, 90, 20, 255]);
        for (pixel_index, pixel) in pixels.iter_mut().enumerate() {
            if pixel_index % (index + 2) == 0 {
                pixel[3] = 0;
            }
        }
        write_png(
            root,
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
    }
}

fn pale_carpet_selector(record: &RegistryRecord) -> ([u8; 4], bool) {
    assert_eq!(
        record.model_state.mask(),
        1 << (ModelStateField::Flags as u8 - 1)
    );
    let flags = record
        .model_state
        .get(ModelStateField::Flags)
        .expect("pale carpet flags");
    assert!(matches!(flags, 0 | MODEL_FLAG_UPPER));
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .expect("parse pale carpet state");
    assert_eq!(state.len(), 5);
    let side = |direction| match carpet_state_value(
        &state,
        &format!("pale_moss_carpet_side_{direction}"),
        "string",
    )
    .as_str()
    .expect("string side")
    {
        "none" => 0,
        "short" => 1,
        "tall" => 2,
        value => panic!("invalid side {value}"),
    };
    let upper = carpet_state_value(&state, "upper_block_bit", "byte")
        .as_u64()
        .expect("byte upper bit")
        != 0;
    assert_eq!(upper, flags == MODEL_FLAG_UPPER);
    (
        [side("east"), side("north"), side("south"), side("west")],
        upper,
    )
}

#[test]
fn generated_carpet_registry_has_exact_ordinary_and_pale_selector_contract() {
    let records = generated_carpet_records();
    assert_eq!(records.len(), 179);
    let ordinary = records
        .iter()
        .filter(|record| record.name.as_ref() != "minecraft:pale_moss_carpet")
        .collect::<Vec<_>>();
    assert_eq!(ordinary.len(), 17);
    assert!(ordinary.iter().all(|record| {
        record.canonical_state.as_ref() == "{}" && record.model_state.mask() == 0
    }));
    let pale = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:pale_moss_carpet")
        .collect::<Vec<_>>();
    assert_eq!(pale.len(), 162);
    let selectors = pale
        .into_iter()
        .map(pale_carpet_selector)
        .collect::<HashSet<_>>();
    let expected = (0..3)
        .flat_map(|east| {
            (0..3).flat_map(move |north| {
                (0..3).flat_map(move |south| {
                    (0..3).flat_map(move |west| {
                        [false, true]
                            .into_iter()
                            .map(move |upper| ([east, north, south, west], upper))
                    })
                })
            })
        })
        .collect::<HashSet<_>>();
    assert_eq!(selectors, expected);
}

#[test]
fn compiler_covers_all_carpet_states_with_exact_geometry_materials_and_determinism() {
    let directory = tempfile::tempdir().expect("create carpet fixture");
    write_carpet_pack(directory.path());
    let records = generated_carpet_records();
    let compiled = compile_pack(directory.path(), &records).expect("compile all carpets");
    assert_eq!(
        compiled.materials.len(),
        21,
        "diagnostic, 18 opaque, two cutout"
    );
    for (id, record) in records.iter().enumerate() {
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
        if record.name.as_ref() != "minecraft:pale_moss_carpet" {
            assert_eq!(quads.len(), 6);
            assert_eq!(model_bounds(quads), ([0, 0, 0], [256, 16, 256]));
            assert!(quads.iter().all(|quad| {
                quad.material != DIAGNOSTIC_MATERIAL
                    && compiled.materials[quad.material as usize].flags == 0
                    && quad.flags & (MODEL_QUAD_FLAG_CULL_FACE_MASK | MODEL_QUAD_FLAG_TWO_SIDED)
                        == 0
            }));
            continue;
        }
        let (sides, upper) = pale_carpet_selector(record);
        let isolated_upper = upper && sides == [0; 4];
        let has_base = !upper || isolated_upper;
        let side_count = if isolated_upper {
            4
        } else {
            sides.into_iter().filter(|side| *side != 0).count()
        };
        assert_eq!(quads.len(), usize::from(has_base) * 6 + side_count * 2);
        let base_count = usize::from(has_base) * 6;
        if has_base {
            assert_eq!(model_bounds(&quads[..6]), ([0, 0, 0], [256, 16, 256]));
            assert!(quads[..6].iter().all(|quad| {
                compiled.materials[quad.material as usize].flags == 0
                    && quad.flags & MODEL_QUAD_FLAG_TWO_SIDED == 0
            }));
        }
        for quad in &quads[base_count..] {
            assert_eq!(
                compiled.materials[quad.material as usize].flags,
                MATERIAL_FLAG_ALPHA_CUTOUT
            );
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
            assert_eq!(quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
            assert!(
                quad.uvs
                    .iter()
                    .flatten()
                    .all(|value| matches!(*value, 0 | 4096))
            );
            let face = quad.flags & MODEL_QUAD_FLAG_FACE_MASK;
            let bounds = model_bounds(std::slice::from_ref(quad));
            assert!(matches!(
                (face, bounds),
                (3 | 4, ([2, 0, 0], [2, 256, 256]))
                    | (3 | 4, ([254, 0, 0], [254, 256, 256]))
                    | (5 | 6, ([0, 0, 2], [256, 256, 2]))
                    | (5 | 6, ([0, 0, 254], [256, 256, 254]))
            ));
        }
    }
    let first_material_pixel = |name: &str, selector: Option<([u8; 4], bool)>| {
        let id = records
            .iter()
            .position(|record| {
                record.name.as_ref() == name
                    && selector.is_none_or(|selector| pale_carpet_selector(record) == selector)
            })
            .expect("requested carpet state");
        let quad = compiled_model_quads(&compiled, id)
            .last()
            .expect("carpet template quad");
        let material = compiled.materials[quad.material as usize];
        assert_eq!(material.texture.page(), 0);
        mip_pixel(&compiled, 0, material.texture.layer(), 0, 0)
    };
    assert_eq!(
        first_material_pixel("minecraft:light_gray_carpet", None),
        [8, 50, 90, 255],
        "light gray must select wool_colored_silver"
    );
    assert_eq!(
        first_material_pixel("minecraft:moss_carpet", None),
        [80, 120, 40, 255],
        "moss carpet must select moss_block"
    );
    assert_eq!(
        first_material_pixel("minecraft:pale_moss_carpet", Some(([1, 0, 0, 0], false))),
        [31, 90, 20, 0],
        "short must select pair index 1 / side_tip / Java small"
    );
    assert_eq!(
        first_material_pixel("minecraft:pale_moss_carpet", Some(([2, 0, 0, 0], false))),
        [30, 90, 20, 0],
        "tall must select pair index 0 / side_base / Java tall"
    );
    let baseline = encode_blob(&compiled).expect("encode carpets");
    let mut reversed = records.clone();
    reversed.reverse();
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &reversed).unwrap()).unwrap(),
        baseline,
        "carpet compilation depends on registry ordering"
    );
    let mut without_collision = records;
    for record in &mut without_collision {
        record.collision_seed = CollisionSeed::default();
    }
    assert_eq!(
        encode_blob(&compile_pack(directory.path(), &without_collision).unwrap()).unwrap(),
        baseline,
        "collision-only seeds changed carpet render geometry"
    );
}

#[test]
fn compiler_pale_moss_side_planes_preserve_both_java_face_uv_orders() {
    let directory = tempfile::tempdir().expect("create pale moss UV fixture");
    write_carpet_pack(directory.path());
    let generated = generated_carpet_records();
    let selectors = [
        ([2, 0, 0, 0], true),
        ([0, 2, 0, 0], true),
        ([0, 0, 2, 0], true),
        ([0, 0, 0, 2], true),
    ];
    let mut records = selectors
        .into_iter()
        .map(|selector| {
            generated
                .iter()
                .find(|record| {
                    record.name.as_ref() == "minecraft:pale_moss_carpet"
                        && pale_carpet_selector(record) == selector
                })
                .expect("requested pale moss direction")
                .clone()
        })
        .collect::<Vec<_>>();
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 95_500 + id as u32;
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile pale moss UV fixture");

    let expected = [
        (
            "east",
            4,
            3,
            [[254, 0, 0], [254, 256, 0], [254, 256, 256], [254, 0, 256]],
            [[4096, 4096], [4096, 0], [0, 0], [0, 4096]],
            [[254, 0, 0], [254, 0, 256], [254, 256, 256], [254, 256, 0]],
            [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
        ),
        (
            "north",
            5,
            6,
            [[0, 0, 2], [0, 256, 2], [256, 256, 2], [256, 0, 2]],
            [[4096, 4096], [4096, 0], [0, 0], [0, 4096]],
            [[0, 0, 2], [256, 0, 2], [256, 256, 2], [0, 256, 2]],
            [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
        ),
        (
            "south",
            6,
            5,
            [[0, 0, 254], [256, 0, 254], [256, 256, 254], [0, 256, 254]],
            [[4096, 4096], [0, 4096], [0, 0], [4096, 0]],
            [[0, 0, 254], [0, 256, 254], [256, 256, 254], [256, 0, 254]],
            [[0, 4096], [0, 0], [4096, 0], [4096, 4096]],
        ),
        (
            "west",
            3,
            4,
            [[2, 0, 0], [2, 0, 256], [2, 256, 256], [2, 256, 0]],
            [[4096, 4096], [0, 4096], [0, 0], [4096, 0]],
            [[2, 0, 0], [2, 256, 0], [2, 256, 256], [2, 0, 256]],
            [[0, 4096], [0, 0], [4096, 0], [4096, 4096]],
        ),
    ];
    for (
        id,
        (
            direction,
            outward_face,
            inward_face,
            outward_positions,
            outward_uvs,
            inward_positions,
            inward_uvs,
        ),
    ) in expected.into_iter().enumerate()
    {
        let quads = compiled_model_quads(&compiled, id);
        assert_eq!(
            quads.len(),
            2,
            "one explicit quad per Java {direction} face"
        );
        assert_eq!(
            quads[0].positions, outward_positions,
            "{direction} outward positions"
        );
        assert_eq!(quads[0].uvs, outward_uvs, "{direction} outward UVs");
        assert_eq!(quads[0].flags, outward_face, "{direction} outward face");
        assert_eq!(
            quads[1].positions, inward_positions,
            "{direction} inward positions"
        );
        assert_eq!(quads[1].uvs, inward_uvs, "{direction} inward UVs");
        assert_eq!(quads[1].flags, inward_face, "{direction} inward face");
        assert_eq!(quads[0].material, quads[1].material);
        assert_eq!(
            compiled.materials[quads[0].material as usize].flags,
            MATERIAL_FLAG_ALPHA_CUTOUT
        );
    }
}

#[test]
fn compiler_carpet_selectors_fail_closed_when_missing_invalid_or_extra() {
    let directory = tempfile::tempdir().expect("create invalid carpet fixture");
    write_carpet_pack(directory.path());
    let generated = generated_carpet_records();
    let ordinary = generated
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:black_carpet")
        .unwrap();
    let pale = generated
        .iter()
        .find(|record| {
            record.name.as_ref() == "minecraft:pale_moss_carpet"
                && pale_carpet_selector(record) == ([0; 4], false)
        })
        .unwrap();
    let typed = |fields: &[(ModelStateField, u32)]| {
        encoded_model_record(
            0,
            1,
            "minecraft:pale_moss_carpet",
            ModelFamily::Carpet,
            fields,
        )
        .model_state
    };
    let mut records = Vec::new();
    let mut extra_ordinary = ordinary.clone();
    extra_ordinary.model_state = typed(&[(ModelStateField::Flags, 0)]);
    records.push(extra_ordinary);
    let mut missing_typed = pale.clone();
    missing_typed.model_state = ModelState::default();
    records.push(missing_typed);
    let mut invalid_flags = pale.clone();
    invalid_flags.model_state = typed(&[(ModelStateField::Flags, 1)]);
    records.push(invalid_flags);
    let mut extra_typed = pale.clone();
    extra_typed.model_state = typed(&[(ModelStateField::Flags, 0), (ModelStateField::Half, 0)]);
    records.push(extra_typed);
    for state in [
        r#"{"pale_moss_carpet_side_east":{"type":"string","value":"none"},"pale_moss_carpet_side_north":{"type":"string","value":"none"},"pale_moss_carpet_side_south":{"type":"string","value":"none"},"upper_block_bit":{"type":"byte","value":0}}"#,
        r#"{"extra":{"type":"byte","value":0},"pale_moss_carpet_side_east":{"type":"string","value":"none"},"pale_moss_carpet_side_north":{"type":"string","value":"none"},"pale_moss_carpet_side_south":{"type":"string","value":"none"},"pale_moss_carpet_side_west":{"type":"string","value":"none"},"upper_block_bit":{"type":"byte","value":0}}"#,
        r#"{"pale_moss_carpet_side_east":{"type":"string","value":"low"},"pale_moss_carpet_side_north":{"type":"string","value":"none"},"pale_moss_carpet_side_south":{"type":"string","value":"none"},"pale_moss_carpet_side_west":{"type":"string","value":"none"},"upper_block_bit":{"type":"byte","value":0}}"#,
        r#"{"pale_moss_carpet_side_east":{"type":"byte","value":"none"},"pale_moss_carpet_side_north":{"type":"string","value":"none"},"pale_moss_carpet_side_south":{"type":"string","value":"none"},"pale_moss_carpet_side_west":{"type":"string","value":"none"},"upper_block_bit":{"type":"byte","value":0}}"#,
        r#"{"pale_moss_carpet_side_east":{"type":"string","value":"none"},"pale_moss_carpet_side_north":{"type":"string","value":"none"},"pale_moss_carpet_side_south":{"type":"string","value":"none"},"pale_moss_carpet_side_west":{"type":"string","value":"none"},"upper_block_bit":{"type":"byte","value":1}}"#,
    ] {
        let mut invalid = pale.clone();
        invalid.canonical_state = state.into();
        records.push(invalid);
    }
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 96_000 + id as u32;
    }
    let compiled = compile_pack(directory.path(), &records).expect("compile invalid carpets");
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
fn compiler_real_pinned_pack_has_zero_diagnostic_carpet_states_when_requested() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let mut records = generated_carpet_records();
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned carpets");
    assert_eq!(records.len(), 179);
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
    let baseline = encode_blob(&compiled).expect("encode pinned carpets");
    records.reverse();
    let reversed = compile_pack(Path::new(&pack), &records).expect("compile reversed carpets");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}
