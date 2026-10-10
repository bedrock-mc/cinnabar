use super::*;

fn grass_pack(side_entry: serde_json::Value) -> tempfile::TempDir {
    let pack = pack();
    write(
        pack.path(),
        "blocks.json",
        br#"{"grass":{"textures":{"up":"world_top","down":"bottom","side":"world_side"},"carried_textures":{"up":"carried_top","down":"bottom","side":"carried_side"}}}"#,
    );
    write(
        pack.path(),
        "textures/terrain_texture.json",
        &serde_json::to_vec(&serde_json::json!({"texture_data":{
            "world_top":{"textures":"textures/blocks/top"},
            "world_side":{"textures":"textures/blocks/side"},
            "bottom":{"textures":"textures/blocks/bottom"},
            "carried_top":{"textures":"textures/blocks/top"},
            "carried_side":{"textures":side_entry}
        }}))
        .unwrap(),
    );
    write(pack.path(), "textures/flipbook_textures.json", b"[]");
    fs::create_dir_all(pack.path().join("textures/blocks")).unwrap();
    for (path, pixel) in [("top", [30, 140, 20, 255]), ("bottom", [110, 70, 35, 255])] {
        image::save_buffer(
            pack.path().join(format!("textures/blocks/{path}.png")),
            &pixel.repeat((TILE_SIZE as usize).pow(2)),
            TILE_SIZE,
            TILE_SIZE,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
    // Bedrock's opaque overlay material treats alpha as a tint mask, not
    // opacity: alpha-zero soil keeps its RGB; alpha-one grey foliage is tinted.
    let mut side = Vec::new();
    for y in 0..TILE_SIZE {
        side.extend_from_slice(
            &if y < TILE_SIZE / 2 {
                [200, 200, 200, 255]
            } else {
                [110, 70, 35, 0]
            }
            .repeat(TILE_SIZE as usize),
        );
    }
    image::save_buffer(
        pack.path().join("textures/blocks/side.png"),
        &side,
        TILE_SIZE,
        TILE_SIZE,
        image::ColorType::Rgba8,
    )
    .unwrap();
    pack
}

fn grass_world(entity: &CompiledEntityAssets) -> (CompiledAssets, BlockVisualId) {
    let mut source = world(entity);
    let ItemVisualDefinitionRoute::BlockItem { block_visual } = entity
        .item_visuals
        .iter()
        .find(|visual| visual.key.identifier.as_ref() == "minecraft:grass_block")
        .expect("pinned registry grass item")
        .route
    else {
        panic!("grass requires a block item route");
    };
    source.visuals[block_visual.0 as usize] = source
        .visuals
        .iter()
        .copied()
        .find(|visual| visual.kind == VisualKind::Cube)
        .unwrap();
    source.materials[1].flags = MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_OVERLAY_MASK;
    (source, block_visual)
}

#[test]
fn carried_overlay_grass_generates_a_visible_inventory_icon() {
    let pack = grass_pack(serde_json::json!({
        "path":"textures/blocks/side", "overlay_color":"#79c05a"
    }));
    let entity = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let (source, _) = grass_world(&entity);
    let compiled =
        compile_icon_assets_with_blocks(pack.path(), MANIFEST, &runtime(&source)).unwrap();
    let catalog = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
    let sprite = catalog
        .lookup("minecraft:grass_block", 0)
        .expect("carried overlay grass must not disappear");
    assert!(
        sprite
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] == 255)
    );
}

fn sheet_pixel(sheet: &IconSprite, face: BlockFace, x: usize, y: usize) -> &[u8] {
    let side = usize::from(BLOCK_ITEM_FACE_SIDE);
    let columns = usize::from(BLOCK_ITEM_SHEET_GRID[0]);
    let tile = face as usize;
    let x = (tile % columns) * side + x;
    let y = (tile / columns) * side + y;
    let offset = (y * usize::from(sheet.width) + x) * 4;
    &sheet.rgba8[offset..offset + 4]
}

fn transparent_cube_template(source: &mut CompiledAssets, material: u32) -> u32 {
    let positions = [
        [[0, 0, 0], [0, 0, 256], [0, 256, 256], [0, 256, 0]],
        [[256, 0, 0], [256, 256, 0], [256, 256, 256], [256, 0, 256]],
        [[0, 0, 0], [256, 0, 0], [256, 0, 256], [0, 0, 256]],
        [[0, 256, 0], [0, 256, 256], [256, 256, 256], [256, 256, 0]],
        [[0, 0, 0], [0, 256, 0], [256, 256, 0], [256, 0, 0]],
        [[0, 0, 256], [256, 0, 256], [256, 256, 256], [0, 256, 256]],
    ];
    let mut templates = source.model_templates.to_vec();
    let mut quads = source.model_quads.to_vec();
    let template = templates.len() as u32;
    templates.push(ModelTemplate {
        quad_start: quads.len() as u32,
        quad_count: positions.len() as u32,
        flags: MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE,
    });
    quads.extend(
        BlockFace::ALL
            .into_iter()
            .zip(positions)
            .map(|(face, positions)| ModelQuad {
                positions,
                uvs: [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
                material,
                flags: face.model_quad_face_id(),
            }),
    );
    source.model_templates = templates.into();
    source.model_quads = quads.into();
    template
}

#[test]
fn carried_overlay_sheet_masks_tint_without_coloring_or_hiding_soil() {
    let pack = grass_pack(serde_json::json!({
        "path":"textures/blocks/side", "overlay_color":"#79c05a"
    }));
    let entity = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let (source, visual) = grass_world(&entity);
    let compiled =
        compile_icon_assets_with_blocks(pack.path(), MANIFEST, &runtime(&source)).unwrap();
    let catalog = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
    let binding = catalog
        .block_sheets()
        .iter()
        .find(|binding| binding.visual == visual)
        .expect("the grass item publishes its carried cube sheet");
    let sheet = &catalog.sprites()[binding.sprite as usize];
    assert_eq!([sheet.width, sheet.height], BLOCK_ITEM_SHEET_SIZE);
    for face in [
        BlockFace::West,
        BlockFace::East,
        BlockFace::North,
        BlockFace::South,
    ] {
        assert_eq!(sheet_pixel(sheet, face, 0, 0), &[94, 150, 70, 255]);
        assert_eq!(
            sheet_pixel(sheet, face, 0, usize::from(BLOCK_ITEM_FACE_SIDE) - 1),
            &[110, 70, 35, 255]
        );
    }
    assert_eq!(
        sheet_pixel(sheet, BlockFace::Down, 0, 0),
        &[110, 70, 35, 255]
    );
    assert_eq!(sheet_pixel(sheet, BlockFace::Up, 0, 0), &[30, 140, 20, 255]);
    assert!(
        sheet
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
    assert_eq!(
        compiled.bytes,
        encode_icon_catalog_with_block_sheets(
            catalog.source_manifest_sha256(),
            catalog.sprites(),
            catalog.entries(),
            catalog.block_sheets()
        )
        .unwrap()
    );
}

#[test]
fn carried_overlay_ignores_hex_high_alpha_byte_and_keeps_partial_mask() {
    let pack = grass_pack(serde_json::json!({
        "path":"textures/blocks/side", "overlay_color":"#0079c05a"
    }));
    let pixels = [200, 200, 200, 128].repeat((TILE_SIZE as usize).pow(2));
    image::save_buffer(
        pack.path().join("textures/blocks/side.png"),
        &pixels,
        TILE_SIZE,
        TILE_SIZE,
        image::ColorType::Rgba8,
    )
    .unwrap();
    let entity = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let (source, visual) = grass_world(&entity);
    let compiled =
        compile_icon_assets_with_blocks(pack.path(), MANIFEST, &runtime(&source)).unwrap();
    let catalog = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
    let binding = catalog
        .block_sheets()
        .iter()
        .find(|sheet| sheet.visual == visual)
        .unwrap();
    let sheet = &catalog.sprites()[binding.sprite as usize];
    assert_eq!(
        sheet_pixel(sheet, BlockFace::West, 0, 0),
        &[147, 175, 135, 255]
    );
}

#[test]
fn carried_overlay_unknown_metadata_and_malformed_colors_remain_unresolved() {
    for side in [
        serde_json::json!({"path":"textures/blocks/side", "overlay_color":"green"}),
        serde_json::json!({"path":"textures/blocks/side", "overlay_color":"#abcd"}),
        serde_json::json!({"path":"textures/blocks/side", "overlay_color":"#79c05z"}),
        serde_json::json!({"path":"textures/blocks/side", "overlay_color":"#79c05a", "unknown":true}),
    ] {
        let pack = grass_pack(side);
        let entity = compile_entity_assets(pack.path(), MANIFEST).unwrap();
        let (source, visual) = grass_world(&entity);
        let compiled =
            compile_icon_assets_with_blocks(pack.path(), MANIFEST, &runtime(&source)).unwrap();
        let catalog = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
        assert!(catalog.lookup("minecraft:grass_block", 0).is_none());
        assert!(
            !catalog
                .block_sheets()
                .iter()
                .any(|sheet| sheet.visual == visual)
        );
    }
}

#[test]
fn carried_overlay_does_not_grant_held_cube_geometry_to_noncube_shapes() {
    let pack = grass_pack(serde_json::json!({
        "path":"textures/blocks/side", "overlay_color":"#79c05a"
    }));
    let entity = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let (mut source, visual) = grass_world(&entity);
    source.visuals[visual.0 as usize].flags = BlockFlags::empty();
    source.visuals[visual.0 as usize].kind = VisualKind::Invisible;
    let compiled =
        compile_icon_assets_with_blocks(pack.path(), MANIFEST, &runtime(&source)).unwrap();
    let catalog = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
    assert!(
        !catalog
            .block_sheets()
            .iter()
            .any(|sheet| sheet.visual == visual)
    );
}

#[test]
fn native_cube_sheets_follow_shape_and_preserve_carried_alpha() {
    let pack = pack();
    let cases = [
        (
            "black_wool",
            BlockFlags::CUBE_GEOMETRY
                | BlockFlags::OCCLUDES_FULL_FACE
                | BlockFlags::FIRE_FLAMMABLE
                | BlockFlags::FIRE_TOP_SUPPORT,
            0,
            [30, 30, 30, 255],
            false,
        ),
        (
            "ice",
            BlockFlags::empty(),
            MATERIAL_FLAG_ALPHA_BLEND,
            [150, 180, 255, 96],
            false,
        ),
        (
            "oak_leaves",
            BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL | BlockFlags::FIRE_FLAMMABLE,
            MATERIAL_FLAG_ALPHA_CUTOUT
                | MATERIAL_FLAG_FOLIAGE_TINT
                | MATERIAL_FLAG_SEASONAL_FOLIAGE
                | MATERIAL_FLAG_EXPOSED_FOLIAGE
                | MATERIAL_FLAG_TWO_SIDED
                | MATERIAL_FLAG_NATIVE_LEAF_COLOUR
                | MATERIAL_FLAG_ISOTROPIC,
            [70, 130, 40, 255],
            true,
        ),
    ];
    let mut blocks = serde_json::Map::new();
    let mut terrain = serde_json::Map::new();
    for (name, _, _, pixel, carried) in cases {
        let mut block = serde_json::json!({"textures":name});
        if carried {
            block["carried_textures"] = format!("{name}_carried").into();
            terrain.insert(
                format!("{name}_carried"),
                serde_json::json!({"textures":format!("textures/blocks/{name}")}),
            );
        }
        blocks.insert(name.into(), block);
        terrain.insert(
            name.into(),
            serde_json::json!({"textures":format!("textures/blocks/{name}")}),
        );
        let mut pixels = pixel.repeat((TILE_SIZE as usize).pow(2));
        if carried {
            pixels[3] = 0;
        }
        let path = pack.path().join(format!("textures/blocks/{name}.png"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        image::save_buffer(path, &pixels, TILE_SIZE, TILE_SIZE, image::ColorType::Rgba8).unwrap();
    }
    write(
        pack.path(),
        "blocks.json",
        &serde_json::to_vec(&blocks).unwrap(),
    );
    write(
        pack.path(),
        "textures/terrain_texture.json",
        &serde_json::to_vec(&serde_json::json!({"texture_data":terrain})).unwrap(),
    );
    write(pack.path(), "textures/flipbook_textures.json", b"[]");
    let entity = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let mut source = world(&entity);
    let mut bindings = Vec::new();
    for (name, flags, material_flags, pixel, carried) in cases {
        let identifier = format!("minecraft:{name}");
        let ItemVisualDefinitionRoute::BlockItem { block_visual } = entity
            .item_visuals
            .iter()
            .find(|item| item.key.identifier.as_ref() == identifier)
            .unwrap()
            .route
        else {
            panic!("native cube requires a block item route");
        };
        let mut materials = source.materials.to_vec();
        let material = materials.len() as u32;
        materials.push(Material {
            flags: material_flags,
            ..materials[1]
        });
        source.materials = materials.into();
        let template = if name == "ice" {
            transparent_cube_template(&mut source, material)
        } else {
            NO_MODEL_TEMPLATE
        };
        source.visuals[block_visual.0 as usize] = BlockVisual {
            faces: [material; 6],
            flags,
            kind: if template == NO_MODEL_TEMPLATE {
                VisualKind::Cube
            } else {
                VisualKind::Model
            },
            support: VisualSupport::Exact,
            contributor_role: ContributorRole::Primary,
            model_template: template,
            animation: NO_ANIMATION,
            variant: 0,
        };
        bindings.push((identifier, block_visual, pixel, carried));
    }
    let compiled =
        compile_icon_assets_with_blocks(pack.path(), MANIFEST, &runtime(&source)).unwrap();
    let catalog = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
    for (name, visual, pixel, cutout) in bindings {
        let binding = catalog
            .block_sheets()
            .iter()
            .find(|sheet| sheet.visual == visual)
            .unwrap_or_else(|| panic!("native cube {name} requires six carried faces"));
        let sheet = &catalog.sprites()[binding.sprite as usize];
        for face in BlockFace::ALL {
            assert_eq!(sheet_pixel(sheet, face, 1, 0), &pixel);
            if cutout {
                assert_eq!(sheet_pixel(sheet, face, 0, 0)[3], 0);
            }
        }
        let icon = catalog.lookup(&name, 0).unwrap();
        if pixel[3] < 255 {
            assert!(
                icon.rgba8
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] > 0 && pixel[3] < 255)
            );
            assert!(
                icon.rgba8
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| pixel[3] < 255)
            );
        }
    }
}
