use super::support::*;

#[test]
fn compiler_marks_only_leaf_faces_as_alpha_cutout() {
    let (_directory, resource_pack, records) = leaf_material_fixture();
    let compiled = compile_pack(&resource_pack, &records).expect("compile leaf materials");

    assert_eq!(MATERIAL_FLAG_UV_MASK, 0x0f);
    assert_eq!(MATERIAL_FLAG_ALPHA_CUTOUT, 0x100);
    assert_eq!(MATERIAL_FLAG_LIQUID_DEPTH_WRITE, 0x800);
    assert_eq!(MATERIAL_FLAGS_MASK, 0xfff);
    assert_eq!(std::mem::size_of::<Material>(), 12);
    let opaque_id = compiled.visuals[0].faces[BlockFace::Up as usize];
    let opaque = compiled.materials[opaque_id as usize];
    assert_eq!(opaque.flags & MATERIAL_FLAG_ALPHA_CUTOUT, 0);
    for leaf in 1..=3 {
        for face in BlockFace::ALL {
            let material_id = compiled.visuals[leaf].faces[face as usize];
            assert_ne!(material_id, DIAGNOSTIC_MATERIAL);
            let material = compiled.materials[material_id as usize];
            assert_eq!(
                material.flags & MATERIAL_FLAG_ALPHA_CUTOUT,
                MATERIAL_FLAG_ALPHA_CUTOUT
            );
            assert_eq!(material.flags & MATERIAL_FLAG_UV_MASK, 0);
        }
    }
    let cherry_id = compiled.visuals[1].faces[BlockFace::Up as usize];
    let cherry = compiled.materials[cherry_id as usize];
    assert_eq!(
        opaque.texture.layer(),
        cherry.texture.layer(),
        "pixels must remain deduplicated"
    );
    assert_ne!(
        opaque_id, cherry_id,
        "opaque and cutout descriptors must differ"
    );
    assert!(
        compiled
            .materials
            .iter()
            .all(|material| material.flags & !MATERIAL_FLAGS_MASK == 0)
    );

    let baseline = encode_blob(&compiled).expect("encode cutout baseline");
    for iteration in 0..100_u64 {
        let shuffled = shuffled_records(&records, 0x9e37_79b9 ^ iteration);
        let actual = compile_pack(&resource_pack, &shuffled).expect("compile shuffled cutout pack");
        assert_eq!(encode_blob(&actual).expect("encode shuffle"), baseline);
    }
}

#[test]
fn compiler_assigns_generic_birch_evergreen_and_self_colored_leaf_flags() {
    let directory = tempfile::tempdir().expect("create leaf class fixture");
    write_pack(
        directory.path(),
        r#"{
            "oak_leaves":{"textures":"leaves"},
            "birch_leaves":{"textures":"leaves"},
            "spruce_leaves":{"textures":"leaves"},
            "cherry_leaves":{"textures":"leaves"}
        }"#,
        r#"{"texture_data":{"leaves":{"textures":"textures/blocks/leaves"}}}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/leaves",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [80, 160, 40, 255]),
    );
    let leaf = BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL;
    let records = [
        record(0, 100, "minecraft:oak_leaves", "{}", leaf),
        record(1, 101, "minecraft:birch_leaves", "{}", leaf),
        record(2, 102, "minecraft:spruce_leaves", "{}", leaf),
        record(3, 103, "minecraft:cherry_leaves", "{}", leaf),
    ];
    let compiled = compile_pack(directory.path(), &records).expect("compile leaf classes");
    let flags = |record_id| material_for_face(&compiled, record_id, BlockFace::Up).flags;

    assert_eq!(
        flags(0),
        MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT
    );
    assert_eq!(
        flags(1),
        MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_BIRCH_FOLIAGE
    );
    assert_eq!(
        flags(2),
        MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_EVERGREEN_FOLIAGE
    );
    assert_eq!(flags(3), MATERIAL_FLAG_ALPHA_CUTOUT);
    assert_eq!(flags(3) & MATERIAL_FLAG_TINT_MASK, 0);
    assert_eq!(flags(3) & MATERIAL_FLAG_FOLIAGE_CLASS_MASK, 0);
}

#[test]
fn assetc_summary_reports_deterministic_cutout_material_count() {
    let (directory, resource_pack, records) = leaf_material_fixture();
    let registry = directory.path().join("registry.bin");
    let light_registry = directory.path().join("light-registry.bin");
    let biome_registry = directory.path().join("biome-registry.bin");
    let output_blob = directory.path().join("vanilla-v1001.mcbea");
    let registry_fixture = registry_bytes(&records);
    fs::write(&registry, &registry_fixture).expect("write registry fixture");
    fs::write(
        &light_registry,
        light_registry_bytes(&registry_fixture, records.len()),
    )
    .expect("write light registry fixture");
    fs::write(&biome_registry, biome_registry_bytes(0, "minecraft:plains"))
        .expect("write biome registry fixture");
    write_biome_fixture(&resource_pack);
    let output = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .args(["compile", "--pack"])
        .arg(&resource_pack)
        .arg("--registry")
        .arg(&registry)
        .arg("--light-registry")
        .arg(&light_registry)
        .arg("--biome-registry")
        .arg(&biome_registry)
        .arg("--out")
        .arg(&output_blob)
        .output()
        .expect("run assetc compile");
    assert!(
        output.status.success(),
        "assetc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 summary"),
        format!(
            "compiled 4 visuals, 5 materials (3 alpha cutout), 4 texture layers, and 1 biome rules to {}\n",
            output_blob.display()
        )
    );
}

#[test]
fn cutout_mips_preserve_each_layer_coverage_without_cross_layer_bleed() {
    let directory = tempfile::tempdir().expect("create coverage fixture");
    write_pack(
        directory.path(),
        r#"{
            "cherry_leaves":{"textures":"red"},
            "azalea_leaves":{"textures":"blue"},
            "azalea_leaves_flowered":{"textures":"green"}
        }"#,
        r#"{"texture_data":{
            "red":{"textures":"textures/blocks/a_red"},
            "blue":{"textures":"textures/blocks/b_blue"},
            "green":{"textures":"textures/blocks/c_green"}
        }}"#,
        "[]",
    );
    let red = cutout_pattern([255, 0, 0], 78);
    let blue = cutout_pattern([0, 0, 255], 181);
    let green = aligned_half_pattern([0, 255, 0]);
    for (path, pixels) in [
        ("textures/blocks/a_red", &red),
        ("textures/blocks/b_blue", &blue),
        ("textures/blocks/c_green", &green),
    ] {
        write_png(directory.path(), path, TILE_SIZE, TILE_SIZE, pixels);
    }
    let flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL;
    let records = [
        record(0, 200, "minecraft:cherry_leaves", "{}", flags),
        record(1, 201, "minecraft:azalea_leaves", "{}", flags),
        record(2, 202, "minecraft:azalea_leaves_flowered", "{}", flags),
    ];
    let compiled = compile_pack(directory.path(), &records).expect("compile coverage fixture");
    let red_layer = material_for_face(&compiled, 0, BlockFace::Up)
        .texture
        .layer();
    let blue_layer = material_for_face(&compiled, 1, BlockFace::Up)
        .texture
        .layer();
    assert_eq!(
        blue_layer,
        red_layer + 1,
        "red and blue must be adjacent layers"
    );

    let mut correction_exercised = false;
    for (record_id, base, colour, no_tie) in [
        (0, red.as_slice(), [255, 0, 0], false),
        (1, blue.as_slice(), [0, 0, 255], false),
        (2, green.as_slice(), [0, 255, 0], true),
    ] {
        let layer = material_for_face(&compiled, record_id, BlockFace::Up)
            .texture
            .layer();
        let raw_mips = reference_raw_mips(base, colour);
        let base_survivors = alpha_survivors(&raw_mips[0]);
        for (mip_index, mip) in compiled.texture_pages[0].texture.mips.iter().enumerate() {
            let actual = mip_layer(&compiled, mip_index, layer);
            let raw = &raw_mips[mip_index];
            let pixels = usize::try_from(mip.size * mip.size).expect("small mip");
            let target = (base_survivors * pixels + 128) / 256;
            let expected_scale = reference_nearest_scale(raw, target);
            assert_eq!(
                alpha_survivors(actual),
                reference_nearest_survivors(raw, target),
                "coverage mismatch for record {record_id} mip {mip_index}"
            );
            for (actual, raw) in actual.chunks_exact(4).zip(raw.chunks_exact(4)) {
                assert_eq!(&actual[..3], &raw[..3], "coverage scaling changed RGB");
                let expected_alpha = if mip_index == 0 {
                    raw[3]
                } else {
                    ((u32::from(raw[3]) * expected_scale + 0x8000) >> 16).min(255) as u8
                };
                assert_eq!(
                    actual[3], expected_alpha,
                    "coverage scaling chose the wrong tie-break scale"
                );
            }
            if record_id == 0 {
                assert!(actual.chunks_exact(4).all(|pixel| pixel[2] == 0));
            } else if record_id == 1 {
                assert!(actual.chunks_exact(4).all(|pixel| pixel[0] == 0));
            }
            if no_tie {
                assert_eq!(alpha_survivors(raw), target, "no-tie fixture missed target");
                assert_eq!(alpha_survivors(actual), target);
            } else if mip_index > 0 && alpha_survivors(raw) != target {
                correction_exercised = true;
            }
        }
    }
    assert!(
        correction_exercised,
        "patterns did not exercise coverage correction"
    );
}

#[test]
fn compiler_rejects_invalid_block_flag_semantics() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{"format_version":[1,1,0]}"#,
        r#"{"texture_data":{}}"#,
        "[]",
    );

    for invalid in [
        BlockFlags::from_bits_retain(0x10),
        BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY,
        BlockFlags::AIR | BlockFlags::OCCLUDES_FULL_FACE,
        BlockFlags::LEAF_MODEL,
        BlockFlags::LEAF_MODEL | BlockFlags::OCCLUDES_FULL_FACE,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE | BlockFlags::LEAF_MODEL,
    ] {
        let records = [record(0, 1, "minecraft:test", "{}", invalid)];
        assert!(matches!(
            compile_pack(directory.path(), &records),
            Err(AssetError::InvalidCompiledAssets { .. })
        ));
    }
}

#[test]
fn compiler_deduplicates_pixels_without_conflating_uv_flags() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "same_a": {"textures": "red_a"},
            "same_b": {"textures": "red_b"},
            "pillar": {"textures": {"up": "red_a", "down": "red_a", "side": "red_a"}},
            "blue": {"textures": "blue"}
        }"#,
        r#"{"texture_data": {
            "red_a": {"textures": "textures/blocks/a_red"},
            "red_b": {"textures": "textures/blocks/b_red_copy"},
            "blue": {"textures": "textures/blocks/c_blue"}
        }}"#,
        "[]",
    );
    let red = solid(TILE_SIZE, TILE_SIZE, [255, 0, 0, 255]);
    write_png(
        directory.path(),
        "textures/blocks/a_red",
        TILE_SIZE,
        TILE_SIZE,
        &red,
    );
    write_png(
        directory.path(),
        "textures/blocks/b_red_copy",
        TILE_SIZE,
        TILE_SIZE,
        &red,
    );
    write_png(
        directory.path(),
        "textures/blocks/c_blue",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [0, 0, 255, 255]),
    );
    let records = [
        record(0, 100, "minecraft:same_a", "{}", BlockFlags::CUBE_GEOMETRY),
        record(1, 101, "minecraft:same_b", "{}", BlockFlags::CUBE_GEOMETRY),
        record(
            2,
            102,
            "minecraft:pillar",
            r#"{"pillar_axis":"x"}"#,
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(3, 103, "minecraft:blue", "{}", BlockFlags::CUBE_GEOMETRY),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile synthetic pack");

    assert_eq!(
        compiled.texture_pages[0].texture.layers, 3,
        "diagnostic + red + blue"
    );
    assert_eq!(
        compiled.materials.len(),
        4,
        "diagnostic + three descriptors"
    );
    assert_eq!(compiled.visuals[0].faces, compiled.visuals[1].faces);

    let red_plain = material_for_face(&compiled, 0, BlockFace::Up);
    let red_rotated = material_for_face(&compiled, 2, BlockFace::North);
    let blue = material_for_face(&compiled, 3, BlockFace::Up);
    assert_eq!(red_plain.texture.layer(), red_rotated.texture.layer());
    assert_eq!(red_plain.flags, 0);
    assert_eq!(red_rotated.flags, MATERIAL_FLAG_ROTATE_UV);
    assert_ne!(red_plain, red_rotated);
    assert_ne!(red_plain.texture.layer(), blue.texture.layer());
}

#[test]
fn compiler_builds_diagnostic_and_layer_isolated_linear_mips() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "red": {"textures": "red"},
            "blue": {"textures": "blue"}
        }"#,
        r#"{"texture_data": {
            "red": {"textures": "textures/blocks/a_red"},
            "blue": {"textures": "textures/blocks/b_blue"}
        }}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/a_red",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [255, 0, 0, 255]),
    );
    write_png(
        directory.path(),
        "textures/blocks/b_blue",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [0, 0, 255, 255]),
    );
    let records = [
        record(0, 200, "minecraft:red", "{}", BlockFlags::CUBE_GEOMETRY),
        record(1, 201, "minecraft:blue", "{}", BlockFlags::CUBE_GEOMETRY),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile synthetic pack");

    assert_eq!(
        compiled.materials[0],
        Material {
            texture: assets::TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: assets::NO_ANIMATION
        }
    );
    assert_eq!(mip_pixel(&compiled, 0, 0, 0, 0), [255, 0, 255, 255]);
    assert_eq!(mip_pixel(&compiled, 0, 0, 1, 0), [0, 0, 0, 255]);
    assert_eq!(compiled.texture_pages[0].texture.mips.len(), 5);
    assert_eq!(
        compiled.texture_pages[0]
            .texture
            .mips
            .iter()
            .map(|mip| mip.size)
            .collect::<Vec<_>>(),
        [16, 8, 4, 2, 1]
    );

    let red_layer = material_for_face(&compiled, 0, BlockFace::Up)
        .texture
        .layer();
    let blue_layer = material_for_face(&compiled, 1, BlockFace::Up)
        .texture
        .layer();
    for (mip_index, mip) in compiled.texture_pages[0].texture.mips.iter().enumerate() {
        for y in 0..mip.size as usize {
            for x in 0..mip.size as usize {
                assert_eq!(
                    mip_pixel(&compiled, mip_index, red_layer, x, y),
                    [255, 0, 0, 255]
                );
                assert_eq!(
                    mip_pixel(&compiled, mip_index, blue_layer, x, y),
                    [0, 0, 255, 255]
                );
            }
        }
    }
}

#[test]
fn compiler_supports_vanilla_glass_and_fails_closed_for_arbitrary_tinted_full_cubes() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "stone": {"textures": "stone"},
            "glass": {"textures": "glass"},
            "tinted_cube": {"textures": "tinted_cube"},
            "grass": {"textures": {
                "down": "grass_bottom", "side": "grass_side", "up": "grass_top"
            }}
        }"#,
        r##"{"texture_data": {
            "stone": {"textures": "textures/blocks/stone"},
            "glass": {"textures": "textures/blocks/glass"},
            "tinted_cube": {"textures": {
                "path": "textures/blocks/tinted_cube", "overlay_color": "#79c05a"
            }},
            "grass_bottom": {"textures": "textures/blocks/grass_bottom"},
            "grass_side": {"textures": "textures/blocks/grass_side"},
            "grass_top": {"textures": "textures/blocks/grass_top"}
        }}"##,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/stone",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [100, 100, 100, 255]),
    );
    let mut glass = solid(TILE_SIZE, TILE_SIZE, [210, 230, 255, 0]);
    glass[0] = [210, 230, 255, 255];
    write_png(
        directory.path(),
        "textures/blocks/glass",
        TILE_SIZE,
        TILE_SIZE,
        &glass,
    );
    for path in [
        "textures/blocks/tinted_cube",
        "textures/blocks/grass_bottom",
        "textures/blocks/grass_side",
        "textures/blocks/grass_top",
    ] {
        write_png(
            directory.path(),
            path,
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, [80, 160, 60, 255]),
        );
    }
    let records = [
        record(0, 300, "minecraft:stone", "{}", BlockFlags::CUBE_GEOMETRY),
        record(1, 301, "minecraft:glass", "{}", BlockFlags::CUBE_GEOMETRY),
        record(
            2,
            302,
            "minecraft:tinted_cube",
            "{}",
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(
            3,
            303,
            "minecraft:grass_block",
            "{}",
            BlockFlags::CUBE_GEOMETRY,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile synthetic pack");

    assert!(
        compiled.visuals[0]
            .faces
            .into_iter()
            .all(|material| material != 0)
    );
    assert_eq!(compiled.visuals[1].kind, VisualKind::Cube);
    assert!(
        compiled.visuals[1]
            .faces
            .into_iter()
            .all(|material| material != DIAGNOSTIC_MATERIAL)
    );
    for face in BlockFace::ALL {
        assert_eq!(
            material_for_face(&compiled, 1, face).flags,
            MATERIAL_FLAG_ALPHA_CUTOUT
        );
    }
    let artifact = RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
    let artifact_glass = artifact.resolve(NetworkIdMode::Sequential, 1);
    assert_eq!(artifact_glass.kind(), VisualKind::Cube);
    assert!(artifact_glass.flags().contains(BlockFlags::CUBE_GEOMETRY));
    for face in BlockFace::ALL {
        assert_eq!(
            artifact
                .material(artifact_glass.face(face).material_id())
                .flags,
            MATERIAL_FLAG_ALPHA_CUTOUT
        );
    }
    assert_eq!(
        compiled.visuals[2].faces, [DIAGNOSTIC_MATERIAL; 6],
        "arbitrary tinted full cube must remain fail closed"
    );
    let grass = compiled.visuals[3];
    assert!(grass.faces.into_iter().all(|material| material != 0));
    assert_eq!(material_for_face(&compiled, 3, BlockFace::Down).flags, 0);
    assert_eq!(
        material_for_face(&compiled, 3, BlockFace::Up).flags,
        MATERIAL_FLAG_GRASS_TINT
    );
    for face in [
        BlockFace::West,
        BlockFace::East,
        BlockFace::North,
        BlockFace::South,
    ] {
        assert_eq!(
            material_for_face(&compiled, 3, face).flags,
            MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_OVERLAY_MASK
        );
    }
}

#[test]
fn alpha_support_is_scoped_to_each_descriptor_when_a_texture_path_is_shared() {
    let directory = tempfile::tempdir().expect("create shared alpha fixture");
    write_pack(
        directory.path(),
        r#"{
            "arbitrary_tinted_cube": {"textures": "shared_stained_glass"},
            "red_stained_glass_pane": {"textures": "shared_stained_glass"}
        }"#,
        r#"{"texture_data": {
            "shared_stained_glass": {"textures": "textures/blocks/shared_stained_glass"}
        }}"#,
        "[]",
    );
    let mut pixels = solid(TILE_SIZE, TILE_SIZE, [180, 20, 30, 96]);
    pixels[0] = [180, 20, 30, 255];
    write_png(
        directory.path(),
        "textures/blocks/shared_stained_glass",
        TILE_SIZE,
        TILE_SIZE,
        &pixels,
    );
    let records = [
        record(
            0,
            400,
            "minecraft:arbitrary_tinted_cube",
            "{}",
            BlockFlags::CUBE_GEOMETRY,
        ),
        model_record(
            1,
            401,
            "minecraft:red_stained_glass_pane",
            "{}",
            ModelFamily::Pane,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile shared alpha pack");

    assert_eq!(
        compiled.visuals[0].faces, [DIAGNOSTIC_MATERIAL; 6],
        "the pane's blend permission must not admit an opaque full-cube descriptor"
    );
    assert_eq!(compiled.visuals[1].kind, VisualKind::Model);
    let pane = compiled.visuals[1];
    assert!(
        template_quads(&compiled, pane.model_template)
            .iter()
            .all(|quad| compiled.materials[quad.material as usize].flags
                & MATERIAL_FLAG_ALPHA_BLEND
                != 0),
        "the stained pane must retain its per-descriptor blend material"
    );
}

#[test]
fn compiler_emits_exact_checked_stained_glass_cube_models() {
    let directory = tempfile::tempdir().expect("create stained-glass cube fixture");
    let mut blocks = serde_json::Map::new();
    let mut terrain = serde_json::Map::new();
    let mut expected_pixels = Vec::new();
    for (name_index, name) in ORDINARY_STAINED_GLASS_NAMES.iter().enumerate() {
        let short_name = name.strip_prefix("minecraft:").unwrap();
        let mut textures = serde_json::Map::new();
        let mut face_pixels = [[0; 4]; 6];
        for (face_index, face_name) in ["west", "east", "down", "up", "north", "south"]
            .into_iter()
            .enumerate()
        {
            let key = format!("glass_{name_index}_{face_index}");
            let path = format!("textures/blocks/{key}");
            let pixel = [
                8 + name_index as u8 * 7,
                16 + face_index as u8 * 13,
                200 - name_index as u8 * 5,
                32 + face_index as u8 * 31,
            ];
            textures.insert(face_name.into(), key.clone().into());
            terrain.insert(key, serde_json::json!({ "textures": path }));
            write_png(
                directory.path(),
                &path,
                TILE_SIZE,
                TILE_SIZE,
                &solid(TILE_SIZE, TILE_SIZE, pixel),
            );
            face_pixels[face_index] = pixel;
        }
        blocks.insert(
            short_name.into(),
            serde_json::json!({ "textures": textures }),
        );
        expected_pixels.push(face_pixels);
    }
    write_pack(
        directory.path(),
        &serde_json::Value::Object(blocks).to_string(),
        &serde_json::json!({ "texture_data": terrain }).to_string(),
        "[]",
    );

    let mut records = ORDINARY_STAINED_GLASS_NAMES
        .iter()
        .enumerate()
        .map(|(id, name)| {
            model_record(id as u32, 90_000 + id as u32, name, "{}", ModelFamily::Cube)
        })
        .collect::<Vec<_>>();
    let mut extra_state = model_record(
        records.len() as u32,
        90_100,
        "minecraft:red_stained_glass",
        r#"{"extra":{"type":"byte","value":0}}"#,
        ModelFamily::Cube,
    );
    extra_state.flags = BlockFlags::CUBE_GEOMETRY;
    records.push(extra_state);
    records.push(model_record(
        records.len() as u32,
        90_101,
        "minecraft:red_stained_glass",
        "{}",
        ModelFamily::Pane,
    ));
    let mut wrong_role = model_record(
        records.len() as u32,
        90_102,
        "minecraft:red_stained_glass",
        "{}",
        ModelFamily::Cube,
    );
    wrong_role.contributor_role = ContributorRole::LiquidAdditional;
    records.push(wrong_role);
    for name in [
        "minecraft:hard_red_stained_glass",
        "minecraft:slime",
        "minecraft:invisible_bedrock",
    ] {
        records.push(model_record(
            records.len() as u32,
            90_000 + records.len() as u32,
            name,
            "{}",
            ModelFamily::Cube,
        ));
    }

    let compiled = compile_pack(directory.path(), &records).expect("compile stained-glass cubes");
    for (id, expected) in expected_pixels.iter().enumerate() {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", records[id].name);
        assert_eq!(visual.contributor_role, ContributorRole::Primary);
        assert!(!visual.flags.intersects(
            BlockFlags::AIR
                | BlockFlags::CUBE_GEOMETRY
                | BlockFlags::OCCLUDES_FULL_FACE
                | BlockFlags::LEAF_MODEL
        ));
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE);
        assert_eq!(template.quad_count, 6);
        let quads = template_quads(&compiled, visual.model_template);
        assert_eq!(model_bounds(quads), ([0, 0, 0], [256, 256, 256]));
        for (face_index, quad) in quads.iter().enumerate() {
            assert_eq!(quad.material, visual.faces[face_index]);
            assert_eq!(quad.flags, [3, 4, 1, 2, 5, 6][face_index]);
            let material = compiled.materials[quad.material as usize];
            assert_eq!(material.flags, MATERIAL_FLAG_ALPHA_BLEND);
            assert_eq!(
                mip_pixel(&compiled, 0, material.texture.layer(), 0, 0),
                expected[face_index]
            );
        }
    }
    assert!(
        compiled.visuals[ORDINARY_STAINED_GLASS_NAMES.len()..]
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic
                && visual.faces == [DIAGNOSTIC_MATERIAL; 6])
    );

    let baseline = encode_blob(&compiled).expect("encode stained-glass cubes");
    records.reverse();
    let reversed = compile_pack(directory.path(), &records).expect("compile reversed records");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

#[test]
fn compiler_real_pinned_pack_admits_only_exact_stained_glass_cube_records() {
    let Some(pack) = std::env::var_os("PINNED_VANILLA_PACK") else {
        return;
    };
    let all = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let ordinary_stained_glass = all
        .iter()
        .filter(|record| {
            ORDINARY_STAINED_GLASS_NAMES
                .binary_search(&record.name.as_ref())
                .is_ok()
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(ordinary_stained_glass.len(), 16);
    assert!(ordinary_stained_glass.iter().all(|record| {
        record.canonical_state.as_ref() == "{}"
            && record.model_family == ModelFamily::Cube
            && record.contributor_role == ContributorRole::Primary
    }));
    let excluded = all
        .iter()
        .filter(|record| {
            record.name.starts_with("minecraft:hard_") && record.name.ends_with("_stained_glass")
                || record.name.as_ref() == "minecraft:slime"
                || record.name.as_ref() == "minecraft:invisible_bedrock"
        })
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        excluded
            .iter()
            .any(|record| record.name.as_ref() == "minecraft:slime")
    );
    assert!(
        excluded
            .iter()
            .any(|record| record.name.as_ref() == "minecraft:invisible_bedrock")
    );

    let ordinary_count = ordinary_stained_glass.len();
    let mut records = ordinary_stained_glass
        .into_iter()
        .chain(excluded)
        .collect::<Vec<_>>();
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 91_000 + id as u32;
    }
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned stained glass");
    for (id, visual) in compiled.visuals.iter().enumerate() {
        if id < ordinary_count {
            assert_eq!(visual.kind, VisualKind::Model, "{}", records[id].name);
            assert_eq!(
                compiled.model_templates[visual.model_template as usize].flags,
                MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE
            );
            assert!(visual.faces.iter().all(|&material| {
                material != DIAGNOSTIC_MATERIAL
                    && compiled.materials[material as usize].flags == MATERIAL_FLAG_ALPHA_BLEND
            }));
        } else {
            assert_eq!(visual.kind, VisualKind::Diagnostic, "{}", records[id].name);
        }
    }
    let baseline = encode_blob(&compiled).expect("encode pinned stained glass");
    records.reverse();
    let reversed =
        compile_pack(Path::new(&pack), &records).expect("compile reversed pinned records");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

#[test]
fn compiler_emits_exact_checked_copper_grate_models() {
    let directory = tempfile::tempdir().expect("create copper-grate fixture");
    let mut blocks = serde_json::Map::new();
    let mut terrain = serde_json::Map::new();
    for (pair_index, (unwaxed, waxed)) in COPPER_GRATE_ALIAS_PAIRS.iter().enumerate() {
        let texture = format!("copper_grate_{pair_index}");
        let path = format!("textures/blocks/{texture}");
        terrain.insert(texture.clone(), serde_json::json!({ "textures": path }));
        for name in [unwaxed, waxed] {
            blocks.insert(
                name.strip_prefix("minecraft:").unwrap().into(),
                serde_json::json!({ "textures": texture }),
            );
        }
        write_png(
            directory.path(),
            &path,
            TILE_SIZE,
            TILE_SIZE,
            &solid(
                TILE_SIZE,
                TILE_SIZE,
                [35 + pair_index as u8 * 40, 90, 130, 64],
            ),
        );
    }
    write_pack(
        directory.path(),
        &serde_json::Value::Object(blocks).to_string(),
        &serde_json::json!({ "texture_data": terrain }).to_string(),
        "[]",
    );

    let mut records = COPPER_GRATE_NAMES
        .iter()
        .enumerate()
        .map(|(id, name)| {
            let mut record =
                model_record(id as u32, 92_000 + id as u32, name, "{}", ModelFamily::Cube);
            record.flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
            record
        })
        .collect::<Vec<_>>();
    let admitted_count = records.len();
    let mut wrong_state = model_record(
        admitted_count as u32,
        92_100,
        "minecraft:copper_grate",
        r#"{"extra":{"type":"byte","value":0}}"#,
        ModelFamily::Cube,
    );
    wrong_state.flags = BlockFlags::CUBE_GEOMETRY;
    records.push(wrong_state);
    records.push(model_record(
        records.len() as u32,
        92_101,
        "minecraft:copper_grate",
        "{}",
        ModelFamily::Pane,
    ));
    let mut wrong_role = model_record(
        records.len() as u32,
        92_102,
        "minecraft:copper_grate",
        "{}",
        ModelFamily::Cube,
    );
    wrong_role.contributor_role = ContributorRole::LiquidAdditional;
    records.push(wrong_role);
    for name in [
        "minecraft:cut_copper_grate",
        "minecraft:copper_bars",
        "minecraft:copper_bulb",
        "minecraft:copper_door",
        "minecraft:copper_trapdoor",
        "minecraft:slime",
        "minecraft:glass",
        "minecraft:red_stained_glass_pane",
        "minecraft:invisible_bedrock",
    ] {
        records.push(model_record(
            records.len() as u32,
            92_000 + records.len() as u32,
            name,
            "{}",
            ModelFamily::Cube,
        ));
    }

    let compiled = compile_pack(directory.path(), &records).expect("compile copper grates");
    for (id, record) in records.iter().take(admitted_count).enumerate() {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.name);
        assert!(!visual.flags.intersects(
            BlockFlags::AIR
                | BlockFlags::CUBE_GEOMETRY
                | BlockFlags::OCCLUDES_FULL_FACE
                | BlockFlags::LEAF_MODEL
        ));
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE);
        assert_eq!(template.quad_count, 6);
        let quads = template_quads(&compiled, visual.model_template);
        assert_eq!(model_bounds(quads), ([0, 0, 0], [256, 256, 256]));
        assert!(quads.iter().enumerate().all(|(face, quad)| {
            quad.material == visual.faces[face]
                && quad.flags == [3, 4, 1, 2, 5, 6][face]
                && compiled.materials[quad.material as usize].flags & MATERIAL_FLAG_ALPHA_CUTOUT
                    != 0
                && compiled.materials[quad.material as usize].flags & MATERIAL_FLAG_ALPHA_BLEND == 0
        }));
    }
    for (unwaxed, waxed) in COPPER_GRATE_ALIAS_PAIRS {
        let faces = |name: &str| {
            let index = records
                .iter()
                .position(|record| record.name.as_ref() == name)
                .unwrap();
            compiled.visuals[index].faces
        };
        assert_eq!(faces(unwaxed), faces(waxed), "alias pair {unwaxed}/{waxed}");
    }
    assert!(
        compiled.visuals[admitted_count..]
            .iter()
            .all(|visual| visual.kind == VisualKind::Diagnostic
                && visual.faces == [DIAGNOSTIC_MATERIAL; 6])
    );

    let baseline = encode_blob(&compiled).expect("encode copper grates");
    records.reverse();
    let reversed =
        compile_pack(directory.path(), &records).expect("compile reversed copper grates");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

fn single_copper_grate_fixture() -> TempDir {
    let directory = tempfile::tempdir().expect("create single copper-grate fixture");
    write_pack(
        directory.path(),
        r#"{"copper_grate":{"textures":"copper_grate"}}"#,
        r#"{"texture_data":{"copper_grate":{"textures":"textures/blocks/copper_grate"}}}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/copper_grate",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [184, 115, 51, 64]),
    );
    directory
}

#[test]
fn compiler_rejects_exact_copper_grate_marked_air() {
    let directory = single_copper_grate_fixture();
    let mut record = model_record(0, 92_200, "minecraft:copper_grate", "{}", ModelFamily::Cube);
    record.flags = BlockFlags::AIR;

    let compiled = compile_pack(directory.path(), &[record]).expect("compile air copper grate");

    assert_eq!(compiled.visuals[0].kind, VisualKind::Diagnostic);
    assert_eq!(compiled.visuals[0].faces, [DIAGNOSTIC_MATERIAL; 6]);
}

#[test]
fn compiler_rejects_exact_copper_grate_with_flags_zero() {
    let directory = single_copper_grate_fixture();
    let record = model_record(0, 92_201, "minecraft:copper_grate", "{}", ModelFamily::Cube);

    let compiled =
        compile_pack(directory.path(), &[record]).expect("compile flags-zero copper grate");

    assert_eq!(compiled.visuals[0].kind, VisualKind::Diagnostic);
    assert_eq!(compiled.visuals[0].faces, [DIAGNOSTIC_MATERIAL; 6]);
}

#[test]
#[ignore = "requires PINNED_VANILLA_PACK pointing at the ignored pinned vanilla resource pack"]
fn compiler_real_pinned_pack_admits_only_exact_copper_grate_records() {
    let pack = std::env::var_os("PINNED_VANILLA_PACK")
        .expect("set PINNED_VANILLA_PACK to the ignored pinned vanilla resource pack");
    let all = read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let copper_grates = all
        .iter()
        .filter(|record| {
            COPPER_GRATE_NAMES
                .binary_search(&record.name.as_ref())
                .is_ok()
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(copper_grates.len(), 8);
    assert!(copper_grates.iter().all(|record| {
        record.canonical_state.as_ref() == "{}"
            && record.model_family == ModelFamily::Cube
            && record.contributor_role == ContributorRole::Primary
            && record.flags == BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
    }));
    let excluded = all
        .iter()
        .filter(|record| {
            record.name.contains("copper_grate")
                && COPPER_GRATE_NAMES
                    .binary_search(&record.name.as_ref())
                    .is_err()
                || matches!(
                    record.name.as_ref(),
                    "minecraft:slime" | "minecraft:invisible_bedrock"
                )
                || record.name.starts_with("minecraft:hard_")
                    && record.name.ends_with("_stained_glass")
        })
        .cloned()
        .collect::<Vec<_>>();
    assert!(!excluded.is_empty());

    let admitted_count = copper_grates.len();
    let mut records = copper_grates
        .into_iter()
        .chain(excluded)
        .collect::<Vec<_>>();
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 93_000 + id as u32;
    }
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned copper grates");
    for (id, visual) in compiled.visuals.iter().enumerate() {
        if id < admitted_count {
            assert_eq!(visual.kind, VisualKind::Model, "{}", records[id].name);
            let template = compiled.model_templates[visual.model_template as usize];
            assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE);
            assert_eq!(template.quad_count, 6);
            assert!(visual.faces.iter().all(|&material| {
                material != DIAGNOSTIC_MATERIAL
                    && compiled.materials[material as usize].flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0
                    && compiled.materials[material as usize].flags & MATERIAL_FLAG_ALPHA_BLEND == 0
            }));
            assert!(!visual.flags.intersects(
                BlockFlags::AIR
                    | BlockFlags::CUBE_GEOMETRY
                    | BlockFlags::OCCLUDES_FULL_FACE
                    | BlockFlags::LEAF_MODEL
            ));
        } else {
            assert_eq!(visual.kind, VisualKind::Diagnostic, "{}", records[id].name);
        }
    }
    for (unwaxed, waxed) in COPPER_GRATE_ALIAS_PAIRS {
        let faces = |name: &str| {
            let index = records
                .iter()
                .position(|record| record.name.as_ref() == name)
                .unwrap();
            compiled.visuals[index].faces
        };
        assert_eq!(faces(unwaxed), faces(waxed), "alias pair {unwaxed}/{waxed}");
    }
    let baseline = encode_blob(&compiled).expect("encode pinned copper grates");
    records.reverse();
    let reversed =
        compile_pack(Path::new(&pack), &records).expect("compile reversed pinned copper grates");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}
