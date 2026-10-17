//! Live terrain generations must replace held model geometry and pixels together.

use super::*;
use crate::prepared_actor_artwork::{PreparedActorArtwork, session_resources};

/// Encodes an original cuboid carrier with independently variable height, color and alpha mode.
fn terrain(height: i16, color: [u8; 4], flags: u32) -> Arc<RuntimeAssets> {
    let quads = assets::BlockFace::ALL.map(|face| {
        let (corners, uvs) = assets::gui_item::cube_face(face);
        let mut quad = assets::ModelQuad {
            positions: corners.map(|point| {
                [
                    (point[0] * 256.0) as i16,
                    (point[1] * f32::from(height)) as i16,
                    (point[2] * 256.0) as i16,
                ]
            }),
            uvs: uvs.map(|uv| uv.map(|value| (value * 4096.0) as u16)),
            material: 1,
            flags: face.model_quad_face_id(),
        };
        // Model cubes start each face at its smallest corner; thumbnail faces may rotate it.
        let start = quad
            .positions
            .iter()
            .enumerate()
            .min_by_key(|(_, point)| *point)
            .unwrap()
            .0;
        quad.positions.rotate_left(start);
        quad.uvs.rotate_left(start);
        quad
    });
    let visual = assets::BlockVisual {
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        faces: [1; 6],
        flags: if height == 256 {
            assets::BlockFlags::OCCLUDES_FULL_FACE
        } else {
            assets::BlockFlags::empty()
        },
        kind: assets::VisualKind::Model,
        support: assets::VisualSupport::Exact,
        contributor_role: assets::ContributorRole::Primary,
        model_template: 0,
        animation: assets::NO_ANIMATION,
        variant: 0,
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
    };
    let compiled = assets::CompiledAssets {
        visuals: vec![visual; 8].into(),
        light_properties: vec![assets::LightProperties::OPAQUE_DARK; 8].into(),
        hashed: Box::new([]),
        materials: vec![
            assets::Material::unvaried(),
            assets::Material {
                texture: assets::TextureRef::new(0, 1).unwrap(),
                flags,
                ..assets::Material::unvaried()
            },
        ]
        .into(),
        model_templates: vec![assets::ModelTemplate {
            quad_start: 0,
            quad_count: quads.len() as u32,
            flags: 0,
        }]
        .into(),
        model_quads: quads.into(),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![assets::TexturePage::new(assets::TextureArray {
            layers: 2,
            mips: [16, 8, 4, 2, 1]
                .into_iter()
                .map(|size| assets::TextureMip {
                    size,
                    rgba8: color.repeat((size * size * 2) as usize).into(),
                })
                .collect(),
        })]
        .into(),
        biomes: assets::CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: [2; 32],
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    };
    Arc::new(RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap())
}

/// Binds the existing player's held fixture to a genuine inventory model route.
fn fixture(
    world: Arc<RuntimeAssets>,
) -> (
    EquipmentRuntime,
    ActorArtworkPages,
    ActorRigSubmission,
    WornItem,
) {
    fixture_with_icons(world, None)
}

/// Builds the same item with an optional authored carried sheet distinct from its inventory icon.
fn fixture_with_icons(
    world: Arc<RuntimeAssets>,
    carried: Option<([u8; 32], [u8; 4])>,
) -> (
    EquipmentRuntime,
    ActorArtworkPages,
    ActorRigSubmission,
    WornItem,
) {
    let definition = assets::ItemVisualDefinition {
        key: assets::ItemVisualKey {
            identifier: "test:opaque_cube".into(),
            metadata: 0,
        },
        source: 0,
        route: assets::ItemVisualDefinitionRoute::BlockItem {
            block_visual: assets::BlockVisualId(7),
        },
        grip: assets::item::grip::ItemGrip::Unresolved,
        glint: assets::item::glint::ItemGlint::Unresolved,
        fuel: assets::item::fuel::ItemFuel::Unresolved,
        armour: assets::item::armour::ItemArmour::Unresolved,
        durability: assets::item::durability::ItemDurability::Unresolved,
        first_person: assets::ItemDisplayTransform::identity(),
        third_person: assets::ItemDisplayTransform::identity(),
        dropped: assets::ItemDisplayTransform::identity(),
    };
    let (base, body, item) = block_fixture_with_visuals(vec![definition]);
    let icons = RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_blocks(
            carried.map_or([1; 32], |(manifest, _)| manifest),
            &std::iter::once(IconSprite {
                width: 1,
                height: 1,
                rgba8: vec![255; 4].into(),
            })
            .chain(carried.map(|(_, color)| {
                let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE;
                IconSprite {
                    width,
                    height,
                    rgba8: color
                        .repeat(usize::from(width) * usize::from(height))
                        .into(),
                }
            }))
            .collect::<Vec<_>>(),
            &[assets::IconEntry {
                identifier: item.identifier.as_ref().into(),
                metadata: 0,
                sprite: 0,
            }],
            &carried
                .into_iter()
                .flat_map(|_| {
                    (6..=7).map(|visual| assets::IconBlockSheet {
                        sprite: 1,
                        visual: assets::BlockVisualId(visual),
                    })
                })
                .collect::<Vec<_>>(),
            &[assets::IconBlockModel {
                sprite: 0,
                visual: assets::BlockVisualId(7),
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let (mut runtime, pages, _) = EquipmentRuntime::build(
        Arc::clone(&base.assets),
        None,
        Arc::new(icons),
        Some(world),
        None,
        ActorArtworkPages::default(),
    );
    runtime.register_skin_rig(body.input.rig, vec!["rightItem".into()]);
    (runtime, pages, body, item)
}

/// Reads the complete held sheet through its published placement, excluding atlas padding.
fn held_sheet(runtime: &EquipmentRuntime, pages: &ActorArtworkPages) -> Vec<u8> {
    let (_, placement, location) = runtime.block_placement(7).expect("held sheet is admitted");
    let page = &pages.pages()[usize::from(location.page()) - 1];
    let (width, height) = page.dimensions();
    (0..usize::from(placement.height))
        .flat_map(|row| {
            let start = ((location.layer() as usize * usize::from(height)
                + usize::from(placement.y)
                + row)
                * usize::from(width)
                + usize::from(placement.x))
                * 4;
            page.pixels()[start..start + usize::from(placement.width) * 4]
                .iter()
                .copied()
        })
        .collect()
}

#[test]
fn held_terrain_reload_matches_fresh_pack_sheets() {
    for height in [128, 256] {
        for (physical, grid, exposed) in [(32u32, 1, 16), (64, 2, 16), (64, 1, 32), (64, 0, 16)] {
            let startup = terrain(height, [20, 80, 200, 255], 0);
            let (mut runtime, pages, _, _) = fixture(Arc::clone(&startup));
            let overlay = assets::BlockOverlay {
                materials: vec![
                    assets::Material::unvaried(),
                    assets::Material {
                        texture: assets::TextureRef::new(1, 0).unwrap(),
                        ..assets::Material::unvaried()
                    },
                ],
                textures: vec![assets::TextureArray {
                    layers: 1,
                    mips: (0..=physical.ilog2())
                        .map(|level| {
                            let side = physical >> level;
                            assets::TextureMip {
                                size: side,
                                rgba8: (0..side * side)
                                    .flat_map(|pixel| {
                                        [
                                            (pixel % side * 3) as u8,
                                            (pixel / side * 3) as u8,
                                            level as u8 * 30,
                                            255,
                                        ]
                                    })
                                    .collect(),
                            }
                        })
                        .collect(),
                }],
                texture_source_sizes: vec![[exposed; 2]],
                texture_source_grids: vec![grid],
                material_overrides: vec![assets::MaterialOverride {
                    material: 1,
                    texture: assets::TextureRef::new(1, 0).unwrap(),
                    animation: assets::NO_ANIMATION,
                    variation: None,
                }],
                ..Default::default()
            };
            let expected =
                render_model::equipment::blocks::overlay_face_sheet(&overlay, [1; 6], None)
                    .expect("pack face preparation accepts the exposed source rectangle");
            let replacement = Arc::new(
                startup
                    .with_block_overlay(startup.visual_count() as u32, &overlay)
                    .unwrap(),
            );
            let (fresh, fresh_pages, _, _) = fixture(Arc::clone(&replacement));
            let blocks = runtime
                .block_item_preparer()
                .unwrap()
                .prepare(&replacement)
                .unwrap();
            let prepared =
                PreparedActorArtwork::new_with_block_items(&pages, None, None, Some(blocks));
            let resources = session_resources(&pages, None, Some(&prepared));
            runtime.set_block_items(resources.block_items.as_ref());
            let reloaded = held_sheet(&runtime, &resources.pages);
            assert_eq!(
                reloaded,
                held_sheet(&fresh, &fresh_pages),
                "reload must match a fresh build byte for byte"
            );
            assert!(
                reloaded == expected.rgba8.as_ref(),
                "held sheet must use the pack's exposed pixels: height={height}, physical={physical}, grid={grid}"
            );
        }
    }
}

#[test]
fn held_terrain_reload_preserves_authored_carried_sheet_routes() {
    for flags in [0, assets::MATERIAL_FLAG_GRASS_TINT] {
        let carried = [30, 180, 70, 255];
        let startup = terrain(256, [90, 110, 50, 255], flags);
        assert!(
            assets::gui_item::full_cube_model_quads(&startup, assets::BlockVisualId(7)).is_some()
        );
        let (mut runtime, pages, body, item) =
            fixture_with_icons(Arc::clone(&startup), Some(([1; 32], carried)));
        let before = runtime
            .first_person_item(&body, &item, ItemAnimationState::default())
            .unwrap();
        let original = runtime.take_pending_geometries().pop().unwrap();
        assert_eq!(
            drawn_pixel(&pages, &before.presentation, &original),
            carried
        );
        assert_eq!(runtime.block_placement(6), runtime.block_placement(7));
        let replacement = terrain(256, [90, 110, 50, 255], flags);
        assert!(replacement.has_same_geometry(&startup));
        let blocks = runtime
            .block_item_preparer()
            .unwrap()
            .prepare(&replacement)
            .unwrap();
        let prepared = PreparedActorArtwork::new_with_block_items(&pages, None, None, Some(blocks));
        let resources = session_resources(&pages, None, Some(&prepared));
        runtime.set_block_items(resources.block_items.as_ref());
        assert_eq!(
            runtime.block_placement(6),
            runtime.block_placement(7),
            "shared carried sheets keep one atlas placement"
        );
        let main = runtime
            .first_person_item(&body, &item, ItemAnimationState::default())
            .expect("a carrier refresh retains the authored held cube");
        let off = runtime.first_person_offhand(&body, &item).unwrap();
        let third = third_person(&mut runtime, &body, &item);
        let changed = runtime.take_pending_geometries().pop().unwrap();
        for draw in [&main.presentation, &off.presentation, &third] {
            assert_eq!(draw.submission.input.rig, changed.id);
            assert_eq!(drawn_pixel(&resources.pages, draw, &changed), carried);
        }
        runtime.set_block_items(None);
        let restored = runtime
            .first_person_item(&body, &item, ItemAnimationState::default())
            .unwrap();
        let geometry = runtime.take_pending_geometries().pop().unwrap();
        assert_eq!(
            drawn_pixel(&pages, &restored.presentation, &geometry),
            carried
        );
    }
}

#[test]
fn held_terrain_reload_keeps_shaped_models_and_rejects_incompatible_carried_art() {
    let blue = [20, 80, 200, 255];
    let green = [40, 200, 80, 255];
    let carried = [220, 50, 70, 255];
    for (height, manifest) in [(128, [1; 32]), (256, [9; 32])] {
        let startup = terrain(height, blue, 0);
        let (mut runtime, pages, body, item) =
            fixture_with_icons(startup, Some((manifest, carried)));
        let before = runtime
            .first_person_item(&body, &item, ItemAnimationState::default())
            .unwrap();
        let original = runtime.take_pending_geometries().pop().unwrap();
        assert_eq!(drawn_pixel(&pages, &before.presentation, &original), blue);
        let replacement = terrain(height, green, 0);
        let blocks = runtime
            .block_item_preparer()
            .unwrap()
            .prepare(&replacement)
            .unwrap();
        let prepared = PreparedActorArtwork::new_with_block_items(&pages, None, None, Some(blocks));
        let resources = session_resources(&pages, None, Some(&prepared));
        runtime.set_block_items(resources.block_items.as_ref());
        let main = runtime
            .first_person_item(&body, &item, ItemAnimationState::default())
            .unwrap();
        let changed = runtime.take_pending_geometries().pop().unwrap();
        assert_eq!(
            drawn_pixel(&resources.pages, &main.presentation, &changed),
            green
        );
        assert_eq!(
            changed
                .vertices
                .iter()
                .map(|v| v.position)
                .collect::<Vec<_>>(),
            original
                .vertices
                .iter()
                .map(|v| v.position)
                .collect::<Vec<_>>(),
        );
    }
}

/// Samples a texel inside the submitted model's first triangle on its actual artwork layer.
fn drawn_pixel(
    pages: &ActorArtworkPages,
    draw: &EquipmentPresentation,
    geometry: &ActorRigGeometry,
) -> [u8; 4] {
    let page = &pages.pages()[usize::from(draw.location.page()) - 1];
    let (width, height) = page.dimensions();
    let uv: [f32; 2] = std::array::from_fn(|axis| {
        geometry.vertices[..3]
            .iter()
            .map(|v| v.uv[axis])
            .sum::<f32>()
            / 3.0
    });
    let x = (uv[0] * f32::from(width)) as usize;
    let y = (uv[1] * f32::from(height)) as usize;
    let offset =
        ((draw.location.layer() as usize * usize::from(height) + y) * usize::from(width) + x) * 4;
    page.pixels()[offset..offset + 4].try_into().unwrap()
}

#[test]
fn held_terrain_reload_replaces_warmed_mesh_pixels_alpha_and_restores_startup() {
    let blue = [20, 80, 200, 255];
    let green = [40, 200, 80, 128];
    let startup = terrain(128, blue, 0);
    let (mut runtime, pages, body, item) = fixture(Arc::clone(&startup));
    let before = runtime
        .first_person_item(&body, &item, ItemAnimationState::default())
        .unwrap();
    let original = runtime.take_pending_geometries().pop().unwrap();
    assert_eq!(drawn_pixel(&pages, &before.presentation, &original), blue);
    let preparer = runtime.block_item_preparer().unwrap();
    assert!(preparer.prepare(&startup).is_none());
    let replacement = terrain(64, green, assets::MATERIAL_FLAG_ALPHA_BLEND);
    let blocks = preparer.prepare(&replacement).unwrap();
    let prepared =
        PreparedActorArtwork::new_with_block_items(&pages, None, None, Some(Arc::clone(&blocks)));
    assert!(prepared.block_items_for(&startup).is_none());
    assert!(prepared.block_items_for(&replacement).is_some());
    let resources = session_resources(&pages, None, Some(&prepared));
    assert!(matches!(resources, std::borrow::Cow::Borrowed(_)));
    runtime.set_block_items(resources.block_items.as_ref());
    let main = runtime
        .first_person_item(&body, &item, ItemAnimationState::default())
        .unwrap();
    let off = runtime.first_person_offhand(&body, &item).unwrap();
    let third = third_person(&mut runtime, &body, &item);
    let changed = runtime.take_pending_geometries().pop().unwrap();
    assert_ne!(changed.vertices, original.vertices);
    for draw in [&main.presentation, &off.presentation, &third] {
        assert_eq!(draw.submission.input.rig, changed.id);
        assert_eq!(drawn_pixel(&resources.pages, draw, &changed), green);
    }
    assert_eq!(main.alpha_mode, render_model::HandItemAlphaMode::Blend);
    assert_eq!(off.alpha_mode, main.alpha_mode);
    runtime.set_block_items(resources.block_items.as_ref());
    runtime
        .first_person_item(&body, &item, ItemAnimationState::default())
        .unwrap();
    assert!(runtime.take_pending_geometries().is_empty());

    runtime.set_block_items(None);
    let restored = runtime
        .first_person_item(&body, &item, ItemAnimationState::default())
        .unwrap();
    let geometry = runtime.take_pending_geometries().pop().unwrap();
    assert_eq!(geometry.vertices, original.vertices);
    assert_eq!(drawn_pixel(&pages, &restored.presentation, &geometry), blue);
    assert_eq!(restored.alpha_mode, render_model::HandItemAlphaMode::Opaque);
    for _ in 0..MAX_ITEM_MESHES + 1 {
        for source in [resources.block_items.as_ref(), None] {
            runtime.set_block_items(source);
            runtime
                .first_person_item(&body, &item, ItemAnimationState::default())
                .unwrap();
            runtime.take_pending_geometries();
        }
    }
}
