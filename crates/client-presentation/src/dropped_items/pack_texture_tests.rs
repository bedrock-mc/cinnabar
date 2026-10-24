//! Pack replacement must reach dropped geometry even when atlas regions stay unchanged.
use super::*;
use bevy::{ecs::system::SystemState, prelude::World};
use client_ui::ui_runtime::{
    UiRuntime,
    presentation::{SessionIcon, SessionIcons},
};

const ITEM: &str = "minecraft:stone";

/// Creates original one-pixel base artwork and an ordinary dropped stack.
fn fixture(metadata: u32) -> (UiPresentationRuntime, WorldStream) {
    let catalog = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog(
            [5; 32],
            &[assets::IconSprite {
                width: 1,
                height: 1,
                rgba8: Arc::from([20, 30, 40, 255]),
            }],
            &[assets::IconEntry {
                identifier: ITEM.into(),
                metadata: 0,
                sprite: 0,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    fixture_with_catalog(metadata, catalog)
}

/// Spawns a retained block item with the supplied original startup artwork.
fn fixture_with_catalog(
    metadata: u32,
    catalog: assets::RuntimeIconCatalog,
) -> (UiPresentationRuntime, WorldStream) {
    let icons = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        client_ui::test_support::fixture_hud(),
        Arc::new(catalog),
    )
    .unwrap();
    let mut stream = stream(false, 1);
    let id = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == ITEM)
        .unwrap()
        .network_id;
    stream
        .submit(
            1,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 7,
                runtime_id: 7,
                kind: protocol::ActorKind::Entity {
                    identifier: "minecraft:item".into(),
                },
                position: [1.0, 2.0, 3.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: protocol::NetworkItemStack {
                    network_id: id,
                    metadata,
                    block_runtime_id: 1,
                    count: 1,
                    ..Default::default()
                },
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    (icons, stream)
}

/// Installs pack artwork through the same UI build boundary used by the client.
fn install(icons: &mut UiPresentationRuntime, color: Option<u8>, cube: bool, metadata: u32) {
    let mut runtime = UiRuntime::new(1);
    runtime.set_session_icons(color.map(|color| {
        let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE.map(u32::from);
        let sheet = SessionIcon {
            identifier: ITEM.into(),
            metadata,
            width,
            height,
            rgba8: (0..width * height)
                .flat_map(|pixel| {
                    let face = (pixel / width / u32::from(assets::BLOCK_ITEM_FACE_SIDE))
                        * u32::from(assets::BLOCK_ITEM_SHEET_GRID[0])
                        + (pixel % width / u32::from(assets::BLOCK_ITEM_FACE_SIDE));
                    [color, face as u8, 80, 255]
                })
                .collect::<Vec<_>>()
                .into(),
        };
        Arc::new(SessionIcons {
            icons: vec![SessionIcon {
                identifier: ITEM.into(),
                metadata,
                width: 1,
                height: 1,
                rgba8: Box::new([color, 0, 80, 255]),
            }],
            block_sheets: if cube { vec![sheet] } else { vec![] },
            ..Default::default()
        })
    }));
    icons
        .build(
            &player_state::PlayerState::new(1),
            &runtime,
            0,
            [800, 600],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
}

#[test]
fn dropped_pack_cubes_replace_all_faces_and_restore_base_after_removal() {
    let (mut icons, stream) = fixture(0);
    let mut world = World::new();
    world.init_resource::<DroppedItemScene>();
    let mut publisher = SystemState::<DroppedItemPublisher<'_, '_>>::new(&mut world);
    let mut previous = 0;
    for color in [Some(90), Some(160), None] {
        install(&mut icons, color, true, 0);
        publisher.get_mut(&mut world).unwrap().publish(
            Some(&icons),
            Some(&stream),
            None,
            (None, None),
            Default::default(),
            (None, Default::default()),
        );
        let scene = world.resource::<DroppedItemScene>();
        assert_eq!(scene.instances.len(), 1);
        assert!(scene.models_revision > previous);
        previous = scene.models_revision;
        let model = &scene.models[scene.instances[0].model as usize];
        match color {
            Some(color) => {
                let DroppedItemModel::Cube(cube) = model else {
                    panic!("pack block sheets must draw as cubes");
                };
                for (face, pixels) in cube.faces.iter().enumerate() {
                    assert!(
                        pixels
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .all(|pixel| *pixel == [color, face as u8, 80, 255])
                    );
                    assert_eq!(cube.tints[face], render_model::OPAQUE_WHITE);
                }
            }
            None => {
                let DroppedItemModel::NativeSprite(sprite) = model else {
                    panic!("removing a pack restores base artwork");
                };
                assert_eq!(sprite.rgba8.as_ref(), [20, 30, 40, 255]);
            }
        }
    }
}

#[test]
fn dropped_sprites_follow_pack_replacement_at_the_same_atlas_region() {
    let (mut icons, stream) = fixture(0);
    let mut world = World::new();
    world.init_resource::<DroppedItemScene>();
    let mut publisher = SystemState::<DroppedItemPublisher<'_, '_>>::new(&mut world);
    let mut region = None;
    for color in [90, 160] {
        install(&mut icons, Some(color), false, 0);
        let icon = icons.item_icon(ITEM, 0).unwrap();
        assert_eq!(*region.get_or_insert(icon), icon);
        publisher.get_mut(&mut world).unwrap().publish(
            Some(&icons),
            Some(&stream),
            None,
            (None, None),
            Default::default(),
            (None, Default::default()),
        );
        let scene = world.resource::<DroppedItemScene>();
        assert_eq!(scene.instances.len(), 1);
        let DroppedItemModel::NativeSprite(sprite) =
            &scene.models[scene.instances[0].model as usize]
        else {
            panic!("ordinary pack icons remain sprites");
        };
        assert_eq!(sprite.rgba8.as_ref(), [color, 0, 80, 255]);
    }
}

#[test]
fn framed_cube_faces_follow_pack_replacement_and_removal() {
    let (mut icons, stream) = fixture(0);
    let mut world = World::new();
    world.init_resource::<DroppedItemScene>();
    world.insert_resource(render_scene::StaticItemPlacementsResource(
        render_model::StaticItemPlacements(vec![render_model::StaticItemPlacement {
            identifier: ITEM.into(),
            metadata: 0,
            potted_block: None,
            carried_block: Some(assets::BlockVisualId(1)),
            world_from_item: render_model::dropped_item_transform([10.0, 2.0, 3.0], 0.0, 1.0),
            light: None,
        }]),
    ));
    let mut publisher = SystemState::<DroppedItemPublisher<'_, '_>>::new(&mut world);
    for color in [Some(90), Some(160), None] {
        install(&mut icons, color, true, 0);
        publisher.get_mut(&mut world).unwrap().publish(
            Some(&icons),
            Some(&stream),
            None,
            (None, None),
            Default::default(),
            (None, Default::default()),
        );
        let scene = world.resource::<DroppedItemScene>();
        let instance = scene
            .instances
            .iter()
            .find(|instance| instance.world_from_item[0][3] == 10.0)
            .expect("the framed item remains visible");
        match (color, &scene.models[instance.model as usize]) {
            (Some(color), DroppedItemModel::Cube(cube)) => {
                for (face, pixels) in cube.faces.iter().enumerate() {
                    assert!(
                        pixels
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .all(|pixel| *pixel == [color, face as u8, 80, 255])
                    );
                }
            }
            (None, DroppedItemModel::Sprite(sprite)) => {
                assert_eq!(sprite.rgba8.as_ref(), [20, 30, 40, 255])
            }
            _ => panic!("frames must retain the current pack's cube artwork"),
        }
    }
}

#[test]
fn dropped_cube_uses_exact_metadata_without_borrowing_another_variant() {
    for (metadata, sheet_metadata, custom) in [(7, 7, true), (7, 0, true), (3, 7, false)] {
        let (mut icons, stream) = fixture(metadata);
        install(&mut icons, Some(160), true, sheet_metadata);
        let mut world = World::new();
        world.init_resource::<DroppedItemScene>();
        let mut publisher = SystemState::<DroppedItemPublisher<'_, '_>>::new(&mut world);
        publisher.get_mut(&mut world).unwrap().publish(
            Some(&icons),
            Some(&stream),
            None,
            (None, None),
            Default::default(),
            (None, Default::default()),
        );
        let scene = world.resource::<DroppedItemScene>();
        assert_eq!(scene.instances.len(), 1);
        let model = &scene.models[scene.instances[0].model as usize];
        if custom {
            let DroppedItemModel::Cube(cube) = model else {
                panic!("matching pack sheet must replace the base");
            };
            assert_eq!(cube.faces[0][0], 160);
        } else {
            let DroppedItemModel::NativeSprite(sprite) = model else {
                panic!("an unrelated variant must retain base artwork");
            };
            assert_eq!(sprite.rgba8.as_ref(), [20, 30, 40, 255]);
        }
    }
}

/// Builds a dropped block with original startup cube artwork.
fn carried_fixture() -> (
    UiPresentationRuntime,
    WorldStream,
    Arc<RuntimeAssets>,
    [assets::IconSprite; 6],
) {
    let (compiled, records) = barrels();
    let assets = Arc::new(RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap());
    let visual = assets::BlockVisualId(
        records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:barrel")
            .unwrap()
            .sequential_id,
    );
    let tiles = std::array::from_fn(|face| assets::IconSprite {
        width: assets::BLOCK_ITEM_FACE_SIDE,
        height: assets::BLOCK_ITEM_FACE_SIDE,
        rgba8: [31 + face as u8, 70, 90, 255]
            .repeat(usize::from(assets::BLOCK_ITEM_FACE_SIDE).pow(2))
            .into(),
    });
    let catalog = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_block_sheets(
            assets.provenance().source_manifest_sha256,
            &[
                assets::IconSprite {
                    width: 1,
                    height: 1,
                    rgba8: Arc::from([20, 30, 40, 255]),
                },
                assets::compose_block_item_sheet(&tiles).unwrap(),
            ],
            &[assets::IconEntry {
                identifier: ITEM.into(),
                metadata: 0,
                sprite: 0,
            }],
            &[assets::IconBlockSheet { visual, sprite: 1 }],
        )
        .unwrap(),
    )
    .unwrap();
    let (icons, mut stream) = fixture_with_catalog(0, catalog);
    stream.set_sequential_id_remap(assets::SequentialIdRemap::from_palette(
        vec![0, visual.0],
        visual.0 + 1,
    ));
    (icons, stream, assets, tiles)
}

#[test]
fn dropped_session_artwork_precedes_startup_carried_cubes() {
    let (mut icons, stream, assets, tiles) = carried_fixture();
    let mut world = World::new();
    world.init_resource::<DroppedItemScene>();
    world.insert_resource(ChunkTextureAssets::new(assets));
    let mut publisher = SystemState::<DroppedItemPublisher<'_, '_>>::new(&mut world);
    for (color, sheet) in [(Some(90), false), (Some(160), true), (None, false)] {
        install(&mut icons, color, sheet, 0);
        publisher.get_mut(&mut world).unwrap().publish(
            Some(&icons),
            Some(&stream),
            None,
            (None, None),
            Default::default(),
            (None, Default::default()),
        );
        let scene = world.resource::<DroppedItemScene>();
        assert_eq!(scene.instances.len(), 1);
        match (
            color,
            sheet,
            &scene.models[scene.instances[0].model as usize],
        ) {
            (Some(color), false, DroppedItemModel::NativeSprite(sprite)) => {
                assert_eq!(sprite.rgba8.as_ref(), [color, 0, 80, 255]);
            }
            (Some(color), true, DroppedItemModel::Cube(cube)) => {
                assert_eq!(cube.faces[0][0], color);
            }
            (None, _, DroppedItemModel::Cube(cube)) => {
                for (face, pixels) in cube.faces.iter().enumerate() {
                    assert_eq!(pixels.as_ref(), tiles[face].rgba8.as_ref());
                }
            }
            _ => panic!("session sheets and sprites must precede startup carried artwork"),
        }
    }
}

/// Keeps unrelated models demanded so only `free` atlas layers remain for item artwork.
fn pressured_cache(free: usize) -> ModelCache {
    let mut cache = ModelCache::default();
    cache.begin_frame();
    cache.finish_demand();
    for id in 0..(render_model::MAX_ITEM_LAYERS - 1 - free) as u32 {
        cache
            .model(
                ModelKey::Block { hashed: false, id },
                || Some(1),
                || {
                    Some(DroppedItemModel::Sprite(DroppedItemSprite {
                        width: 1,
                        height: 1,
                        rgba8: Arc::from([255; 4]),
                    }))
                },
            )
            .unwrap();
    }
    cache
}

#[test]
fn dropped_session_cube_uses_session_sprite_under_cache_pressure() {
    let (mut icons, stream) = fixture(0);
    install(&mut icons, Some(160), true, 0);
    let mut views = Vec::new();
    stream.authority().dropped_items_into(0.0, &mut views);
    let view = &views[0];
    let preparation = DropPreparation {
        view,
        native_origin: view.position,
        camera: None,
        frustum: None,
    };
    for free in 1..assets::BlockFace::ALL.len() {
        let mut cache = pressured_cache(free);
        let prepared =
            DroppedItemPublisher::dropped_model(&mut cache, &icons, None, None, &preparation)
                .expect("a refused cube must try its one-layer session sprite");
        let mut instances = Vec::new();
        prepared.append(view, &mut instances, |_| (0, 15));
        assert_eq!(instances.len(), 1);
        let DroppedItemModel::NativeSprite(sprite) = &cache.models[instances[0].model as usize]
        else {
            panic!("session sprite must precede startup artwork");
        };
        assert_eq!(sprite.rgba8.as_ref(), [160, 0, 80, 255]);
        assert_eq!(cache.budget_fallbacks, 1);
    }
}

#[test]
fn framed_session_cube_uses_session_sprite_under_cache_pressure() {
    let (mut icons, _) = fixture(0);
    install(&mut icons, Some(160), true, 0);
    let placement = render_model::StaticItemPlacement {
        identifier: ITEM.into(),
        metadata: 0,
        potted_block: None,
        carried_block: Some(assets::BlockVisualId(1)),
        world_from_item: render_model::dropped_item_transform([0.0; 3], 0.0, 0.5),
        light: None,
    };
    for free in 1..assets::BlockFace::ALL.len() {
        let mut cache = pressured_cache(free);
        let model = DroppedItemPublisher::placement_model(
            &mut cache,
            &icons,
            None,
            &placement,
            icons.startup_item_icon(ITEM, 0),
        )
        .expect("a refused framed cube must try its one-layer session sprite");
        let DroppedItemModel::Sprite(sprite) = &cache.models[model as usize] else {
            panic!("session sprite must precede startup artwork");
        };
        assert_eq!(sprite.rgba8.as_ref(), [160, 0, 80, 255]);
        assert_eq!(cache.budget_fallbacks, 1);
    }
}

#[test]
fn refused_session_artwork_reuses_resident_startup_sprites() {
    let (mut icons, stream) = fixture(0);
    install(&mut icons, Some(160), true, 0);
    let startup_icon = icons.startup_item_icon(ITEM, 0).unwrap();
    assert_ne!(Some(startup_icon), icons.session_item_icon(ITEM, 0));
    let mut views = Vec::new();
    stream.authority().dropped_items_into(0.0, &mut views);
    let view = &views[0];
    let preparation = DropPreparation {
        view,
        native_origin: view.position,
        camera: None,
        frustum: None,
    };
    let placement = render_model::StaticItemPlacement {
        identifier: ITEM.into(),
        metadata: 0,
        potted_block: None,
        carried_block: Some(assets::BlockVisualId(1)),
        world_from_item: render_model::dropped_item_transform([0.0; 3], 0.0, 0.5),
        light: None,
    };
    for native in [false, true] {
        let mut cache = pressured_cache(1);
        let startup =
            DroppedItemPublisher::resolved_icon_model(&mut cache, startup_icon, native, || {
                resolved_icon_sprite(&icons, startup_icon, native)
            })
            .unwrap();
        let model = if native {
            let prepared =
                DroppedItemPublisher::dropped_model(&mut cache, &icons, None, None, &preparation)
                    .expect("resident startup sprites must survive refused session artwork");
            let mut instances = Vec::new();
            prepared.append(view, &mut instances, |_| (0, 15));
            instances[0].model
        } else {
            DroppedItemPublisher::placement_model(
                &mut cache,
                &icons,
                None,
                &placement,
                Some(startup_icon),
            )
            .expect("resident startup sprites must survive refused session artwork")
        };
        assert_eq!(model, startup);
        let (DroppedItemModel::Sprite(sprite) | DroppedItemModel::NativeSprite(sprite)) =
            &cache.models[model as usize]
        else {
            panic!("startup sprite must be retained");
        };
        assert_eq!(sprite.rgba8.as_ref(), [20, 30, 40, 255]);
        assert_eq!(cache.budget_fallbacks, 1);
        cache.begin_frame();
        assert_eq!(cache.budget_fallbacks, 0);
    }
}

#[test]
fn checkerboard_session_icon_falls_back_to_startup_cube() {
    let (mut icons, stream, assets, _) = carried_fixture();
    let mut runtime = UiRuntime::new(1);
    runtime.set_session_icons(Some(Arc::new(SessionIcons {
        icons: vec![SessionIcon {
            identifier: ITEM.into(),
            metadata: 0,
            width: 512,
            height: 512,
            rgba8: (0_u32..512 * 512)
                .flat_map(|pixel| {
                    [
                        90,
                        0,
                        80,
                        if (pixel / 512 + pixel % 512).is_multiple_of(2) {
                            255
                        } else {
                            0
                        },
                    ]
                })
                .collect(),
        }],
        ..Default::default()
    })));
    icons
        .build(
            &player_state::PlayerState::new(1),
            &runtime,
            0,
            [800, 600],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let mut cache = ModelCache::default();
    cache.begin_frame();
    cache.finish_demand();
    let mut dropped = Vec::new();
    stream.authority().dropped_items_into(0.0, &mut dropped);
    let view = &dropped[0];
    let preparation = DropPreparation {
        view,
        native_origin: view.position,
        camera: None,
        frustum: None,
    };
    let prepared = DroppedItemPublisher::dropped_model(
        &mut cache,
        &icons,
        Some(&assets),
        item_block_id(
            &stream,
            view.item.visual,
            view.item.identifier.as_deref(),
            Some(&assets),
        ),
        &preparation,
    );
    let prepared = prepared.expect("valid startup artwork remains available");
    let mut instances = Vec::new();
    prepared.append(view, &mut instances, |_| (0, 15));
    assert!(
        matches!(
            cache.models[instances[0].model as usize],
            DroppedItemModel::Cube(_)
        ),
        "an undrawable session extrusion must try the startup cube"
    );
}

#[test]
fn framed_cubes_fall_back_to_sprites_at_the_device_layer_limit() {
    let (mut icons, _) = fixture(0);
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE.map(u32::from);
    let mut runtime = UiRuntime::new(1);
    runtime.set_session_icons(Some(Arc::new(SessionIcons {
        icons: (0..43)
            .map(|metadata| SessionIcon {
                identifier: ITEM.into(),
                metadata,
                width: 1,
                height: 1,
                rgba8: Box::new([metadata as u8, 0, 80, 255]),
            })
            .collect(),
        block_sheets: (0..43)
            .map(|metadata| SessionIcon {
                identifier: ITEM.into(),
                metadata,
                width,
                height,
                rgba8: [metadata as u8, 0, 80, 255]
                    .repeat((width * height) as usize)
                    .into(),
            })
            .collect(),
        ..Default::default()
    })));
    icons
        .build(
            &player_state::PlayerState::new(1),
            &runtime,
            0,
            [800, 600],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let mut cache = ModelCache::default();
    cache.budget = render_model::dropped_item::ItemModelBudget::new(
        assets::texture::policy::device_policy(),
        256,
        u64::MAX,
    );
    for _ in 0..2 {
        cache.begin_frame();
        cache.finish_demand();
        let models: Vec<_> = (0..43)
            .map(|metadata| {
                let placement = render_model::StaticItemPlacement {
                    identifier: ITEM.into(),
                    metadata,
                    potted_block: None,
                    carried_block: Some(assets::BlockVisualId(1)),
                    world_from_item: render_model::dropped_item_transform([0.0; 3], 0.0, 1.0),
                    light: None,
                };
                DroppedItemPublisher::placement_model(
                    &mut cache,
                    &icons,
                    None,
                    &placement,
                    icons.startup_item_icon(ITEM, metadata),
                )
                .expect("every frame has drawable artwork")
            })
            .collect();
        assert!(
            models[..42]
                .iter()
                .all(|&slot| matches!(cache.models[slot as usize], DroppedItemModel::Cube(_)))
        );
        assert!(
            matches!(
                cache.models[models[42] as usize],
                DroppedItemModel::Sprite(_)
            ),
            "the overflowing cube must select a sprite"
        );
        cache.finish_frame();
        let mut budget = cache.budget;
        budget.reset();
        for slot in models {
            let model = &cache.shared[slot as usize];
            let mesh = model
                .mesh(model.atlas_side(budget.policy()).unwrap(), 1)
                .unwrap();
            assert!(
                budget.admit(model, mesh.len()).is_some(),
                "every selected model must fit the renderer budget"
            );
        }
    }
}
