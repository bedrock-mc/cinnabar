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
