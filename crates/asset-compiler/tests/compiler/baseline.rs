use super::support::*;

#[test]
fn compiler_only_loads_full_cubes_and_builds_equivalent_lookup_tables() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "flower": {"textures": "missing_flower"},
            "stone": {"textures": "stone"}
        }"#,
        r#"{"texture_data": {
            "missing_flower": {"textures": "textures/blocks/not_present"},
            "stone": {"textures": "textures/blocks/stone"}
        }}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/stone",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [100, 100, 100, 255]),
    );
    let records = [
        record(
            5,
            0xf000_0005,
            "minecraft:stone",
            "{}",
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(
            2,
            0x8000_0002,
            "minecraft:flower",
            "{}",
            BlockFlags::empty(),
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile full cubes only");

    assert_eq!(compiled.visuals.len(), 6);
    assert_eq!(compiled.visuals[2].faces, [0; 6]);
    assert!(
        compiled.visuals[5]
            .faces
            .into_iter()
            .all(|material| material != 0)
    );
    assert_eq!(&*compiled.hashed, &[(0x8000_0002, 2), (0xf000_0005, 5)]);
    for &(hash, visual_index) in compiled.hashed.iter() {
        let record = records
            .iter()
            .find(|record| record.network_hash == hash)
            .expect("hash record");
        assert_eq!(visual_index, record.sequential_id);
        assert_eq!(
            compiled.visuals[visual_index as usize],
            compiled.visuals[record.sequential_id as usize]
        );
    }
}

#[test]
fn compiler_compiles_grass_and_flipbook_faces_but_keeps_unlisted_blocks_diagnostic() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "grass": {"textures": {
                "down": "grass_bottom", "side": "grass_side", "up": "grass_top"
            }},
            "seaLantern": {"textures": "sea_lantern"}
        }"#,
        r#"{"texture_data": {
            "grass_bottom": {"textures": "textures/blocks/grass_bottom"},
            "grass_side": {"textures": "textures/blocks/grass_side"},
            "grass_top": {"textures": "textures/blocks/grass_top"},
            "sea_lantern": {"textures": "textures/blocks/sea_lantern"}
        }}"#,
        r#"[{
            "flipbook_texture": "textures/blocks/sea_lantern",
            "atlas_tile": "sea_lantern",
            "ticks_per_frame": 5
        }]"#,
    );
    for (path, colour) in [
        ("textures/blocks/grass_bottom", [80, 50, 20, 255]),
        ("textures/blocks/grass_top", [70, 190, 50, 255]),
    ] {
        write_png(
            directory.path(),
            path,
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, colour),
        );
    }
    write_tga(
        directory.path(),
        "textures/blocks/grass_side",
        TILE_SIZE as u16,
        TILE_SIZE as u16,
        &solid(TILE_SIZE, TILE_SIZE, [100, 150, 60, 255]),
    );
    let mut sea_lantern_strip = solid(TILE_SIZE, TILE_SIZE, [30, 180, 200, 255]);
    sea_lantern_strip.extend(solid(TILE_SIZE, TILE_SIZE, [50, 210, 230, 255]));
    write_png(
        directory.path(),
        "textures/blocks/sea_lantern",
        TILE_SIZE,
        TILE_SIZE * 2,
        &sea_lantern_strip,
    );
    let records = [
        record(
            0,
            0xde31_28b4,
            "minecraft:grass_block",
            "null",
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(
            1,
            0x1111_1111,
            "minecraft:sea_lantern",
            "null",
            BlockFlags::CUBE_GEOMETRY,
        ),
        record(
            2,
            0x2222_2222,
            "minecraft:invisible_bedrock",
            "null",
            BlockFlags::CUBE_GEOMETRY,
        ),
    ];

    let compiled = compile_pack(directory.path(), &records).expect("compile deferred visuals");
    let grass = compiled.visuals[0];
    let sea_lantern = compiled.visuals[1];

    assert!(grass.faces.into_iter().all(|material| material != 0));
    assert_eq!(material_for_face(&compiled, 0, BlockFace::Down).flags, 0);
    assert_eq!(
        material_for_face(&compiled, 0, BlockFace::Up).flags,
        MATERIAL_FLAG_GRASS_TINT
    );
    assert_eq!(
        material_for_face(&compiled, 0, BlockFace::North).flags,
        MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_OVERLAY_MASK
    );
    assert!(
        sea_lantern
            .faces
            .into_iter()
            .all(|material| material != DIAGNOSTIC_MATERIAL)
    );
    assert_eq!(
        compiled.materials[sea_lantern.faces[0] as usize].animation,
        0
    );
    assert_eq!(compiled.visuals[2].faces, [DIAGNOSTIC_MATERIAL; 6]);
    assert_eq!(compiled.materials.len(), 5);
    assert_eq!(compiled.texture_pages[0].texture.layers, 6);
}

#[test]
fn compiler_installs_recognized_flipbooks_after_loading_the_strip() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{
            "water":{"textures":{"up":"water_still","down":"water_still","side":"water_flow"}},
            "lava":{"textures":{"up":"lava_still","down":"lava_still","side":"lava_flow"}},
            "broken_water":{"textures":{"west":"water_flow","east":"water_flow","up":"water_still","north":"water_flow","south":"water_flow"}}
        }"#,
        r#"{"texture_data":{
            "water_still":{"textures":"textures/blocks/water_still"},
            "water_flow":{"textures":"textures/blocks/water_flow"},
            "lava_still":{"textures":"textures/blocks/lava_still"},
            "lava_flow":{"textures":"textures/blocks/lava_flow"}
        }}"#,
        r#"[
            {"flipbook_texture":"textures/blocks/water_still","atlas_tile":"water_still"},
            {"flipbook_texture":"textures/blocks/water_flow","atlas_tile":"water_flow"},
            {"flipbook_texture":"textures/blocks/lava_still","atlas_tile":"lava_still"},
            {"flipbook_texture":"textures/blocks/lava_flow","atlas_tile":"lava_flow"}
        ]"#,
    );
    let mut water_record = model_record(
        0,
        700,
        "minecraft:water",
        r#"{"liquid_depth":{"type":"int","value":3}}"#,
        ModelFamily::Liquid,
    );
    water_record.contributor_role = ContributorRole::LiquidAdditional;
    assert_eq!(
        water_record.model_state.get(ModelStateField::LiquidDepth),
        None
    );
    let mut lava_record = model_record(
        1,
        701,
        "minecraft:lava",
        r#"{"liquid_depth":{"type":"int","value":15}}"#,
        ModelFamily::Liquid,
    );
    lava_record.contributor_role = ContributorRole::LiquidAdditional;
    let mut missing_depth = model_record(2, 702, "minecraft:water", "{}", ModelFamily::Liquid);
    missing_depth.contributor_role = ContributorRole::LiquidAdditional;
    let mut out_of_range_depth = model_record(
        3,
        703,
        "minecraft:water",
        r#"{"liquid_depth":{"type":"int","value":16}}"#,
        ModelFamily::Liquid,
    );
    out_of_range_depth.contributor_role = ContributorRole::LiquidAdditional;
    let mut missing_face = model_record(
        4,
        704,
        "minecraft:broken_water",
        r#"{"liquid_depth":{"type":"int","value":2}}"#,
        ModelFamily::Liquid,
    );
    missing_face.contributor_role = ContributorRole::LiquidAdditional;
    let records = [
        water_record,
        lava_record,
        missing_depth,
        out_of_range_depth,
        missing_face,
    ];
    for (index, path) in ["water_still", "water_flow", "lava_still", "lava_flow"]
        .into_iter()
        .enumerate()
    {
        let mut strip = solid(TILE_SIZE, TILE_SIZE, [20 + index as u8, 80, 200, 180]);
        strip.extend(solid(
            TILE_SIZE,
            TILE_SIZE,
            [30 + index as u8, 100, 220, 180],
        ));
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE * 2,
            &strip,
        );
    }

    let compiled = compile_pack(directory.path(), &records).expect("compile flipbook reference");

    assert_eq!(compiled.visuals[0].kind, VisualKind::Liquid);
    assert_eq!(
        compiled.visuals[0].contributor_role,
        ContributorRole::LiquidAdditional
    );
    assert!(
        !compiled.visuals[0]
            .flags
            .contains(BlockFlags::CUBE_GEOMETRY)
    );
    assert_eq!(compiled.visuals[0].variant, 3);
    assert_eq!(compiled.visuals[1].kind, VisualKind::Liquid);
    assert_eq!(compiled.visuals[1].variant, 15);
    for visual in &compiled.visuals[2..] {
        assert_eq!(visual.kind, VisualKind::Diagnostic);
        assert_eq!(visual.faces, [DIAGNOSTIC_MATERIAL; 6]);
        assert_eq!(visual.variant, 0);
    }
    assert_eq!(
        compiled.visuals[0].faces[BlockFace::Up as usize],
        compiled.visuals[0].faces[BlockFace::Down as usize]
    );
    assert_eq!(
        compiled.visuals[0].faces[BlockFace::West as usize],
        compiled.visuals[0].faces[BlockFace::East as usize]
    );
    assert_eq!(
        compiled.visuals[0].faces[BlockFace::North as usize],
        compiled.visuals[0].faces[BlockFace::South as usize]
    );
    assert_ne!(
        compiled.visuals[0].faces[BlockFace::Up as usize],
        compiled.visuals[0].faces[BlockFace::North as usize]
    );
    assert_eq!(
        compiled.visuals[1].contributor_role,
        ContributorRole::LiquidAdditional
    );
    assert_eq!(
        compiled.materials.len(),
        5,
        "unsupported liquid descriptors sharing alpha strips must not create materials"
    );
    let water_material =
        compiled.materials[compiled.visuals[0].faces[BlockFace::Up as usize] as usize];
    let water_flow =
        compiled.materials[compiled.visuals[0].faces[BlockFace::North as usize] as usize];
    assert_eq!(
        water_material.flags,
        MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT
    );
    assert_eq!(water_material.animation, 0);
    assert_eq!(water_flow.animation, 1);
    let lava_still = compiled.materials[compiled.visuals[1].faces[BlockFace::Up as usize] as usize];
    let lava_flow =
        compiled.materials[compiled.visuals[1].faces[BlockFace::North as usize] as usize];
    assert_eq!(lava_still.flags, MATERIAL_FLAG_LIQUID_DEPTH_WRITE);
    assert_eq!(lava_flow.flags, MATERIAL_FLAG_LIQUID_DEPTH_WRITE);
    assert_eq!(lava_still.animation, 2);
    assert_eq!(lava_flow.animation, 3);
    assert_eq!(compiled.animations.len(), 4);
    assert_eq!(compiled.animation_frames.len(), 8);
    assert_eq!(compiled.texture_pages[0].texture.layers, 9);
}

#[test]
fn compiler_maps_every_protocol_1001_lava_depth_for_both_runtime_names() {
    let directory = tempfile::tempdir().expect("create lava fixture");
    write_pack(
        directory.path(),
        r#"{
            "lava":{"textures":{"up":"lava_still","down":"lava_still","side":"lava_flow"}},
            "flowing_lava":{"textures":{"up":"lava_still","down":"lava_still","side":"lava_flow"}}
        }"#,
        r#"{"texture_data":{
            "lava_still":{"textures":"textures/blocks/lava_still"},
            "lava_flow":{"textures":"textures/blocks/lava_flow"}
        }}"#,
        r#"[
            {"flipbook_texture":"textures/blocks/lava_still","atlas_tile":"lava_still"},
            {"flipbook_texture":"textures/blocks/lava_flow","atlas_tile":"lava_flow"}
        ]"#,
    );
    for (index, path) in ["lava_still", "lava_flow"].into_iter().enumerate() {
        let mut strip = solid(TILE_SIZE, TILE_SIZE, [240, 40 + index as u8, 4, 255]);
        strip.extend(solid(TILE_SIZE, TILE_SIZE, [255, 80 + index as u8, 8, 255]));
        write_png(
            directory.path(),
            &format!("textures/blocks/{path}"),
            TILE_SIZE,
            TILE_SIZE * 2,
            &strip,
        );
    }

    let mut records = Vec::new();
    for (name_index, name) in ["minecraft:lava", "minecraft:flowing_lava"]
        .into_iter()
        .enumerate()
    {
        for depth in 0..16_u32 {
            let sequential_id = (name_index as u32) * 16 + depth;
            let mut record = model_record(
                sequential_id,
                10_000 + sequential_id,
                name,
                &format!(r#"{{"liquid_depth":{{"type":"int","value":{depth}}}}}"#),
                ModelFamily::Liquid,
            );
            record.contributor_role = ContributorRole::LiquidAdditional;
            records.push(record);
        }
    }

    let compiled = compile_pack(directory.path(), &records).expect("compile all lava states");
    assert_eq!(compiled.visuals.len(), 32);
    for (sequential_id, visual) in compiled.visuals.iter().enumerate() {
        assert_eq!(visual.kind, VisualKind::Liquid, "state {sequential_id}");
        assert_eq!(
            visual.contributor_role,
            ContributorRole::LiquidAdditional,
            "state {sequential_id}",
        );
        assert_eq!(visual.variant, sequential_id as u32 % 16);
        for material in visual.faces {
            assert_ne!(material, DIAGNOSTIC_MATERIAL);
            assert_eq!(
                compiled.materials[material as usize].flags,
                MATERIAL_FLAG_LIQUID_DEPTH_WRITE,
            );
        }
    }
    assert_eq!(compiled.animations.len(), 2);
}

fn one_texture_fixture() -> (TempDir, RegistryRecord, PathBuf) {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{"broken":{"textures":"broken_key"}}"#,
        r#"{"texture_data":{"broken_key":{"textures":"textures/blocks/broken"}}}"#,
        "[]",
    );
    let expected = directory.path().join("textures/blocks/broken.png");
    (
        directory,
        record(0, 900, "minecraft:broken", "{}", BlockFlags::CUBE_GEOMETRY),
        expected,
    )
}

fn assert_source_context(error: &AssetError, expected_path: &Path) {
    let rendered = error.to_string();
    assert!(
        rendered.contains("broken_key"),
        "missing source key: {rendered}"
    );
    assert!(
        rendered.contains(expected_path.to_string_lossy().as_ref()),
        "missing source path: {rendered}"
    );
}

#[test]
fn compiler_reports_missing_malformed_and_wrong_size_png_sources() {
    let (missing_root, missing_record, missing_path) = one_texture_fixture();
    let error = compile_pack(missing_root.path(), &[missing_record]).expect_err("missing PNG");
    assert!(matches!(error, AssetError::TextureIo { .. }));
    assert_source_context(&error, &missing_path);

    let (malformed_root, malformed_record, malformed_path) = one_texture_fixture();
    write_file(&malformed_path, b"not a png");
    let error =
        compile_pack(malformed_root.path(), &[malformed_record]).expect_err("malformed PNG");
    assert!(matches!(error, AssetError::TextureDecode { .. }));
    assert_source_context(&error, &malformed_path);

    let (sized_root, sized_record, sized_path) = one_texture_fixture();
    write_file(&sized_path, png_bytes(8, 16, &solid(8, 16, [1, 2, 3, 255])));
    let error = compile_pack(sized_root.path(), &[sized_record]).expect_err("wrong PNG size");
    assert!(matches!(
        error,
        AssetError::WrongTextureDimensions {
            width: 8,
            height: 16,
            ..
        }
    ));
    assert_source_context(&error, &sized_path);
}

#[test]
fn compiler_reports_malformed_and_wrong_size_tga_with_source_context() {
    let fixture = || {
        let directory = tempfile::tempdir().expect("create fixture");
        write_pack(
            directory.path(),
            r#"{"broken":{"textures":"broken_key"}}"#,
            r#"{"texture_data":{"broken_key":{"textures":"textures/blocks/broken.tga"}}}"#,
            "[]",
        );
        let path = directory.path().join("textures/blocks/broken.tga");
        let record = record(0, 901, "minecraft:broken", "{}", BlockFlags::CUBE_GEOMETRY);
        (directory, record, path)
    };

    let (malformed_root, malformed_record, malformed_path) = fixture();
    write_file(&malformed_path, b"not a tga");
    let error =
        compile_pack(malformed_root.path(), &[malformed_record]).expect_err("malformed TGA");
    assert!(matches!(error, AssetError::TextureDecode { .. }));
    assert_source_context(&error, &malformed_path);

    let (sized_root, sized_record, sized_path) = fixture();
    write_file(&sized_path, tga_bytes(8, 16, &solid(8, 16, [1, 2, 3, 255])));
    let error = compile_pack(sized_root.path(), &[sized_record]).expect_err("wrong TGA size");
    assert!(matches!(
        error,
        AssetError::WrongTextureDimensions {
            width: 8,
            height: 16,
            ..
        }
    ));
    assert_source_context(&error, &sized_path);
}

#[test]
fn compiler_rejects_unsupported_static_texture_formats_with_source_context() {
    let directory = tempfile::tempdir().expect("create fixture");
    write_pack(
        directory.path(),
        r#"{"broken":{"textures":"broken_key"}}"#,
        r#"{"texture_data":{"broken_key":{"textures":"textures/blocks/broken.jpg"}}}"#,
        "[]",
    );
    let path = directory.path().join("textures/blocks/broken.jpg");
    write_file(&path, b"not a supported static texture");
    let record = record(0, 902, "minecraft:broken", "{}", BlockFlags::CUBE_GEOMETRY);

    let error = compile_pack(directory.path(), &[record]).expect_err("unsupported texture format");
    let rendered = error.to_string();
    assert!(rendered.contains("unsupported"), "{rendered}");
    assert!(rendered.contains("broken_key"), "{rendered}");
    assert!(
        rendered.contains(path.to_string_lossy().as_ref()),
        "{rendered}"
    );
}

#[test]
fn compiler_rolls_reachable_layers_into_the_bounded_second_page() {
    let directory = tempfile::tempdir().expect("create fixture");
    let mut blocks = String::from("{");
    let mut terrain = String::from(r#"{"texture_data":{"#);
    let mut records = Vec::with_capacity(MAX_TEXTURE_LAYERS);
    for index in 0..MAX_TEXTURE_LAYERS {
        if index != 0 {
            blocks.push(',');
            terrain.push(',');
        }
        write!(blocks, r#""block_{index}":{{"textures":"key_{index}"}}"#)
            .expect("append block JSON");
        write!(
            terrain,
            r#""key_{index}":{{"textures":"textures/generated/layer_{index}"}}"#
        )
        .expect("append terrain JSON");
        let color = [index as u8, (index >> 8) as u8, (index >> 16) as u8, 255];
        write_png(
            directory.path(),
            &format!("textures/generated/layer_{index}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, color),
        );
        records.push(record(
            index as u32,
            0x8000_0000 + index as u32,
            &format!("minecraft:block_{index}"),
            "{}",
            BlockFlags::CUBE_GEOMETRY,
        ));
    }
    blocks.push('}');
    terrain.push_str("}}");
    write_pack(directory.path(), &blocks, &terrain, "[]");

    let compiled = compile_pack(directory.path(), &records).expect("two-page asset set");
    assert_eq!(compiled.texture_pages.len(), 2);
    assert_eq!(
        compiled.texture_pages[0].texture.layers,
        MAX_TEXTURE_LAYERS as u32
    );
    assert_eq!(compiled.texture_pages[1].texture.layers, 1);
}
