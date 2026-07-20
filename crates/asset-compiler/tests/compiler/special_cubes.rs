use super::support::*;

fn write_chiseled_bookshelf_pack(root: &Path, terrain_overrides: Option<&str>) {
    write_pack(
        root,
        r#"{"chiseled_bookshelf":{"textures":{"down":"chiseled_bookshelf_top","up":"chiseled_bookshelf_top","north":"chiseled_bookshelf_front","east":"chiseled_bookshelf_side","south":"chiseled_bookshelf_side","west":"chiseled_bookshelf_side"}}}"#,
        terrain_overrides.unwrap_or(
            r#"{"texture_data":{
                "chiseled_bookshelf_front":{"textures":["textures/blocks/chiseled_bookshelf_empty","textures/blocks/chiseled_bookshelf_occupied"]},
                "chiseled_bookshelf_side":{"textures":"textures/blocks/chiseled_bookshelf_side"},
                "chiseled_bookshelf_top":{"textures":"textures/blocks/chiseled_bookshelf_top"}
            }}"#,
        ),
        "[]",
    );
    for (path, color) in [
        (
            "textures/blocks/chiseled_bookshelf_empty",
            [10, 20, 30, 255],
        ),
        (
            "textures/blocks/chiseled_bookshelf_occupied",
            [40, 50, 60, 255],
        ),
        ("textures/blocks/chiseled_bookshelf_side", [70, 80, 90, 255]),
        (
            "textures/blocks/chiseled_bookshelf_top",
            [100, 110, 120, 255],
        ),
    ] {
        write_png(
            root,
            path,
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, color),
        );
    }
}

fn chiseled_bookshelf_records() -> Vec<RegistryRecord> {
    let mut records = Vec::with_capacity(256);
    for books in 0..64_u32 {
        for direction in 0..4_u32 {
            let id = 1605 + books * 4 + direction;
            let mut record = encoded_model_record(
                id,
                100_000 + id,
                "minecraft:chiseled_bookshelf",
                ModelFamily::ChiseledBookshelf,
                &[
                    (ModelStateField::Connections, books),
                    (ModelStateField::Orientation, direction),
                ],
            );
            record.canonical_state = serde_json::json!({
                "books_stored": {"type": "int", "value": books},
                "direction": {"type": "int", "value": direction}
            })
            .to_string()
            .into();
            record.flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
            record.face_coverage = 0x3f;
            record.collision_seed = CollisionSeed {
                shape_id: 1,
                confidence: CollisionConfidence::CollisionOnly,
                boxes: vec![CollisionBox {
                    max_x: 100_000_000,
                    max_y: 100_000_000,
                    max_z: 100_000_000,
                    ..CollisionBox::default()
                }]
                .into_boxed_slice(),
            };
            records.push(record);
        }
    }
    records
}

#[test]
fn compiler_emits_exact_chiseled_bookshelf_materials_templates_and_uv_partition() {
    let directory = tempfile::tempdir().expect("create chiseled-bookshelf fixture");
    write_chiseled_bookshelf_pack(directory.path(), None);
    let mut records = chiseled_bookshelf_records();

    let compiled = compile_pack(directory.path(), &records).expect("compile chiseled bookshelves");
    let admitted = records
        .iter()
        .map(|record| compiled.visuals[record.sequential_id as usize])
        .collect::<Vec<_>>();
    assert!(admitted.iter().all(|visual| {
        visual.kind == VisualKind::Model
            && visual.flags == BlockFlags::OCCLUDES_FULL_FACE
            && visual.model_template != assets::NO_MODEL_TEMPLATE
    }));
    assert_eq!(
        compiled.materials.len(),
        5,
        "diagnostic plus four source materials"
    );
    assert_eq!(compiled.model_templates.len(), 64);
    assert_eq!(compiled.model_quads.len(), 64 * 11);

    let empty_visual = compiled.visuals[(1605 + 2) as usize];
    let empty = template_quads(&compiled, empty_visual.model_template)[5].material;
    let occupied_visual = compiled.visuals[(1605 + 63 * 4 + 2) as usize];
    let occupied = template_quads(&compiled, occupied_visual.model_template)[5].material;

    for books in [0_u32, 1, 2, 4, 8, 16, 32, 63] {
        let visuals = (0..4)
            .map(|direction| compiled.visuals[(1605 + books * 4 + direction) as usize])
            .collect::<Vec<_>>();
        assert!(
            visuals
                .iter()
                .all(|visual| visual.model_template == visuals[0].model_template)
        );
        for (direction, visual) in visuals.iter().enumerate() {
            assert_eq!(visual.variant, (direction as u32 + 2) & 3);
        }
        let quads = template_quads(&compiled, visuals[0].model_template);
        assert_eq!(quads.len(), 11);
        assert_eq!(model_bounds(quads), ([0, 0, 0], [256, 256, 256]));
        assert!(quads.iter().all(|quad| {
            let face = quad.flags & MODEL_QUAD_FLAG_FACE_MASK;
            let cull = (quad.flags & MODEL_QUAD_FLAG_CULL_FACE_MASK) >> 4;
            face != 0 && face == cull
        }));

        let slots = &quads[5..];
        let mut x_boundaries = slots
            .iter()
            .flat_map(|quad| quad.positions.iter().map(|position| position[0]))
            .collect::<Vec<_>>();
        x_boundaries.sort_unstable();
        x_boundaries.dedup();
        assert_eq!(x_boundaries, [0, 85, 171, 256]);
        let mut u_boundaries = slots
            .iter()
            .flat_map(|quad| quad.uvs.iter().map(|uv| uv[0]))
            .collect::<Vec<_>>();
        u_boundaries.sort_unstable();
        u_boundaries.dedup();
        assert_eq!(u_boundaries, [0, 1365, 2731, 4096]);
        let mut y_boundaries = slots
            .iter()
            .flat_map(|quad| quad.positions.iter().map(|position| position[1]))
            .collect::<Vec<_>>();
        y_boundaries.sort_unstable();
        y_boundaries.dedup();
        assert_eq!(y_boundaries, [0, 128, 256]);
        for (slot, quad) in slots.iter().enumerate() {
            assert_eq!(
                quad.material,
                if books & (1 << slot) == 0 {
                    empty
                } else {
                    occupied
                }
            );
        }
    }

    let baseline = encode_blob(&compiled).expect("encode chiseled bookshelf assets");
    records.reverse();
    let reversed = compile_pack(directory.path(), &records).expect("compile reversed records");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

#[test]
fn compiler_chiseled_bookshelf_admission_fails_closed_as_a_complete_family() {
    let valid_pack = tempfile::tempdir().expect("create exact pack");
    write_chiseled_bookshelf_pack(valid_pack.path(), None);
    let records = chiseled_bookshelf_records();

    let mut families = Vec::new();
    let mut missing = records.clone();
    missing.pop();
    families.push(missing);
    let mut wrong_id = records.clone();
    wrong_id[0].sequential_id = 1604;
    families.push(wrong_id);
    let mut wrong_state = records.clone();
    wrong_state[0].canonical_state =
        r#"{"books_stored":{"type":"byte","value":0},"direction":{"type":"int","value":0}}"#.into();
    families.push(wrong_state);
    let mut extra_state = records.clone();
    extra_state[0].canonical_state = r#"{"books_stored":{"type":"int","value":0},"direction":{"type":"int","value":0},"extra":{"type":"int","value":0}}"#.into();
    families.push(extra_state);
    let mut wrong_flags = records.clone();
    wrong_flags[0].flags = BlockFlags::CUBE_GEOMETRY;
    families.push(wrong_flags);
    let mut wrong_collision = records.clone();
    wrong_collision[0].collision_seed.boxes[0].max_y -= 1;
    families.push(wrong_collision);

    for (index, family) in families.iter().enumerate() {
        let compiled = compile_pack(valid_pack.path(), family).expect("compile rejected family");
        assert!(
            family.iter().all(|record| {
                compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
            }),
            "invalid family {index} leaked a supported visual"
        );
    }

    for terrain in [
        r#"{"texture_data":{"chiseled_bookshelf_front":{"textures":["textures/blocks/chiseled_bookshelf_empty"]},"chiseled_bookshelf_side":{"textures":"textures/blocks/chiseled_bookshelf_side"},"chiseled_bookshelf_top":{"textures":"textures/blocks/chiseled_bookshelf_top"}}}"#,
        r#"{"texture_data":{"chiseled_bookshelf_front":{"textures":["textures/blocks/chiseled_bookshelf_empty","textures/blocks/chiseled_bookshelf_occupied","textures/blocks/chiseled_bookshelf_extra"]},"chiseled_bookshelf_side":{"textures":"textures/blocks/chiseled_bookshelf_side"},"chiseled_bookshelf_top":{"textures":"textures/blocks/chiseled_bookshelf_top"}}}"#,
        r#"{"texture_data":{"chiseled_bookshelf_front":{"textures":["textures/blocks/chiseled_bookshelf_empty","textures/blocks/chiseled_bookshelf_occupied"]},"chiseled_bookshelf_side":{"textures":["textures/blocks/chiseled_bookshelf_side"]},"chiseled_bookshelf_top":{"textures":"textures/blocks/chiseled_bookshelf_top"}}}"#,
    ] {
        let malformed = tempfile::tempdir().expect("create malformed terrain fixture");
        write_chiseled_bookshelf_pack(malformed.path(), Some(terrain));
        let compiled = compile_pack(malformed.path(), &records).expect("compile malformed terrain");
        assert!(records.iter().all(|record| {
            compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
        }));
    }
}

#[test]
fn compiler_chiseled_bookshelf_rejects_overlay_metadata() {
    let records = chiseled_bookshelf_records();
    for (label, terrain) in [
        (
            "front pair",
            r##"{"texture_data":{"chiseled_bookshelf_front":{"textures":[{"path":"textures/blocks/chiseled_bookshelf_empty","overlay_color":"#ffffff"},"textures/blocks/chiseled_bookshelf_occupied"]},"chiseled_bookshelf_side":{"textures":"textures/blocks/chiseled_bookshelf_side"},"chiseled_bookshelf_top":{"textures":"textures/blocks/chiseled_bookshelf_top"}}}"##,
        ),
        (
            "side static",
            r##"{"texture_data":{"chiseled_bookshelf_front":{"textures":["textures/blocks/chiseled_bookshelf_empty","textures/blocks/chiseled_bookshelf_occupied"]},"chiseled_bookshelf_side":{"textures":{"path":"textures/blocks/chiseled_bookshelf_side","overlay_color":"#ffffff"}},"chiseled_bookshelf_top":{"textures":"textures/blocks/chiseled_bookshelf_top"}}}"##,
        ),
        (
            "top static",
            r##"{"texture_data":{"chiseled_bookshelf_front":{"textures":["textures/blocks/chiseled_bookshelf_empty","textures/blocks/chiseled_bookshelf_occupied"]},"chiseled_bookshelf_side":{"textures":"textures/blocks/chiseled_bookshelf_side"},"chiseled_bookshelf_top":{"textures":{"path":"textures/blocks/chiseled_bookshelf_top","overlay_color":"#ffffff"}}}}"##,
        ),
    ] {
        let malformed = tempfile::tempdir().expect("create overlay terrain fixture");
        write_chiseled_bookshelf_pack(malformed.path(), Some(terrain));
        let compiled = compile_pack(malformed.path(), &records).expect("compile overlay terrain");
        assert!(
            records.iter().all(|record| {
                compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
            }),
            "{label} overlay metadata was accepted"
        );
    }
}

#[test]
fn compiler_chiseled_bookshelf_rejects_extra_side_fallback() {
    let records = chiseled_bookshelf_records();
    let fallback = tempfile::tempdir().expect("create extra-side block fixture");
    write_chiseled_bookshelf_pack(fallback.path(), None);
    write_file(
        fallback.path().join("blocks.json"),
        r#"{"chiseled_bookshelf":{"textures":{"down":"chiseled_bookshelf_top","up":"chiseled_bookshelf_top","north":"chiseled_bookshelf_front","east":"chiseled_bookshelf_side","south":"chiseled_bookshelf_side","west":"chiseled_bookshelf_side","side":"chiseled_bookshelf_side"}}}"#,
    );
    let compiled = compile_pack(fallback.path(), &records).expect("compile extra-side block map");
    assert!(records.iter().all(|record| {
        compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
    }));
}

fn write_bee_housing_pack(root: &Path) {
    write_pack(
        root,
        r#"{
            "bee_nest":{"textures":{"down":"bee_nest_bottom","east":"bee_nest_side","north":"bee_nest_side","south":"bee_nest_front","up":"bee_nest_top","west":"bee_nest_side"}},
            "beehive":{"textures":{"down":"beehive_top","east":"beehive_side","north":"beehive_side","south":"beehive_front","up":"beehive_top","west":"beehive_side"}}
        }"#,
        r#"{"texture_data":{
            "bee_nest_bottom":{"textures":["textures/blocks/bee_nest_bottom"]},
            "bee_nest_front":{"textures":["textures/blocks/bee_nest_front","textures/blocks/bee_nest_front_honey"]},
            "bee_nest_side":{"textures":["textures/blocks/bee_nest_side"]},
            "bee_nest_top":{"textures":["textures/blocks/bee_nest_top"]},
            "beehive_front":{"textures":["textures/blocks/beehive_front","textures/blocks/beehive_front_honey"]},
            "beehive_side":{"textures":["textures/blocks/beehive_side"]},
            "beehive_top":{"textures":["textures/blocks/beehive_top"]}
        }}"#,
        "[]",
    );
    for (name, colour) in [
        ("bee_nest_bottom", [11, 12, 13, 255]),
        ("bee_nest_front", [21, 22, 23, 255]),
        ("bee_nest_front_honey", [31, 32, 33, 255]),
        ("bee_nest_side", [41, 42, 43, 255]),
        ("bee_nest_top", [51, 52, 53, 255]),
        ("beehive_front", [61, 62, 63, 255]),
        ("beehive_front_honey", [71, 72, 73, 255]),
        ("beehive_side", [81, 82, 83, 255]),
        ("beehive_top", [91, 92, 93, 255]),
    ] {
        write_png(
            root,
            &format!("textures/blocks/{name}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, colour),
        );
    }
}

#[test]
fn compiler_emits_exact_compact_bee_housing_cubes_for_all_states_and_network_modes() {
    let directory = tempfile::tempdir().expect("create bee fixture");
    write_bee_housing_pack(directory.path());
    let mut records = bee_housing_records();
    let compiled = compile_pack(directory.path(), &records).expect("compile exact bee family");

    assert_eq!(
        compiled.materials.len(),
        10,
        "diagnostic plus nine exact textures"
    );
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
    assert!(
        compiled.materials[1..]
            .iter()
            .all(|material| { material.flags == 0 && material.animation == assets::NO_ANIMATION })
    );

    for record in &records {
        let direction = record
            .model_state
            .get(ModelStateField::Orientation)
            .expect("bee direction") as usize;
        let honey = record
            .model_state
            .get(ModelStateField::Growth)
            .expect("bee honey level");
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Cube);
        assert_eq!(
            visual.flags,
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
        );
        assert_eq!(visual.model_template, assets::NO_MODEL_TEMPLATE);
        assert_eq!(visual.variant, 0);
        assert!(
            visual
                .faces
                .iter()
                .all(|&material| material != DIAGNOSTIC_MATERIAL)
        );

        let front_face = [
            BlockFace::South,
            BlockFace::West,
            BlockFace::North,
            BlockFace::East,
        ][direction] as usize;
        let ordinary_front =
            compiled.visuals[(record.sequential_id - honey * 4) as usize].faces[front_face];
        let honey_front =
            compiled.visuals[(record.sequential_id - honey * 4 + 20) as usize].faces[front_face];
        assert_eq!(
            visual.faces[front_face],
            if honey == 5 {
                honey_front
            } else {
                ordinary_front
            },
            "{} direction={direction} honey={honey}",
            record.name
        );

        let horizontal = [
            visual.faces[BlockFace::West as usize],
            visual.faces[BlockFace::East as usize],
            visual.faces[BlockFace::North as usize],
            visual.faces[BlockFace::South as usize],
        ];
        for (face, material) in horizontal.into_iter().enumerate() {
            if [
                BlockFace::West,
                BlockFace::East,
                BlockFace::North,
                BlockFace::South,
            ][face] as usize
                != front_face
            {
                assert_ne!(material, visual.faces[front_face]);
            }
        }
        if record.name.as_ref() == "minecraft:beehive" {
            assert_eq!(
                visual.faces[BlockFace::Down as usize],
                visual.faces[BlockFace::Up as usize]
            );
        } else {
            assert_ne!(
                visual.faces[BlockFace::Down as usize],
                visual.faces[BlockFace::Up as usize]
            );
        }
    }

    let blob = encode_blob(&compiled).expect("encode bee assets");
    let runtime = RuntimeAssets::decode(&blob).expect("decode bee assets");
    for record in &records {
        let sequential = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let hashed = runtime.resolve(NetworkIdMode::Hashed, record.network_hash);
        assert_eq!(sequential.kind(), VisualKind::Cube);
        assert_eq!(hashed.kind(), VisualKind::Cube);
        for face in BlockFace::ALL {
            assert_eq!(
                sequential.face(face).material_id(),
                hashed.face(face).material_id()
            );
        }
    }
    assert_eq!(runtime.missing_count(), 0);

    let baseline = blob;
    records.reverse();
    let reversed = compile_pack(directory.path(), &records).expect("compile reversed bee family");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

#[test]
fn compiler_bee_housing_admission_is_atomic_and_pack_routes_are_exact() {
    let directory = tempfile::tempdir().expect("create bee fixture");
    write_bee_housing_pack(directory.path());
    let records = bee_housing_records();
    let mut families = Vec::new();
    let mut missing = records.clone();
    missing.remove(0);
    families.push(("missing state", missing));
    let mut wrong_type = records.clone();
    wrong_type[0].canonical_state =
        r#"{"direction":{"type":"byte","value":0},"honey_level":{"type":"int","value":0}}"#.into();
    families.push(("wrong canonical type", wrong_type));
    let mut extra = records.clone();
    extra[0].canonical_state = r#"{"direction":{"type":"int","value":0},"extra":{"type":"int","value":0},"honey_level":{"type":"int","value":0}}"#.into();
    families.push(("extra canonical key", extra));
    let mut wrong_projection = records.clone();
    wrong_projection[0].model_state = encoded_model_record(
        10_395,
        100_395,
        "minecraft:bee_nest",
        ModelFamily::Cube,
        &[
            (ModelStateField::Orientation, 0),
            (ModelStateField::Growth, 1),
        ],
    )
    .model_state;
    families.push(("projection disagreement", wrong_projection));
    let mut wrong_id = records.clone();
    wrong_id[0].sequential_id -= 1;
    families.push(("wrong ID", wrong_id));
    let mut wrong_family = records.clone();
    wrong_family[0].model_family = ModelFamily::Decorative;
    families.push(("wrong family", wrong_family));
    let mut wrong_role = records.clone();
    wrong_role[0].contributor_role = ContributorRole::LiquidAdditional;
    families.push(("wrong role", wrong_role));
    let mut wrong_flags = records.clone();
    wrong_flags[0].flags = BlockFlags::CUBE_GEOMETRY;
    families.push(("wrong flags", wrong_flags));
    let mut wrong_collision = records.clone();
    wrong_collision[0].collision_seed.boxes[0].max_y -= 1;
    families.push(("wrong collision", wrong_collision));

    for (label, family) in families {
        let compiled =
            compile_pack(directory.path(), &family).expect("compile malformed bee family");
        assert!(
            family
                .iter()
                .filter(|record| matches!(
                    record.name.as_ref(),
                    "minecraft:bee_nest" | "minecraft:beehive"
                ))
                .all(|record| {
                    compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
                }),
            "invalid family `{label}` leaked a supported visual"
        );
    }

    for (label, blocks, terrain) in [
        (
            "extra block fallback",
            r#"{"bee_nest":{"textures":{"down":"bee_nest_bottom","east":"bee_nest_side","north":"bee_nest_side","side":"bee_nest_side","south":"bee_nest_front","up":"bee_nest_top","west":"bee_nest_side"}},"beehive":{"textures":{"down":"beehive_top","east":"beehive_side","north":"beehive_side","south":"beehive_front","up":"beehive_top","west":"beehive_side"}}}"#,
            None,
        ),
        (
            "wrong front cardinal",
            r#"{"bee_nest":{"textures":{"down":"bee_nest_bottom","east":"bee_nest_side","north":"bee_nest_front","south":"bee_nest_side","up":"bee_nest_top","west":"bee_nest_side"}},"beehive":{"textures":{"down":"beehive_top","east":"beehive_side","north":"beehive_side","south":"beehive_front","up":"beehive_top","west":"beehive_side"}}}"#,
            None,
        ),
        (
            "front variant count",
            r#"{"bee_nest":{"textures":{"down":"bee_nest_bottom","east":"bee_nest_side","north":"bee_nest_side","south":"bee_nest_front","up":"bee_nest_top","west":"bee_nest_side"}},"beehive":{"textures":{"down":"beehive_top","east":"beehive_side","north":"beehive_side","south":"beehive_front","up":"beehive_top","west":"beehive_side"}}}"#,
            Some(
                r#"{"texture_data":{"bee_nest_bottom":{"textures":"textures/blocks/bee_nest_bottom"},"bee_nest_front":{"textures":["textures/blocks/bee_nest_front"]},"bee_nest_side":{"textures":"textures/blocks/bee_nest_side"},"bee_nest_top":{"textures":"textures/blocks/bee_nest_top"},"beehive_front":{"textures":["textures/blocks/beehive_front","textures/blocks/beehive_front_honey"]},"beehive_side":{"textures":"textures/blocks/beehive_side"},"beehive_top":{"textures":"textures/blocks/beehive_top"}}}"#,
            ),
        ),
    ] {
        let malformed = tempfile::tempdir().expect("create malformed bee pack");
        write_bee_housing_pack(malformed.path());
        write_file(malformed.path().join("blocks.json"), blocks);
        if let Some(terrain) = terrain {
            write_file(
                malformed.path().join("textures/terrain_texture.json"),
                terrain,
            );
        }
        let compiled =
            compile_pack(malformed.path(), &records).expect("compile malformed bee route");
        assert!(
            records.iter().all(|record| {
                compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
            }),
            "nonexact route `{label}` was accepted"
        );
    }
}

#[test]
#[ignore = "requires PINNED_VANILLA_PACK pointing at the ignored pinned vanilla resource pack"]
fn compiler_real_pinned_pack_admits_all_exact_bee_housing_records() {
    let pack = std::env::var_os("PINNED_VANILLA_PACK")
        .expect("set PINNED_VANILLA_PACK to the ignored pinned vanilla resource pack");
    let records = bee_housing_records();
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned bee family");
    assert!(records.iter().all(|record| {
        let visual = compiled.visuals[record.sequential_id as usize];
        visual.kind == VisualKind::Cube
            && visual.flags == BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
            && visual.model_template == assets::NO_MODEL_TEMPLATE
    }));
    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
}

#[test]
#[ignore = "requires PINNED_VANILLA_PACK pointing at the ignored pinned vanilla resource pack"]
fn compiler_real_pinned_pack_admits_all_exact_chiseled_bookshelf_records() {
    let pack = std::env::var_os("PINNED_VANILLA_PACK")
        .expect("set PINNED_VANILLA_PACK to the ignored pinned vanilla resource pack");
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == "minecraft:chiseled_bookshelf")
    .collect::<Vec<_>>();
    assert_eq!(records.len(), 256);
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned bookshelves");
    assert!(records.iter().all(|record| {
        let visual = compiled.visuals[record.sequential_id as usize];
        visual.kind == VisualKind::Model
            && visual.flags == BlockFlags::OCCLUDES_FULL_FACE
            && compiled.model_templates[visual.model_template as usize].quad_count == 11
    }));
    assert_eq!(compiled.materials.len(), 5);
    assert_eq!(compiled.model_templates.len(), 64);
    assert_eq!(compiled.model_quads.len(), 704);
    let baseline = encode_blob(&compiled).expect("encode pinned chiseled bookshelf assets");
    records.reverse();
    let reversed =
        compile_pack(Path::new(&pack), &records).expect("compile reversed pinned records");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

#[test]
fn compiler_output_is_identical_across_shuffled_sources_and_records() {
    fn fixture(blocks: &str, terrain: &str) -> TempDir {
        let directory = tempfile::tempdir().expect("create fixture");
        write_pack(directory.path(), blocks, terrain, "[]");
        write_png(
            directory.path(),
            "textures/blocks/a",
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [10, 20, 30, 255]),
        );
        write_png(
            directory.path(),
            "textures/blocks/z",
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [220, 210, 200, 255]),
        );
        directory
    }

    let first = fixture(
        r#"{"alpha":{"textures":"alpha"},"zeta":{"textures":"zeta"}}"#,
        r#"{"texture_data":{"alpha":{"textures":"textures/blocks/a"},"zeta":{"textures":"textures/blocks/z"}}}"#,
    );
    let second = fixture(
        r#"{"zeta":{"textures":"zeta"},"alpha":{"textures":"alpha"}}"#,
        r#"{"texture_data":{"zeta":{"textures":"textures/blocks/z"},"alpha":{"textures":"textures/blocks/a"}}}"#,
    );
    let alpha = record(
        0,
        0xffff_fff0,
        "minecraft:alpha",
        "{}",
        BlockFlags::CUBE_GEOMETRY,
    );
    let zeta = record(
        1,
        0x8000_0001,
        "minecraft:zeta",
        "{}",
        BlockFlags::CUBE_GEOMETRY,
    );

    let first = compile_pack(first.path(), &[alpha.clone(), zeta.clone()]).expect("first compile");
    let second = compile_pack(second.path(), &[zeta, alpha]).expect("second compile");

    assert_eq!(first, second);
    assert_eq!(
        encode_blob(&first).expect("encode first"),
        encode_blob(&second).expect("encode second")
    );
}

#[test]
fn compiler_selects_huge_mushroom_face_variants_and_keeps_other_arrays_at_zero() {
    let directory = tempfile::tempdir().expect("create fixture");
    let families = [
        ("brown_mushroom_block", "mushroom_brown"),
        ("red_mushroom_block", "mushroom_red"),
        ("mushroom_stem", "mushroom_stem"),
    ];
    let faces = [
        (BlockFace::West, "west", "west"),
        (BlockFace::East, "east", "east"),
        (BlockFace::Down, "down", "bottom"),
        (BlockFace::Up, "up", "top"),
        (BlockFace::North, "north", "north"),
        (BlockFace::South, "south", "south"),
    ];
    let colour = |family: usize, face: usize, bits: u8| {
        let discriminator = 1 + family as u8 * 36 + face as u8 * 2 + u8::from(bits == 15);
        [discriminator, 255 - discriminator, bits, 255]
    };
    let is_static_stem_face = |family: usize, face: BlockFace| {
        family == 2
            && matches!(
                face,
                BlockFace::West | BlockFace::East | BlockFace::North | BlockFace::South
            )
    };

    let mut block_entries = serde_json::Map::new();
    let mut terrain_entries = serde_json::Map::new();
    for (family_index, (block_name, texture_prefix)) in families.iter().enumerate() {
        let mut face_entries = serde_json::Map::new();
        for (face_index, (face, block_face, texture_suffix)) in faces.iter().enumerate() {
            let key = format!("{texture_prefix}_{texture_suffix}");
            face_entries.insert((*block_face).into(), serde_json::Value::String(key.clone()));
            let selected_bits = if is_static_stem_face(family_index, *face) {
                terrain_entries.insert(
                    key,
                    serde_json::json!({
                        "textures": format!(
                            "textures/blocks/{texture_prefix}_{texture_suffix}_static"
                        )
                    }),
                );
                vec![0]
            } else {
                let variants = (0..16)
                    .map(|bits| {
                        serde_json::Value::String(format!(
                            "textures/blocks/{texture_prefix}_{texture_suffix}_{bits}"
                        ))
                    })
                    .collect::<Vec<_>>();
                terrain_entries.insert(key, serde_json::json!({ "textures": variants }));
                (0..16).collect::<Vec<_>>()
            };

            for bits in selected_bits {
                let source = if is_static_stem_face(family_index, *face) {
                    format!("textures/blocks/{texture_prefix}_{texture_suffix}_static")
                } else {
                    format!("textures/blocks/{texture_prefix}_{texture_suffix}_{bits}")
                };
                write_png(
                    directory.path(),
                    &source,
                    TILE_SIZE,
                    TILE_SIZE,
                    &solid(TILE_SIZE, TILE_SIZE, colour(family_index, face_index, bits)),
                );
            }
        }
        block_entries.insert(
            (*block_name).into(),
            serde_json::json!({ "textures": face_entries }),
        );
    }

    let unrelated_variants = (0..16)
        .map(|bits| serde_json::Value::String(format!("textures/blocks/unrelated_{bits}")))
        .collect::<Vec<_>>();
    block_entries.insert(
        "unrelated".into(),
        serde_json::json!({ "textures": "unrelated" }),
    );
    terrain_entries.insert(
        "unrelated".into(),
        serde_json::json!({ "textures": unrelated_variants }),
    );
    write_png(
        directory.path(),
        "textures/blocks/unrelated_0",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [240, 120, 60, 255]),
    );

    write_pack(
        directory.path(),
        &serde_json::Value::Object(block_entries).to_string(),
        &serde_json::json!({ "texture_data": terrain_entries }).to_string(),
        "[]",
    );

    let mut records = Vec::new();
    for (family_index, (block_name, _)) in families.iter().enumerate() {
        for bits in 0_u8..16 {
            let sequential_id = (family_index * 16 + usize::from(bits)) as u32;
            records.push(record(
                sequential_id,
                0x8000_1000 + sequential_id,
                &format!("minecraft:{block_name}"),
                &format!(r#"{{"huge_mushroom_bits":{{"type":"int","value":{bits}}}}}"#),
                BlockFlags::CUBE_GEOMETRY,
            ));
        }
    }
    let fallback_states = [
        "{}",
        "null",
        "not JSON",
        r#"{"huge_mushroom_bits":0}"#,
        r#"{"huge_mushroom_bits":15}"#,
        r#"{"huge_mushroom_bits":-1}"#,
        r#"{"huge_mushroom_bits":16}"#,
        r#"{"huge_mushroom_bits":"15"}"#,
        r#"{"huge_mushroom_bits":{"type":"byte","value":15}}"#,
        r#"{"huge_mushroom_bits":{"type":"int","value":"15"}}"#,
        r#"{"huge_mushroom_bits":{"extra":0,"type":"int","value":15}}"#,
        r#"{"extra":{"type":"byte","value":0},"huge_mushroom_bits":{"type":"int","value":15}}"#,
        r#"{"huge_mushroom_bits":{"type":"int","value":0},"huge_mushroom_bits":{"type":"int","value":15}}"#,
        r#"{"huge_mushroom_bits":{"type":"byte","type":"int","value":15}}"#,
        r#"{"huge_mushroom_bits":{"type":"int","value":0,"value":15}}"#,
    ];
    for state in fallback_states {
        let sequential_id = records.len() as u32;
        records.push(record(
            sequential_id,
            0x8000_1000 + sequential_id,
            "minecraft:brown_mushroom_block",
            state,
            BlockFlags::CUBE_GEOMETRY,
        ));
    }
    let invalid_stem_id = records.len() as u32;
    records.push(record(
        invalid_stem_id,
        0x8000_1000 + invalid_stem_id,
        "minecraft:mushroom_stem",
        "{}",
        BlockFlags::CUBE_GEOMETRY,
    ));
    let unrelated_id = records.len() as u32;
    records.push(record(
        unrelated_id,
        0x8000_1000 + unrelated_id,
        "minecraft:unrelated",
        r#"{"huge_mushroom_bits":15}"#,
        BlockFlags::CUBE_GEOMETRY,
    ));

    let compiled = compile_pack(directory.path(), &records).expect("compile mushroom variants");
    for (family_index, _) in families.iter().enumerate() {
        for bits in 0_u8..16 {
            let sequential_id = family_index * 16 + usize::from(bits);
            for (face_index, (face, _, _)) in faces.iter().enumerate() {
                let material = material_for_face(&compiled, sequential_id, *face);
                let expected_bits = if is_static_stem_face(family_index, *face) {
                    0
                } else {
                    bits
                };
                assert_eq!(
                    mip_pixel(&compiled, 0, material.texture.layer(), 0, 0),
                    colour(family_index, face_index, expected_bits),
                    "wrong {bits} texture for {} {face:?}",
                    families[family_index].0
                );
            }
        }
    }
    let fallback_start = families.len() * 16;
    for (offset, state) in fallback_states.iter().enumerate() {
        let sequential_id = fallback_start + offset;
        assert_eq!(
            compiled.visuals[sequential_id].kind,
            VisualKind::Diagnostic,
            "invalid or absent mushroom selector must fail closed: {state}"
        );
        assert_eq!(
            compiled.visuals[sequential_id].faces, [0; 6],
            "invalid or absent mushroom selector retained faces: {state}"
        );
    }
    assert_eq!(
        compiled.visuals[invalid_stem_id as usize].kind,
        VisualKind::Diagnostic,
        "invalid selector must also fail closed for static mushroom face paths"
    );
    assert_eq!(compiled.visuals[invalid_stem_id as usize].faces, [0; 6]);
    let unrelated = material_for_face(&compiled, unrelated_id as usize, BlockFace::Up);
    assert_eq!(
        mip_pixel(&compiled, 0, unrelated.texture.layer(), 0, 0),
        [240, 120, 60, 255],
        "unrelated terrain arrays must retain variant-zero selection"
    );

    let mut reversed = records.clone();
    reversed.reverse();
    let reversed = compile_pack(directory.path(), &reversed).expect("compile reversed records");
    assert_eq!(
        encode_blob(&compiled).expect("encode mushroom variants"),
        encode_blob(&reversed).expect("encode reversed mushroom variants")
    );
}

#[test]
fn compiler_fails_closed_for_noncanonical_mushroom_variant_counts() {
    let directory = tempfile::tempdir().expect("create fixture");
    let variants = (0..15)
        .map(|bits| format!("textures/blocks/mushroom_brown_top_{bits}"))
        .collect::<Vec<_>>();
    write_pack(
        directory.path(),
        r#"{"brown_mushroom_block":{"textures":"mushroom_brown_top"}}"#,
        &serde_json::json!({
            "texture_data": {
                "mushroom_brown_top": { "textures": variants }
            }
        })
        .to_string(),
        "[]",
    );
    let records = [record(
        0,
        0x8000_2000,
        "minecraft:brown_mushroom_block",
        r#"{"huge_mushroom_bits":{"type":"int","value":14}}"#,
        BlockFlags::CUBE_GEOMETRY,
    )];

    let compiled = compile_pack(directory.path(), &records)
        .expect("a malformed mushroom variant table must fail closed without loading a texture");

    assert_eq!(compiled.visuals[0].kind, VisualKind::Diagnostic);
    assert_eq!(compiled.visuals[0].faces, [0; 6]);
    assert_eq!(compiled.materials.len(), 1);
    assert_eq!(compiled.texture_pages[0].texture.layers, 1);
}

#[test]
fn compiler_real_pinned_pack_preserves_checked_transparent_cubes_with_exact_huge_mushrooms() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let all = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let huge_mushrooms = all
        .iter()
        .filter(|record| HUGE_MUSHROOM_NAMES.contains(&record.name.as_ref()))
        .cloned()
        .collect::<Vec<_>>();
    let legacy_flags_zero = all
        .iter()
        .filter(|record| {
            record.model_family == ModelFamily::Cube && record.flags == BlockFlags::empty()
        })
        .cloned()
        .collect::<Vec<_>>();
    let transparency_family = all
        .iter()
        .filter(|record| {
            record.model_family == ModelFamily::Cube
                && (record.name.ends_with("_stained_glass")
                    || record.name.contains("copper_grate")
                    || record.name.as_ref() == "minecraft:slime")
        })
        .cloned()
        .collect::<Vec<_>>();
    let invisible_bedrock = all
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:invisible_bedrock")
        .expect("canonical invisible bedrock")
        .clone();
    assert_eq!(huge_mushrooms.len(), 48);
    assert_eq!(legacy_flags_zero.len(), 43);
    assert_eq!(transparency_family.len(), 25);

    let non_mushroom_count = legacy_flags_zero.len() + transparency_family.len() + 1;
    let mut records = huge_mushrooms
        .into_iter()
        .chain(legacy_flags_zero)
        .chain(transparency_family)
        .chain(std::iter::once(invisible_bedrock))
        .collect::<Vec<_>>();
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 108_000 + id as u32;
    }

    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned mushrooms");
    for (id, record) in records.iter().enumerate() {
        if id < 48 {
            assert_eq!(compiled.visuals[id].kind, VisualKind::Cube, "{record:?}");
            assert!(
                compiled.visuals[id]
                    .faces
                    .iter()
                    .all(|material| *material != DIAGNOSTIC_MATERIAL),
                "{record:?}"
            );
        } else if ORDINARY_STAINED_GLASS_NAMES
            .binary_search(&record.name.as_ref())
            .is_ok()
            || COPPER_GRATE_NAMES
                .binary_search(&record.name.as_ref())
                .is_ok()
        {
            assert_eq!(
                compiled.visuals[id].kind,
                VisualKind::Model,
                "checked transparent cube became diagnostic: {record:?}"
            );
        } else {
            assert_eq!(
                compiled.visuals[id].kind,
                VisualKind::Diagnostic,
                "excluded record became drawable: {record:?}"
            );
        }
    }
    assert_eq!(records.len() - 48, non_mushroom_count);

    let baseline = encode_blob(&compiled).expect("encode pinned mushrooms");
    records.reverse();
    let reversed = compile_pack(Path::new(&pack), &records).expect("compile reversed mushrooms");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}
