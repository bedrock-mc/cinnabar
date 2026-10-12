use super::*;
use assets::{ContributorRole, NO_MODEL_TEMPLATE, TextureRef};

fn overlay(flags: BlockFlags, material_flags: u32, size: u32) -> BlockOverlay {
    let mut mips = Vec::new();
    let mut side = size;
    loop {
        let mut pixels = vec![0; side as usize * side as usize * 4 * 2];
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.copy_from_slice(&[40, 80, 120, 128]);
        }
        mips.push(TextureMip {
            size: side,
            rgba8: pixels.into(),
        });
        if side == 1 {
            break;
        }
        side /= 2;
    }
    BlockOverlay {
        visuals: vec![assets::BlockVisual {
            native_material: assets::native_material::NativeMaterialFacts::Unresolved,
            faces: [1; 6],
            flags,
            kind: VisualKind::Cube,
            support: VisualSupport::Exact,
            contributor_role: ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
            destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
        }],
        materials: vec![
            Material::unvaried(),
            Material {
                texture: TextureRef::new(1, 1).unwrap(),
                flags: material_flags,
                ..Material::unvaried()
            },
        ],
        texture: Some(TextureArray {
            layers: 2,
            mips: mips.into(),
        }),
        ..Default::default()
    }
}

#[test]
fn cube_sheets_ignore_gameplay_flags_and_terrain_occlusion() {
    for flags in [
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE | BlockFlags::FIRE_FLAMMABLE,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE | BlockFlags::FIRE_TOP_SUPPORT,
        BlockFlags::CUBE_GEOMETRY,
        BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
    ] {
        let sheet =
            overlay_sheet(&overlay(flags, 0, 16), 0).expect("cube shape determines held geometry");
        assert_eq!([sheet.width, sheet.height], assets::BLOCK_ITEM_SHEET_SIZE);
    }
}

#[test]
fn cube_sheets_preserve_blended_and_cutout_face_pixels() {
    for flags in [
        assets::MATERIAL_FLAG_ALPHA_BLEND,
        assets::MATERIAL_FLAG_ALPHA_CUTOUT,
    ] {
        let sheet = overlay_sheet(&overlay(BlockFlags::CUBE_GEOMETRY, flags, 32), 0)
            .expect("terrain alpha does not turn a held cube into a sprite");
        assert!(
            sheet
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [40, 80, 120, 128])
        );
    }
}

#[test]
fn world_isotropy_preserves_carried_cube_face_pixels() {
    for flags in [
        0,
        assets::MATERIAL_FLAG_ALPHA_BLEND,
        assets::MATERIAL_FLAG_ALPHA_CUTOUT,
    ] {
        let mut source = overlay(
            BlockFlags::CUBE_GEOMETRY,
            flags,
            BLOCK_ITEM_FACE_SIDE.into(),
        );
        source.texture.as_mut().unwrap().mips[0].rgba8[TILE * TILE * 4] = 91;
        let expected = overlay_sheet(&source, 0).unwrap();
        source.materials[1].flags |= assets::MATERIAL_FLAG_ISOTROPIC;
        let actual = overlay_sheet(&source, 0)
            .expect("world-position rotation preserves carried face admission");
        assert_eq!(actual.rgba8, expected.rgba8);
        source.materials[1].flags |= assets::MATERIAL_FLAG_FOLIAGE_TINT;
        assert!(overlay_sheet(&source, 0).is_none());
    }
}

#[test]
fn unresolved_tint_and_non_cube_shapes_do_not_create_cube_sheets() {
    assert!(
        overlay_sheet(
            &overlay(
                BlockFlags::CUBE_GEOMETRY,
                assets::MATERIAL_FLAG_FOLIAGE_TINT,
                16
            ),
            0
        )
        .is_none()
    );
    let mut model = overlay(BlockFlags::empty(), 0, 16);
    model.visuals[0].kind = VisualKind::Model;
    assert!(overlay_sheet(&model, 0).is_none());
}

#[test]
fn transparent_cube_templates_keep_cube_geometry() {
    let mut ice = overlay(BlockFlags::empty(), assets::MATERIAL_FLAG_ALPHA_BLEND, 16);
    ice.visuals[0].kind = VisualKind::Model;
    ice.visuals[0].model_template = 0;
    ice.model_templates.push(assets::ModelTemplate {
        quad_start: 0,
        quad_count: 6,
        flags: assets::MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE,
    });
    let sheet = overlay_sheet(&ice, 0).expect("transparent cube is held as a block");
    assert_eq!(&sheet.rgba8[..4], &[40, 80, 120, 128]);
    ice.model_templates[0].flags = assets::MODEL_TEMPLATE_FLAG_STAIR;
    assert!(overlay_sheet(&ice, 0).is_none());
}

#[test]
fn pinned_cube_items_have_sheets_when_carriers_are_available() {
    let (Ok(world_path), Ok(entity_path)) = (
        std::env::var("PINNED_WORLD_CARRIER"),
        std::env::var("PINNED_ENTITY_CARRIER"),
    ) else {
        eprintln!("missing fixture: PINNED_WORLD_CARRIER and PINNED_ENTITY_CARRIER");
        return;
    };
    let world = RuntimeAssets::decode(&std::fs::read(world_path).unwrap()).unwrap();
    let entities = RuntimeEntityAssets::decode(&std::fs::read(entity_path).unwrap()).unwrap();
    let sheets = collect(&world, &entities);
    for name in [
        "black_wool",
        "white_wool",
        "ice",
        "glass",
        "oak_planks",
        "stone",
    ] {
        let identifier = format!("minecraft:{name}");
        let definition = entities
            .item_visuals()
            .iter()
            .find(|entry| entry.key.identifier.as_ref() == identifier)
            .unwrap();
        let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
            panic!("{identifier} has no block route");
        };
        assert!(
            sheets.by_visual.contains_key(&block_visual.0),
            "{identifier} must retain its held cube"
        );
    }
    eprintln!(
        "{} cube visuals share {} sheets",
        sheets.by_visual.len(),
        sheets.sheets.len()
    );
}

#[test]
fn pinned_portal_frame_keeps_its_short_block_mesh_and_distinct_faces() {
    let (Ok(world_path), Ok(pack_root)) = (
        std::env::var("PINNED_WORLD_CARRIER"),
        std::env::var("CINNABAR_VANILLA_RESOURCE_PACK"),
    ) else {
        eprintln!(
            "missing portal-frame fixture: PINNED_WORLD_CARRIER and CINNABAR_VANILLA_RESOURCE_PACK"
        );
        return;
    };
    let world = RuntimeAssets::decode(&std::fs::read(world_path).unwrap()).unwrap();
    let entities = RuntimeEntityAssets::from_compiled(
        pack_compiler::compile_entity_assets(
            std::path::Path::new(&pack_root),
            assets::VANILLA_SOURCE_MANIFEST.as_bytes(),
        )
        .unwrap(),
    )
    .unwrap();
    let definition = entities
        .item_visuals()
        .iter()
        .find(|entry| entry.key.identifier.as_ref() == assets::END_PORTAL_FRAME_IDENTIFIER)
        .unwrap();
    let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
        panic!("portal frame requires a block route");
    };
    let sheets = collect(&world, &entities);
    let vertices = sheets
        .models
        .get(&block_visual.0)
        .expect("the held portal frame must retain its compiled block model");
    assert_eq!(vertices.len(), 36);
    let min = vertices
        .iter()
        .map(|vertex| vertex.position[1])
        .fold(f32::INFINITY, f32::min);
    let max = vertices
        .iter()
        .map(|vertex| vertex.position[1])
        .fold(f32::NEG_INFINITY, f32::max);
    assert_eq!(min, -0.5);
    assert_eq!(max, 13.0 / 16.0 - 0.5);
    for triangle in vertices.as_chunks::<3>().0 {
        let positions = triangle
            .iter()
            .map(|vertex| glam::Vec3::from_array(vertex.position))
            .collect::<Vec<_>>();
        assert!(
            (positions[1] - positions[0])
                .cross(positions[2] - positions[0])
                .dot(glam::Vec3::from_array(triangle[0].normal))
                > 0.0
        );
    }
    let sheet = &sheets.sheets[sheets.by_visual[&block_visual.0]];
    let tile_color = |face: BlockFace| {
        let rect = face_rects([0.0, 0.0, 1.0, 1.0])[face as usize];
        let x = (((rect[0] + rect[2]) * 0.5) * f32::from(sheet.width)) as usize;
        let y = (((rect[1] + rect[3]) * 0.5) * f32::from(sheet.height)) as usize;
        &sheet.rgba8[(y * usize::from(sheet.width) + x) * 4..][..4]
    };
    assert_ne!(tile_color(BlockFace::Up), tile_color(BlockFace::Down));
}

#[test]
fn overlay_sheet_preserves_source_resolution_when_physical_page_is_larger() {
    let mut source = overlay(BlockFlags::CUBE_GEOMETRY, 0, 64);
    source.texture_source_sizes = vec![[16; 2]; 2];
    let texture = source.texture.as_mut().unwrap();
    for mip in &mut texture.mips {
        for layer in 0..2usize {
            let side = mip.size as usize;
            for (pixel, rgba) in mip.rgba8[layer * side * side * 4..][..side * side * 4]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .enumerate()
            {
                rgba.copy_from_slice(if side == 64 {
                    if (pixel % side / 4).is_multiple_of(2) {
                        &[255, 0, 0, 255]
                    } else {
                        &[0, 0, 255, 255]
                    }
                } else {
                    &[127, 0, 127, 255]
                });
            }
        }
    }
    let sheet = overlay_sheet(&source, 0).unwrap();
    assert_eq!(
        &sheet.rgba8[..4],
        &[255, 0, 0, 255],
        "held texture preserves original source detail"
    );
    assert_eq!(&sheet.rgba8[4..8], &[0, 0, 255, 255]);
}

/// Binds original barrel item definitions to compiled world states, without carried sheets.
fn barrel_entities(
    world: &RuntimeAssets,
    records: &[assets::RegistryRecord],
) -> RuntimeEntityAssets {
    RuntimeEntityAssets::from_compiled(
        assets::CompiledEntityAssets {
            source_manifest_sha256: world.provenance().source_manifest_sha256,
            block_visual_count: world.visual_count() as u32,
            sources: vec![assets::EntityAssetSource {
                path: "entity/fixture.entity.json".into(),
                source_bytes: 2,
                source_sha256: [1; 32],
            }]
            .into(),
            symbols: vec![assets::EntityAssetSymbol {
                mount_equipment: assets::entity::mount::MountEquipment::Unresolved,
                kind: assets::EntityAssetKind::Entity,
                identifier: "test:fixture".into(),
                source_index: 0,
                dependencies: Box::new([]),
            }]
            .into(),
            geometries: Box::new([]),
            animation_clips: Box::new([]),
            animation_channels: Box::new([]),
            animation_keyframes: Box::new([]),
            molang_symbols: Box::new([]),
            molang_expressions: Box::new([]),
            molang_ops: Box::new([]),
            molang_collections: Box::new([]),
            molang_collection_items: Box::new([]),
            controllers: Box::new([]),
            controller_states: Box::new([]),
            controller_animations: Box::new([]),
            controller_transitions: Box::new([]),
            rig_bindings: Box::new([]),
            rig_geometries: Box::new([]),
            rig_animations: Box::new([]),
            rig_controllers: Box::new([]),
            item_visual_aliases: Box::new([]),
            interaction_traits: Box::new([]),
            item_use_profiles: Box::new([]),
            seat_layouts: Box::new([]),
            item_visuals: records
                .iter()
                .filter(|r| r.name.as_ref() == "minecraft:barrel")
                .enumerate()
                .map(|(metadata, record)| assets::ItemVisualDefinition {
                    key: assets::ItemVisualKey {
                        identifier: "minecraft:barrel".into(),
                        metadata: metadata as u32,
                    },
                    source: 0,
                    route: ItemVisualDefinitionRoute::BlockItem {
                        block_visual: assets::BlockVisualId(record.sequential_id),
                    },
                    grip: assets::item::grip::ItemGrip::Unresolved,
                    glint: assets::item::glint::ItemGlint::Unresolved,
                    fuel: assets::item::fuel::ItemFuel::Unresolved,
                    armour: assets::item::armour::ItemArmour::Unresolved,
                    durability: assets::item::durability::ItemDurability::Unresolved,
                    first_person: assets::ItemDisplayTransform::identity(),
                    third_person: assets::ItemDisplayTransform::identity(),
                    dropped: assets::ItemDisplayTransform::identity(),
                })
                .collect(),
            render: Default::default(),
        }
        .with_unresolved_item_use()
        .with_unresolved_seats()
        .with_unresolved_interactions(),
    )
    .unwrap()
}

#[test]
fn barrel_equipment_fallback_preserves_normalized_faces_and_model_uvs() {
    let (compiled, records) = crate::viewmodel::cube::tests::barrels_with_size(32);
    let world = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let entities = barrel_entities(&world, &records);
    let sheets = collect(&world, &entities);
    assert_eq!(
        sheets.by_visual.len(),
        12,
        "32-pixel barrels retain cube equipment without authored carried sheets"
    );
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:barrel")
    {
        let visual = record.sequential_id;
        let vertices = sheets
            .models
            .get(&visual)
            .expect("equipment retains directional model UVs");
        let quads =
            assets::gui_item::full_cube_model_quads(&world, assets::BlockVisualId(visual)).unwrap();
        assert_eq!(vertices.len(), 36);
        for (face, (quad, vertices)) in quads.iter().zip(vertices.chunks_exact(6)).enumerate() {
            let rect = face_rects([0., 0., 1., 1.])[face];
            for (corner, vertex) in [0, 1, 2, 0, 2, 3].into_iter().zip(vertices) {
                assert_eq!(
                    vertex.position,
                    quad.positions[corner].map(|p| f32::from(p) / 256. - 0.5)
                );
                let expected = std::array::from_fn::<_, 2, _>(|axis| {
                    rect[axis]
                        + f32::from(quad.uvs[corner][axis]) / 4096. * (rect[axis + 2] - rect[axis])
                });
                assert_eq!(vertex.uv, expected);
            }
        }
        let sheet = &sheets.sheets[sheets.by_visual[&visual]];
        assert_eq!([sheet.width, sheet.height], assets::BLOCK_ITEM_SHEET_SIZE);
        assert!(
            sheet
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[0] == 20 && p[3] == 255)
        );
    }
}

#[test]
fn barrel_equipment_prefers_matching_authored_carried_faces() {
    let (compiled, records) = crate::viewmodel::cube::tests::barrels();
    let world = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let entities = barrel_entities(&world, &records);
    let visual = entities.item_visuals()[0].route;
    let ItemVisualDefinitionRoute::BlockItem { block_visual } = visual else {
        panic!("barrel block route");
    };
    let tiles = std::array::from_fn(|face| IconSprite {
        width: BLOCK_ITEM_FACE_SIDE,
        height: BLOCK_ITEM_FACE_SIDE,
        rgba8: [91 + face as u8, 40, 80, 255].repeat(TILE * TILE).into(),
    });
    let sprite = assets::compose_block_item_sheet(&tiles).unwrap();
    for matches in [false, true] {
        let source = if matches {
            world.provenance().source_manifest_sha256
        } else {
            [8; 32]
        };
        let icons = assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog_with_block_sheets(
                source,
                std::slice::from_ref(&sprite),
                &[],
                &[assets::IconBlockSheet {
                    visual: block_visual,
                    sprite: 0,
                }],
            )
            .unwrap(),
        )
        .unwrap();
        let mut sheets = collect(&world, &entities);
        assert_eq!(sheets.models.len(), 12);
        sheets.prefer_carried_faces(&world, &icons);
        assert_eq!(sheets.models.contains_key(&block_visual.0), !matches);
        assert_eq!(sheets.models.len(), if matches { 11 } else { 12 });
    }
}
