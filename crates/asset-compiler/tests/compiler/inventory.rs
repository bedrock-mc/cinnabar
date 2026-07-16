use super::support::*;

fn generated_huge_mushroom_records() -> Vec<RegistryRecord> {
    let mut records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| HUGE_MUSHROOM_NAMES.contains(&record.name.as_ref()))
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| record.sequential_id);
    records
}

fn generated_stained_glass_cube_records() -> Vec<RegistryRecord> {
    read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| {
        ORDINARY_STAINED_GLASS_NAMES
            .binary_search(&record.name.as_ref())
            .is_ok()
    })
    .collect()
}

fn generated_copper_grate_records() -> Vec<RegistryRecord> {
    read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| {
        COPPER_GRATE_NAMES
            .binary_search(&record.name.as_ref())
            .is_ok()
    })
    .collect()
}

#[test]
fn compiled_checked_in_air_preserves_both_runtime_network_identities() {
    let directory = TempDir::new().unwrap();
    write_pack(
        directory.path(),
        r#"{"format_version":[1,1,0]}"#,
        r#"{"texture_data":{}}"#,
        "[]",
    );
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == "minecraft:air")
    .collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
    let compiled = compile_pack(directory.path(), &records).expect("compile committed air record");
    let runtime = RuntimeAssets::decode(
        &encode_blob(&compiled)
            .expect("encode compiled committed air record")
            .into_vec(),
    )
    .expect("decode compiled committed air record");

    assert_eq!(
        runtime.air_network_id(NetworkIdMode::Sequential),
        Some(13_094)
    );
    assert_eq!(
        runtime.air_network_id(NetworkIdMode::Hashed),
        Some(0xdbf4_4120)
    );
}

#[test]
fn generated_registry_has_exact_copper_grate_inventory() {
    let copper_grates = generated_copper_grate_records();
    assert_eq!(copper_grates.len(), 8);
    assert!(copper_grates.iter().all(|record| {
        record.canonical_state.as_ref() == "{}"
            && record.model_family == ModelFamily::Cube
            && record.contributor_role == ContributorRole::Primary
            && record.flags == BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
    }));
    let mut actual = copper_grates
        .iter()
        .map(|record| record.name.as_ref())
        .collect::<Vec<_>>();
    actual.sort_unstable();
    assert_eq!(actual, COPPER_GRATE_NAMES);
}

#[test]
fn generated_registry_has_exact_chiseled_bookshelf_inventory() {
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let selected = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:chiseled_bookshelf")
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 256);
    let mut seen = [false; 256];
    for record in selected {
        assert_eq!(record.model_family, ModelFamily::ChiseledBookshelf);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert_eq!(
            record.flags,
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
        );
        assert_eq!(record.face_coverage, 0x3f);
        assert_eq!(record.collision_seed.shape_id, 1);
        assert_eq!(record.collision_seed.boxes.len(), 1);
        let books = record
            .model_state
            .get(ModelStateField::Connections)
            .expect("books selector");
        let direction = record
            .model_state
            .get(ModelStateField::Orientation)
            .expect("direction selector");
        let index = (books * 4 + direction) as usize;
        assert_eq!(record.sequential_id, 1605 + index as u32);
        assert!(!seen[index]);
        seen[index] = true;
    }
    assert!(seen.into_iter().all(|present| present));
}

#[test]
fn generated_registry_has_exact_bee_housing_inventory() {
    let records = bee_housing_records();
    assert_eq!(records.len(), 48);
    let selector_mask =
        1 << (ModelStateField::Orientation as u8 - 1) | 1 << (ModelStateField::Growth as u8 - 1);
    let mut seen = HashSet::new();
    for record in records {
        let base = match record.name.as_ref() {
            "minecraft:bee_nest" => 10_395,
            "minecraft:beehive" => 12_495,
            _ => unreachable!(),
        };
        assert_eq!(record.model_family, ModelFamily::Cube);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert_eq!(
            record.flags,
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
        );
        assert_eq!(record.face_coverage, 0x3f);
        assert_eq!(record.collision_seed.shape_id, 1);
        assert_eq!(
            record.collision_seed.confidence,
            CollisionConfidence::CollisionOnly
        );
        assert_eq!(
            record.collision_seed.boxes.as_ref(),
            [CollisionBox {
                max_x: 100_000_000,
                max_y: 100_000_000,
                max_z: 100_000_000,
                ..CollisionBox::default()
            }]
        );
        assert_eq!(record.model_state.mask(), selector_mask);
        let direction = record
            .model_state
            .get(ModelStateField::Orientation)
            .expect("bee direction");
        let honey = record
            .model_state
            .get(ModelStateField::Growth)
            .expect("bee honey level");
        assert!(direction < 4);
        assert!(honey < 6);
        assert_eq!(record.sequential_id, base + honey * 4 + direction);
        assert_eq!(
            record.canonical_state.as_ref(),
            format!(
                r#"{{"direction":{{"type":"int","value":{direction}}},"honey_level":{{"type":"int","value":{honey}}}}}"#
            )
        );
        assert!(seen.insert((record.name, direction, honey)));
    }
    assert_eq!(seen.len(), 48);
}

#[test]
fn generated_registry_has_exact_resin_clump_inventory() {
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let selected = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:resin_clump")
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 64);
    let mut seen = [false; 64];
    for record in selected {
        assert_eq!(record.model_family, ModelFamily::ResinClump);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert_eq!(record.flags, BlockFlags::empty());
        assert_eq!(record.face_coverage, 0);
        assert_eq!(record.collision_seed.shape_id, 0);
        assert_eq!(
            record.collision_seed.confidence,
            CollisionConfidence::CollisionOnly
        );
        assert!(record.collision_seed.boxes.is_empty());
        let mask = record
            .model_state
            .get(ModelStateField::Connections)
            .expect("resin direction mask");
        assert!(mask < 64);
        assert_eq!(
            record.model_state.mask(),
            1 << (ModelStateField::Connections as u8 - 1)
        );
        assert_eq!(record.sequential_id, 2930 + mask);
        assert!(!seen[mask as usize]);
        seen[mask as usize] = true;
    }
    assert!(seen.into_iter().all(|present| present));
}

#[test]
fn generated_registry_has_exact_cactus_inventory() {
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let selected = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:cactus")
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 16);
    let mut seen = [false; 16];
    for record in selected {
        assert_eq!(record.model_family, ModelFamily::Cuboid);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert_eq!(record.flags, BlockFlags::empty());
        assert_eq!(record.face_coverage, 0);
        assert_eq!(record.collision_seed.shape_id, 84);
        assert_eq!(
            record.collision_seed.confidence,
            CollisionConfidence::CollisionOnly
        );
        assert_eq!(
            record.collision_seed.boxes.as_ref(),
            [CollisionBox {
                min_x: 6_250_000,
                min_y: 0,
                min_z: 6_250_000,
                max_x: 93_750_000,
                max_y: 100_000_000,
                max_z: 93_750_000,
            }]
        );
        let age = record
            .model_state
            .get(ModelStateField::Growth)
            .expect("cactus age");
        assert!(age < 16);
        assert_eq!(
            record.model_state.mask(),
            1 << (ModelStateField::Growth as u8 - 1)
        );
        assert_eq!(record.sequential_id, 13_606 + age);
        assert_eq!(
            record.canonical_state.as_ref(),
            format!(r#"{{"age":{{"type":"int","value":{age}}}}}"#)
        );
        assert!(!seen[age as usize]);
        seen[age as usize] = true;
    }
    assert!(seen.into_iter().all(|present| present));
}

#[test]
fn generated_registry_has_exact_cake_inventory() {
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let selected = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:cake")
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 7);
    let min_x = [
        6_250_000, 18_750_000, 31_250_000, 43_750_000, 56_250_000, 68_750_000, 81_250_000,
    ];
    let mut seen = [false; 7];
    for record in selected {
        let bite = record
            .model_state
            .get(ModelStateField::Growth)
            .expect("cake bite counter");
        assert!(bite < 7);
        assert_eq!(record.model_family, ModelFamily::Cuboid);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert_eq!(record.flags, BlockFlags::empty());
        assert_eq!(record.face_coverage, 0);
        assert_eq!(
            record.model_state.mask(),
            1 << (ModelStateField::Growth as u8 - 1)
        );
        assert_eq!(record.sequential_id, 14_055 + bite);
        assert_eq!(record.collision_seed.shape_id, 89 + bite as u16);
        assert_eq!(
            record.collision_seed.confidence,
            CollisionConfidence::CollisionOnly
        );
        assert_eq!(
            record.collision_seed.boxes.as_ref(),
            [CollisionBox {
                min_x: min_x[bite as usize],
                min_y: 0,
                min_z: 6_250_000,
                max_x: 93_750_000,
                max_y: 50_000_000,
                max_z: 93_750_000,
            }]
        );
        assert_eq!(
            record.canonical_state.as_ref(),
            format!(r#"{{"bite_counter":{{"type":"int","value":{bite}}}}}"#)
        );
        assert!(!seen[bite as usize]);
        seen[bite as usize] = true;
    }
    assert!(seen.into_iter().all(|present| present));
}

#[test]
fn generated_registry_has_exact_farmland_inventory() {
    let records = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let selected = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:farmland")
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 8);
    let hashes = [
        360_492_383,
        421_967_206,
        483_442_029,
        544_916_852,
        606_391_675,
        667_866_498,
        729_341_321,
        790_816_144,
    ];
    let mut seen = [false; 8];
    for record in selected {
        let amount = record
            .model_state
            .get(ModelStateField::Growth)
            .expect("farmland moisture");
        assert!(amount < 8);
        assert_eq!(record.model_family, ModelFamily::Cuboid);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert_eq!(record.flags, BlockFlags::empty());
        assert_eq!(record.face_coverage, 0);
        assert_eq!(
            record.model_state.mask(),
            1 << (ModelStateField::Growth as u8 - 1)
        );
        assert_eq!(record.sequential_id, 6_122 + amount);
        assert_eq!(record.network_hash, hashes[amount as usize]);
        assert_eq!(record.collision_seed.shape_id, 43);
        assert_eq!(
            record.collision_seed.confidence,
            CollisionConfidence::CollisionOnly
        );
        assert_eq!(
            record.collision_seed.boxes.as_ref(),
            [CollisionBox {
                min_x: 0,
                min_y: 0,
                min_z: 0,
                max_x: 100_000_000,
                max_y: 93_750_000,
                max_z: 100_000_000,
            }]
        );
        assert_eq!(
            record.canonical_state.as_ref(),
            format!(r#"{{"moisturized_amount":{{"type":"int","value":{amount}}}}}"#)
        );
        assert!(!seen[amount as usize]);
        seen[amount as usize] = true;
    }
    assert!(seen.into_iter().all(|present| present));
}

const SELECTOR_ALIAS_CUBE_NAMES: [&str; 7] = [
    "minecraft:bone_block",
    "minecraft:chiseled_quartz_block",
    "minecraft:hay_block",
    "minecraft:purpur_block",
    "minecraft:quartz_block",
    "minecraft:smooth_quartz",
    "minecraft:tnt",
];

fn selector_alias_cube_records() -> Vec<RegistryRecord> {
    read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry")
    .into_iter()
    .filter(|record| SELECTOR_ALIAS_CUBE_NAMES.contains(&record.name.as_ref()))
    .collect()
}

#[test]
fn generated_registry_has_exact_reviewed_selector_alias_cube_products() {
    let records = selector_alias_cube_records();
    assert_eq!(records.len(), 38);
    let target_ids = [
        2908, 2909, 2910, 2912, 2913, 2914, 2916, 2917, 2918, 5443, 5444, 6466, 6467, 6468, 6470,
        6471, 6472, 6474, 6475, 6476, 7082, 7083, 13113, 14686, 14687, 15345, 15346,
    ];
    assert_eq!(target_ids.len(), 27);
    let actual_ids = records
        .iter()
        .map(|record| record.sequential_id)
        .collect::<HashSet<_>>();
    assert!(target_ids.iter().all(|id| actual_ids.contains(id)));
    for record in records {
        assert_eq!(record.model_family, ModelFamily::Cube);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert_eq!(
            record.flags,
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
        );
        assert_eq!(record.face_coverage, 0x3f);
        assert_eq!(record.collision_seed.shape_id, 1);
        assert_eq!(
            record.collision_seed.confidence,
            CollisionConfidence::CollisionOnly
        );
        assert_eq!(
            record.collision_seed.boxes.as_ref(),
            &[CollisionBox {
                min_x: 0,
                min_y: 0,
                min_z: 0,
                max_x: 100_000_000,
                max_y: 100_000_000,
                max_z: 100_000_000,
            }]
        );
        if record.name.as_ref() == "minecraft:tnt" {
            assert_eq!(record.model_state.mask(), 0);
        } else {
            assert_eq!(
                record.model_state.mask(),
                1 << (ModelStateField::Orientation as u8 - 1)
            );
            assert!(matches!(
                record.model_state.get(ModelStateField::Orientation),
                Some(0..=2)
            ));
        }
    }
}

fn write_selector_alias_cube_pack(root: &Path, hay_route: &str) {
    write_pack(
        root,
        &format!(
            r#"{{
                "bone_block":{{"textures":{{"down":"bone_block_top","side":"bone_block_side","up":"bone_block_top"}}}},
                "chiseled_quartz_block":{{"textures":{{"down":"chiseled_quartz_block_top","side":"chiseled_quartz_block_side","up":"chiseled_quartz_block_top"}}}},
                "hay_block":{{"textures":{hay_route}}},
                "purpur_block":{{"textures":"flattened_purpur_block"}},
                "quartz_block":{{"textures":{{"down":"flattened_quartz_block_top","side":"flattened_quartz_block_side","up":"flattened_quartz_block_top"}}}},
                "smooth_quartz":{{"textures":"smooth_quartz"}},
                "tnt":{{"textures":{{"down":"flattened_tnt_bottom","side":"flattened_tnt_side","up":"flattened_tnt_top"}}}}
            }}"#
        ),
        r#"{"texture_data":{
            "bone_block_top":{"textures":"textures/blocks/bone_block_top"},
            "bone_block_side":{"textures":"textures/blocks/bone_block_side"},
            "chiseled_quartz_block_top":{"textures":"textures/blocks/quartz_block_chiseled_top"},
            "chiseled_quartz_block_side":{"textures":"textures/blocks/quartz_block_chiseled"},
            "hayblock_top":{"textures":"textures/blocks/hay_block_top"},
            "hayblock_side":{"textures":"textures/blocks/hay_block_side"},
            "hay_alias":{"textures":"textures/blocks/hay_block_side"},
            "flattened_purpur_block":{"textures":"textures/blocks/purpur_block"},
            "flattened_quartz_block_top":{"textures":"textures/blocks/quartz_block_top"},
            "flattened_quartz_block_side":{"textures":"textures/blocks/quartz_block_side"},
            "smooth_quartz":{"textures":"textures/blocks/quartz_block_bottom"},
            "flattened_tnt_bottom":{"textures":"textures/blocks/tnt_bottom"},
            "flattened_tnt_side":{"textures":"textures/blocks/tnt_side"},
            "flattened_tnt_top":{"textures":"textures/blocks/tnt_top"}
        }}"#,
        "[]",
    );
    for (index, key) in [
        "bone_block_top",
        "bone_block_side",
        "quartz_block_chiseled_top",
        "quartz_block_chiseled",
        "hay_block_top",
        "hay_block_side",
        "purpur_block",
        "quartz_block_top",
        "quartz_block_side",
        "quartz_block_bottom",
        "tnt_bottom",
        "tnt_side",
        "tnt_top",
    ]
    .into_iter()
    .enumerate()
    {
        write_png(
            root,
            &format!("textures/blocks/{key}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [index as u8 + 1, 40, 80, 255]),
        );
    }
}

fn remapped_selector_alias_cube_records() -> Vec<RegistryRecord> {
    selector_alias_cube_records()
}

#[test]
fn compiler_selector_alias_cube_admission_rejects_extra_state_properties_atomically() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_selector_alias_cube_pack(
        directory.path(),
        r#"{"down":"hayblock_top","side":"hayblock_side","up":"hayblock_top"}"#,
    );
    let mut records = remapped_selector_alias_cube_records();
    let hay = records
        .iter_mut()
        .find(|record| record.name.as_ref() == "minecraft:hay_block")
        .expect("hay record");
    let mut state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&hay.canonical_state)
            .expect("canonical hay state");
    state.insert(
        "extra".into(),
        serde_json::json!({"type": "int", "value": 0}),
    );
    hay.canonical_state = serde_json::to_string(&state).unwrap().into();
    let compiled = compile_pack(directory.path(), &records).expect("compile malformed inventory");
    assert!(records.iter().all(|record| {
        compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
    }));
}

#[test]
fn compiler_selector_alias_cube_pack_route_rejects_descriptor_aliases() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_selector_alias_cube_pack(directory.path(), r#""hay_alias""#);
    let records = remapped_selector_alias_cube_records();
    let compiled = compile_pack(directory.path(), &records).expect("compile aliased pack");
    for record in &records {
        let kind = compiled.visuals[record.sequential_id as usize].kind;
        if record.name.as_ref() == "minecraft:hay_block" {
            assert_eq!(kind, VisualKind::Diagnostic);
        } else {
            assert_eq!(kind, VisualKind::Cube, "{record:?}");
        }
    }
}

#[test]
fn compiler_selector_alias_cube_pack_route_rejects_aliases_variants_tint_flipbooks_and_alpha() {
    for malformed in ["path_alias", "variants", "tint", "flipbook", "alpha"] {
        let directory = tempfile::tempdir().expect("create fixture");
        write_selector_alias_cube_pack(
            directory.path(),
            r#"{"down":"hayblock_top","side":"hayblock_side","up":"hayblock_top"}"#,
        );
        match malformed {
            "path_alias" | "variants" | "tint" => {
                let path = directory.path().join("textures/terrain_texture.json");
                let terrain = fs::read_to_string(&path).unwrap();
                let replacement = match malformed {
                    "path_alias" => {
                        r#""hayblock_side":{"textures":"textures/blocks/bone_block_side"}"#
                    }
                    "variants" => {
                        r#""hayblock_side":{"textures":["textures/blocks/hay_block_side"]}"#
                    }
                    "tint" => {
                        r##""hayblock_side":{"textures":{"path":"textures/blocks/hay_block_side","overlay_color":"#ffffff"}}"##
                    }
                    _ => unreachable!(),
                };
                fs::write(
                    path,
                    terrain.replace(
                        r#""hayblock_side":{"textures":"textures/blocks/hay_block_side"}"#,
                        replacement,
                    ),
                )
                .unwrap();
            }
            "flipbook" => fs::write(
                directory.path().join("textures/flipbook_textures.json"),
                r#"[{"flipbook_texture":"textures/blocks/hay_block_side","atlas_tile":"hayblock_side"}]"#,
            )
            .unwrap(),
            "alpha" => write_png(
                directory.path(),
                "textures/blocks/hay_block_side",
                TILE_SIZE,
                TILE_SIZE,
                &solid(TILE_SIZE, TILE_SIZE, [10, 20, 30, 127]),
            ),
            _ => unreachable!(),
        }
        let records = remapped_selector_alias_cube_records();
        let compiled = compile_pack(directory.path(), &records).expect("compile malformed route");
        for record in &records {
            let kind = compiled.visuals[record.sequential_id as usize].kind;
            if record.name.as_ref() == "minecraft:hay_block" {
                assert_eq!(kind, VisualKind::Diagnostic, "{malformed}: {record:?}");
            } else {
                assert_eq!(kind, VisualKind::Cube, "{malformed}: {record:?}");
            }
        }
    }
}

#[test]
fn compiler_emits_exact_selector_alias_cube_faces_aliases_and_uv_rotations() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_selector_alias_cube_pack(
        directory.path(),
        r#"{"down":"hayblock_top","side":"hayblock_side","up":"hayblock_top"}"#,
    );
    let records = remapped_selector_alias_cube_records();
    let compiled = compile_pack(directory.path(), &records).expect("compile exact products");

    assert!(compiled.model_templates.is_empty());
    assert!(compiled.model_quads.is_empty());
    assert!(compiled.animations.is_empty());
    assert!(compiled.animation_frames.is_empty());
    for record in &records {
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Cube, "{record:?}");
        assert_eq!(visual.model_template, assets::NO_MODEL_TEMPLATE);
        for material in visual.faces {
            assert_ne!(material, DIAGNOSTIC_MATERIAL);
            let material = compiled.materials[material as usize];
            assert!(matches!(material.flags, 0 | MATERIAL_FLAG_ROTATE_UV));
            assert_eq!(material.animation, assets::NO_ANIMATION);
        }
    }

    for name in [
        "minecraft:bone_block",
        "minecraft:chiseled_quartz_block",
        "minecraft:hay_block",
        "minecraft:quartz_block",
    ] {
        let by_axis = |orientation| {
            records
                .iter()
                .find(|record| {
                    record.name.as_ref() == name
                        && record.model_state.get(ModelStateField::Orientation) == Some(orientation)
                })
                .expect("axis record")
        };
        let y = compiled.visuals[by_axis(1).sequential_id as usize];
        let x = compiled.visuals[by_axis(0).sequential_id as usize];
        let z = compiled.visuals[by_axis(2).sequential_id as usize];
        let y_cap = material_for_face(
            &compiled,
            by_axis(1).sequential_id as usize,
            BlockFace::Down,
        );
        let y_side = material_for_face(
            &compiled,
            by_axis(1).sequential_id as usize,
            BlockFace::North,
        );
        for face in [BlockFace::Down, BlockFace::Up] {
            assert_eq!(compiled.materials[y.faces[face as usize] as usize].flags, 0);
        }
        for face in [BlockFace::West, BlockFace::East] {
            let material = compiled.materials[x.faces[face as usize] as usize];
            assert_eq!(material.texture.layer(), y_cap.texture.layer());
            assert_eq!(material.flags, 0);
        }
        for face in [
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::North,
            BlockFace::South,
        ] {
            let material = compiled.materials[x.faces[face as usize] as usize];
            assert_eq!(material.texture.layer(), y_side.texture.layer());
            assert_eq!(material.flags, MATERIAL_FLAG_ROTATE_UV);
        }
        for face in [BlockFace::North, BlockFace::South] {
            let material = compiled.materials[z.faces[face as usize] as usize];
            assert_eq!(material.texture.layer(), y_cap.texture.layer());
            assert_eq!(material.flags, 0);
        }
        for face in [
            BlockFace::West,
            BlockFace::East,
            BlockFace::Down,
            BlockFace::Up,
        ] {
            let material = compiled.materials[z.faces[face as usize] as usize];
            assert_eq!(material.texture.layer(), y_side.texture.layer());
            assert_eq!(material.flags, MATERIAL_FLAG_ROTATE_UV);
        }
    }

    for name in ["minecraft:hay_block", "minecraft:bone_block"] {
        for orientation in 0..=2 {
            let aliases = records
                .iter()
                .filter(|record| {
                    record.name.as_ref() == name
                        && record.model_state.get(ModelStateField::Orientation) == Some(orientation)
                })
                .map(|record| compiled.visuals[record.sequential_id as usize].faces)
                .collect::<Vec<_>>();
            assert_eq!(aliases.len(), 4);
            assert!(aliases.windows(2).all(|pair| pair[0] == pair[1]));
        }
    }
    let tnt = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:tnt")
        .map(|record| compiled.visuals[record.sequential_id as usize].faces)
        .collect::<Vec<_>>();
    assert_eq!(tnt.len(), 2);
    assert_eq!(tnt[0], tnt[1]);

    let baseline = encode_blob(&compiled).expect("encode exact selector aliases");
    let mut reversed = records.clone();
    reversed.reverse();
    let reversed = compile_pack(directory.path(), &reversed).expect("compile reversed records");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

#[test]
#[ignore = "requires PINNED_VANILLA_PACK pointing at the ignored vanilla resource pack"]
fn compiler_real_pinned_pack_admits_all_exact_selector_alias_cube_records() {
    let pack = std::env::var_os("PINNED_VANILLA_PACK")
        .map(PathBuf::from)
        .expect("set PINNED_VANILLA_PACK");
    let records = remapped_selector_alias_cube_records();
    let first = compile_pack(&pack, &records).expect("compile pinned selector aliases");
    assert!(records.iter().all(|record| {
        let visual = first.visuals[record.sequential_id as usize];
        visual.kind == VisualKind::Cube
            && visual.model_template == assets::NO_MODEL_TEMPLATE
            && visual
                .faces
                .iter()
                .all(|material| *material != DIAGNOSTIC_MATERIAL)
    }));
    assert!(first.model_templates.is_empty());
    assert!(first.model_quads.is_empty());
    assert!(first.animations.is_empty());
    let baseline = encode_blob(&first).unwrap();
    let second = compile_pack(&pack, &records).expect("compile pinned selector aliases twice");
    assert_eq!(encode_blob(&second).unwrap(), baseline);
    let mut reversed = records;
    reversed.reverse();
    let reversed = compile_pack(&pack, &reversed).expect("compile reversed pinned aliases");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

#[test]
fn generated_registry_has_exact_stained_glass_cube_inventory() {
    let ordinary_stained_glass = generated_stained_glass_cube_records();
    assert_eq!(ordinary_stained_glass.len(), 16);
    assert!(ordinary_stained_glass.iter().all(|record| {
        record.canonical_state.as_ref() == "{}"
            && record.model_family == ModelFamily::Cube
            && record.contributor_role == ContributorRole::Primary
    }));
    let mut actual = ordinary_stained_glass
        .iter()
        .map(|record| record.name.as_ref())
        .collect::<Vec<_>>();
    actual.sort_unstable();
    assert_eq!(actual, ORDINARY_STAINED_GLASS_NAMES);
}

#[test]
fn generated_registry_has_exact_canonical_huge_mushroom_inventory() {
    let records = generated_huge_mushroom_records();
    assert_eq!(records.len(), 48);

    for name in HUGE_MUSHROOM_NAMES {
        let selected = records
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 16, "{name} record count");
        let mut bits = [false; 16];
        for record in selected {
            assert_eq!(record.model_family, ModelFamily::Cube, "{name}");
            assert!(
                record.flags.contains(BlockFlags::CUBE_GEOMETRY),
                "{name} {}",
                record.canonical_state
            );
            let state = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
                &record.canonical_state,
            )
            .expect("canonical huge-mushroom state");
            assert_eq!(state.len(), 1, "{}", record.canonical_state);
            let selector = state["huge_mushroom_bits"]
                .as_object()
                .expect("tagged huge-mushroom selector");
            assert_eq!(selector.len(), 2, "{}", record.canonical_state);
            assert_eq!(selector["type"], "int", "{}", record.canonical_state);
            let value = selector["value"]
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .expect("nonnegative integer huge-mushroom selector");
            assert!(value < bits.len(), "{}", record.canonical_state);
            assert!(!bits[value], "duplicate {name} selector {value}");
            bits[value] = true;
        }
        assert!(bits.into_iter().all(|present| present), "{name} coverage");
    }
}

#[test]
fn flowerbed_generated_registry_has_exact_canonical_state_matrix() {
    let bytes = if let Ok(revision) = std::env::var("FLOWERBED_REGISTRY_GIT_REV") {
        let output = Command::new("git")
            .args([
                "show",
                &format!("{revision}:crates/assets/data/block-registry-v1001.bin"),
            ])
            .output()
            .expect("read requested registry revision");
        assert!(output.status.success(), "git show failed: {output:?}");
        output.stdout
    } else {
        include_bytes!("../../../assets/data/block-registry-v1001.bin").to_vec()
    };
    let records = read_registry(&bytes).expect("decode committed generated registry");

    for name in ["minecraft:wildflowers", "minecraft:pink_petals"] {
        let selected = records
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 32, "{name} record count");

        let mut growths = [false; 8];
        let mut orientations = [false; 4];
        let mut selectors = HashSet::with_capacity(32);
        let mut canonical_states = HashSet::with_capacity(32);
        for record in selected {
            assert_eq!(record.model_family as u8, 31, "{name} raw family");
            assert_ne!(record.model_family, ModelFamily::Cross, "{name} is Cross");
            assert_ne!(
                record.model_family,
                ModelFamily::Unknown,
                "{name} is Unknown"
            );
            let growth = record
                .model_state
                .get(ModelStateField::Growth)
                .expect("flowerbed growth") as usize;
            let orientation = record
                .model_state
                .get(ModelStateField::Orientation)
                .expect("flowerbed orientation") as usize;
            assert!(
                growth < growths.len(),
                "{name} growth {growth} out of range"
            );
            assert!(
                orientation < orientations.len(),
                "{name} orientation {orientation} out of range"
            );
            growths[growth] = true;
            orientations[orientation] = true;
            assert!(
                selectors.insert((growth, orientation)),
                "{name} duplicate growth/orientation pair {growth}/{orientation}"
            );
            assert!(
                canonical_states.insert(record.canonical_state.as_ref()),
                "{name} duplicate canonical state"
            );
        }

        assert!(
            growths.into_iter().all(|present| present),
            "{name} growth coverage"
        );
        assert!(
            orientations.into_iter().all(|present| present),
            "{name} orientation coverage"
        );
        assert_eq!(selectors.len(), 32, "{name} selector uniqueness");
        assert_eq!(
            canonical_states.len(),
            32,
            "{name} canonical-state uniqueness"
        );
    }
}
