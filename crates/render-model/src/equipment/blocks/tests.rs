use super::*;
use assets::{ContributorRole, NO_MODEL_TEMPLATE, TextureMip, TextureRef};

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
        textures: vec![TextureArray {
            layers: 2,
            mips: mips.into(),
        }],
        ..Default::default()
    }
}

#[test]
fn multi_page_overlay_face_sheets_reserve_all_base_pages() {
    let mut source = overlay(BlockFlags::CUBE_GEOMETRY, 0, 16);
    let expected = overlay_face_sheet(&source, [1; 6], None).unwrap();
    source.base_texture_pages = 2;
    source.materials[1].texture = TextureRef::new(2, 1).unwrap();
    let actual = overlay_face_sheet(&source, [1; 6], None).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn carried_face_sheets_sample_every_overlay_shard() {
    let mut source = overlay(BlockFlags::CUBE_GEOMETRY, 0, 16);
    for page in 2..assets::MAX_TEXTURE_PAGES as u32 {
        let color = [40, page as u8, 120, 128];
        let mut shard = overlay(BlockFlags::CUBE_GEOMETRY, 0, 32)
            .textures
            .into_iter()
            .next()
            .unwrap();
        for mip in &mut shard.mips {
            for pixel in mip.rgba8.as_chunks_mut::<4>().0 {
                pixel.copy_from_slice(&color);
            }
        }
        source.textures.push(shard);
        source.materials[1].texture = TextureRef::new(page, 1).unwrap();
        let faces = overlay_face_sheet(&source, [1; 6], None)
            .expect("a carried face may reference any available overlay shard");
        let visual = overlay_sheet(&source, 0, None).unwrap();
        assert_eq!(faces.rgba8, visual.rgba8);
        assert_eq!([faces.width, faces.height], assets::BLOCK_ITEM_SHEET_SIZE);
        assert!(
            faces
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == color)
        );
    }
}

#[test]
fn carried_face_sheets_reject_diagnostic_and_unavailable_overlay_references() {
    let mut source = overlay(BlockFlags::CUBE_GEOMETRY, 0, 16);
    assert!(overlay_face_sheet(&source, [DIAGNOSTIC_MATERIAL; 6], None).is_none());
    for reference in [
        TextureRef::new(0, 1).unwrap(),
        TextureRef::new(2, 1).unwrap(),
        TextureRef::new(1, source.textures[0].layers).unwrap(),
    ] {
        source.materials[1].texture = reference;
        assert!(overlay_face_sheet(&source, [1; 6], None).is_none());
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
        let sheet = overlay_sheet(&overlay(flags, 0, 16), 0, None)
            .expect("cube shape determines held geometry");
        assert_eq!([sheet.width, sheet.height], assets::BLOCK_ITEM_SHEET_SIZE);
    }
}

#[test]
fn cube_sheets_preserve_blended_and_cutout_face_pixels() {
    for flags in [
        assets::MATERIAL_FLAG_ALPHA_BLEND,
        assets::MATERIAL_FLAG_ALPHA_CUTOUT,
        assets::MATERIAL_FLAG_ALPHA_CUTOUT
            | assets::MATERIAL_FLAG_DISABLE_AO
            | assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING,
    ] {
        let sheet = overlay_sheet(&overlay(BlockFlags::CUBE_GEOMETRY, flags, 32), 0, None)
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
        source.textures.first_mut().unwrap().mips[0].rgba8[TILE * TILE * 4] = 91;
        let expected = overlay_sheet(&source, 0, None).unwrap();
        source.materials[1].flags |= assets::MATERIAL_FLAG_ISOTROPIC;
        let actual = overlay_sheet(&source, 0, None)
            .expect("world-position rotation preserves carried face admission");
        assert_eq!(actual.rgba8, expected.rgba8);
        source.materials[1].flags |= assets::MATERIAL_FLAG_FOLIAGE_TINT;
        assert!(overlay_sheet(&source, 0, None).is_none());
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
            0,
            None
        )
        .is_none()
    );
    let mut model = overlay(BlockFlags::empty(), 0, 16);
    model.visuals[0].kind = VisualKind::Model;
    assert!(overlay_sheet(&model, 0, None).is_none());
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
    let sheet = overlay_sheet(&ice, 0, None).expect("transparent cube is held as a block");
    assert_eq!(&sheet.rgba8[..4], &[40, 80, 120, 128]);
    ice.model_templates[0].flags = assets::MODEL_TEMPLATE_FLAG_STAIR;
    assert!(overlay_sheet(&ice, 0, None).is_none());
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
        pack_entity_compiler::compile_entity_assets(
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
    let texture = source.textures.first_mut().unwrap();
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
    let sheet = overlay_sheet(&source, 0, None).unwrap();
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
        for (face, (quad, vertices)) in quads
            .iter()
            .zip(vertices.as_chunks::<6>().0.iter())
            .enumerate()
        {
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

#[test]
fn weighted_replacement_updates_held_pixels_and_keeps_the_variation_range() {
    let (mut compiled, records) = crate::viewmodel::cube::tests::barrels();
    let visual = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:barrel")
        .unwrap()
        .sequential_id as usize;
    let representative = compiled.materials[compiled.visuals[visual].faces[0] as usize];
    let mut materials = compiled.materials.into_vec();
    let selector = materials.len() as u32;
    materials.push(Material {
        variation_start: selector + 1,
        variation_count: 2,
        ..representative
    });
    materials.extend([0.25_f32, 0.75_f32].map(|weight| Material {
        variation_weight: weight.to_bits(),
        ..representative
    }));
    compiled.materials = materials.into();
    let world = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let original = compose_sheet(&world, &[selector; 6]).unwrap();
    let replacement = BlockOverlay {
        textures: overlay(BlockFlags::CUBE_GEOMETRY, 0, 16).textures,
        material_overrides: (selector..selector + 3)
            .map(|material| assets::MaterialOverride {
                material,
                texture: TextureRef::new(1, 1).unwrap(),
                animation: NO_ANIMATION,
                variation: None,
            })
            .collect(),
        ..BlockOverlay::default()
    };
    let session = world
        .with_block_overlay(world.visual_count() as u32, &replacement)
        .unwrap();
    let held = compose_sheet(&session, &[selector; 6]).unwrap();
    assert!(
        held.rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [40, 80, 120, 128])
    );
    assert_ne!(held.rgba8, original.rgba8);
    let group = session.material(selector);
    assert_eq!(group.variation_start, selector + 1);
    assert_eq!(group.variation_count, 2);
    for (leaf, weight) in [0.25_f32, 0.75_f32].into_iter().enumerate() {
        let material = session.material(selector + 1 + leaf as u32);
        assert_eq!(material.texture, group.texture);
        assert_eq!(material.variation_weight, weight.to_bits());
    }
}

/// Pinned world, entity and icon carriers, or `None` with a named missing-fixture message.
fn pinned_carriers() -> Option<(
    RuntimeAssets,
    RuntimeEntityAssets,
    assets::RuntimeIconCatalog,
)> {
    let (Ok(world), Ok(entities), Ok(icons)) = (
        std::env::var("PINNED_WORLD_CARRIER"),
        std::env::var("PINNED_ENTITY_CARRIER"),
        std::env::var("PINNED_ICON_CARRIER"),
    ) else {
        eprintln!(
            "missing fixture: PINNED_WORLD_CARRIER, PINNED_ENTITY_CARRIER and PINNED_ICON_CARRIER"
        );
        return None;
    };
    Some((
        RuntimeAssets::decode(&std::fs::read(world).unwrap()).unwrap(),
        RuntimeEntityAssets::decode(&std::fs::read(entities).unwrap()).unwrap(),
        assets::RuntimeIconCatalog::decode(&std::fs::read(icons).unwrap()).unwrap(),
    ))
}

#[test]
fn pinned_non_cube_items_hold_their_inventory_model_and_flat_items_stay_sprites() {
    let Some((world, entities, icons)) = pinned_carriers() else {
        return;
    };
    let held = collect_held(&world, &entities, &icons);
    let slots = collect(&world, &entities);
    let visual = |name: &str| {
        let definition = entities
            .item_visuals()
            .iter()
            .find(|entry| entry.key.identifier.as_ref() == name)
            .unwrap();
        let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
            panic!("{name} has no block route");
        };
        block_visual.0
    };
    for name in [
        "minecraft:oak_stairs",
        "minecraft:oak_slab",
        "minecraft:oak_fence",
    ] {
        let vertices = held
            .models
            .get(&visual(name))
            .unwrap_or_else(|| panic!("{name} holds as its model"));
        assert!(!vertices.is_empty());
        assert!(
            vertices.iter().all(|vertex| vertex
                .position
                .iter()
                .all(|axis| (-0.5..=0.5).contains(axis))),
            "{name} stays within its block"
        );
        assert!(
            !slots.models.contains_key(&visual(name)),
            "{name}: slot cubes never take template sheets"
        );
    }
    for name in ["minecraft:torch", "minecraft:poppy"] {
        assert!(
            !held.models.contains_key(&visual(name)),
            "{name} is drawn flat in the inventory and stays a sprite in hand"
        );
    }
}

#[test]
fn pinned_end_rod_is_carried_as_a_three_dimensional_model() {
    let Some((world, entities, icons)) = pinned_carriers() else {
        return;
    };
    let visual = entities
        .item_visuals()
        .iter()
        .find(|entry| entry.key.identifier.as_ref() == "minecraft:end_rod")
        .and_then(|entry| match entry.route {
            ItemVisualDefinitionRoute::BlockItem { block_visual } => Some(block_visual.0),
            _ => None,
        })
        .expect("end rod has a reviewed block-item route");
    let held = collect_held(&world, &entities, &icons);
    let vertices = held
        .models
        .get(&visual)
        .expect("end rod keeps its non-cube inventory model in hand");
    assert_eq!(
        vertices.len(),
        72,
        "the shaft and mounting plate are both held"
    );
    let (min, max) = vertices.iter().fold(
        ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
        |(mut min, mut max), vertex| {
            for axis in 0..3 {
                min[axis] = min[axis].min(vertex.position[axis]);
                max[axis] = max[axis].max(vertex.position[axis]);
            }
            (min, max)
        },
    );
    assert_eq!(min, [-0.125, -0.5, -0.125]);
    assert_eq!(max, [0.125, 0.5, 0.125]);
}

#[test]
fn pinned_carried_shapes_keep_their_three_dimensional_held_models() {
    let Some((world, entities, icons)) = pinned_carriers() else {
        return;
    };
    for name in [
        assets::facts::pack::PackBlock::DriedGhast,
        assets::facts::pack::PackBlock::SculkSensor,
        assets::facts::pack::PackBlock::CalibratedSculkSensor,
        assets::facts::pack::PackBlock::SculkShrieker,
    ] {
        let visual = entities
            .item_visuals()
            .iter()
            .find(|entry| entry.key.identifier.as_ref() == name.identifier())
            .and_then(|entry| match entry.route {
                ItemVisualDefinitionRoute::BlockItem { block_visual } => Some(block_visual.0),
                _ => None,
            })
            .expect("registered dried ghast block item");
        let held = collect_held(&world, &entities, &icons);
        assert!(
            !held
                .models
                .get(&visual)
                .expect("carried thumbnail retains the held model")
                .is_empty()
        );
    }
}

#[test]
fn held_non_cube_admission_uses_the_inventory_state_and_preserves_its_height() {
    let (mut compiled, records) = crate::viewmodel::cube::tests::barrels();
    let visual = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:barrel")
        .unwrap()
        .sequential_id;
    let template = compiled.visuals[visual as usize].model_template as usize;
    compiled.visuals[visual as usize].flags = BlockFlags::empty();
    compiled.model_templates[template].flags = 0;
    let span = compiled.model_templates[template];
    for quad in &mut compiled.model_quads
        [span.quad_start as usize..(span.quad_start + span.quad_count) as usize]
    {
        for position in &mut quad.positions {
            position[1] /= 2;
        }
    }
    let world = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let entities = barrel_entities(&world, &records);
    let sprite = IconSprite {
        width: 16,
        height: 16,
        rgba8: [40, 80, 120, 255].repeat(16 * 16).into(),
    };
    let entry = assets::IconEntry {
        identifier: "minecraft:barrel".into(),
        metadata: 0,
        sprite: 0,
    };
    for (has_model, held_sprite, expected_model) in [
        (false, None, false),
        (true, None, true),
        (true, Some(0), false),
    ] {
        let bindings = has_model
            .then_some(assets::IconBlockModel {
                held_sprite,
                sprite: 0,
                visual: assets::BlockVisualId(visual),
            })
            .into_iter()
            .collect::<Vec<_>>();
        let icons = assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog_with_blocks(
                world.provenance().source_manifest_sha256,
                std::slice::from_ref(&sprite),
                std::slice::from_ref(&entry),
                &[],
                &bindings,
            )
            .unwrap(),
        )
        .unwrap();
        let held = collect_held(&world, &entities, &icons);
        assert_eq!(held.models.contains_key(&visual), expected_model);
        if let Some(vertices) = held.models.get(&visual) {
            let (min, max) = vertices
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), vertex| {
                    (min.min(vertex.position[1]), max.max(vertex.position[1]))
                });
            assert_eq!([min, max], [-0.5, 0.0]);
            assert!(vertices.iter().all(|vertex| {
                vertex
                    .uv
                    .iter()
                    .all(|component| (0.0..=1.0).contains(component))
            }));
            for triangle in vertices.as_chunks::<3>().0 {
                let [a, b, c] = triangle.map(|vertex| glam::Vec3::from_array(vertex.position));
                let normal = glam::Vec3::from_array(triangle[0].normal);
                assert!((b - a).cross(c - a).dot(normal) > 0.0);
            }
        }
        assert!(!collect(&world, &entities).models.contains_key(&visual));
    }
}

#[test]
fn custom_content_template_sheet_retains_authored_non_cube_geometry() {
    let mut source = overlay(BlockFlags::empty(), 0, 16);
    source.visuals[0].kind = VisualKind::Model;
    source.visuals[0].model_template = 0;
    source.model_templates.push(assets::ModelTemplate {
        quad_start: 0,
        quad_count: 1,
        flags: 0,
    });
    source.model_quads.push(assets::ModelQuad {
        positions: [[96, 0, 96], [96, 256, 96], [160, 256, 96], [160, 0, 96]],
        uvs: [[0, 4096], [0, 0], [4096, 0], [4096, 4096]],
        material: 1,
        flags: 0,
    });
    let (sheet, vertices) = overlay_item(&source, 0, None, &|_| None).expect("exact template");
    assert_eq!([sheet.width, sheet.height], assets::BLOCK_ITEM_SHEET_SIZE);
    let vertices = vertices.unwrap();
    assert_eq!(vertices.len(), 6);
    assert!(vertices.iter().all(|v| v.position[2] == -0.125));
    assert!(vertices.iter().all(|v| v.position[0].abs() == 0.125));
    assert!(vertices.iter().all(|v| v.position[1].abs() == 0.5));
    source.model_quads[0].uvs[0][0] = 8192;
    assert!(overlay_item(&source, 0, None, &|_| None).is_none());
}

#[test]
fn custom_content_dropped_models_keep_held_shape_and_sheet_uvs() {
    let mut vertices = crate::textured_cube_vertices(face_rects([0.0, 0.0, 1.0, 1.0]));
    for vertex in &mut vertices {
        vertex.position[0] *= 0.25;
        vertex.position[2] *= 0.25;
    }
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE.map(u32::from);
    let sheet = crate::DroppedItemSprite {
        width,
        height,
        rgba8: vec![255; (width * height * 4) as usize].into(),
    };
    let model = dropped_model(sheet, &vertices).unwrap();
    assert_eq!(model.quads.len(), 6);
    assert_eq!(model.materials.len(), assets::BlockFace::ALL.len());
    let dropped = crate::dropped_item::block_mesh(&model, 1, 64).unwrap();
    assert_eq!(vertices.len(), dropped.len());
    for held in &vertices {
        assert!(dropped.iter().any(|dropped| {
            held.position == dropped.position
                && held.normal == dropped.normal
                && (0..2).all(|axis| {
                    let tile = [width, height][axis] as f32
                        / f32::from(assets::BLOCK_ITEM_SHEET_GRID[axis]);
                    let local = (held.uv[axis] - held.glint_origin[axis])
                        * f32::from(assets::BLOCK_ITEM_SHEET_GRID[axis]);
                    let expected = local * tile / 64.0;
                    let glint = crate::ActorGlint::item(0.0, [1.0; 2]).uv_scale[axis];
                    (dropped.uv[axis] - expected).abs() < 0.0002
                        && (dropped.uv[axis] * dropped.glint_scale[axis] - local * glint).abs()
                            < 0.001
                })
        }));
    }
    assert!(dropped_model(model.materials[0].0.clone(), &vertices[..5]).is_none());
}

#[test]
fn custom_content_carried_tints_are_baked_without_changing_alpha() {
    let source = overlay(
        BlockFlags::CUBE_GEOMETRY,
        assets::MATERIAL_FLAG_GRASS_TINT,
        16,
    );
    assert!(overlay_item(&source, 0, None, &|_| None).is_none());
    let (sheet, model) = overlay_item(&source, 0, None, &|_| Some([255, 128, 0])).unwrap();
    assert!(model.is_none());
    assert!(
        sheet
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [40, 40, 0, 128])
    );
}
