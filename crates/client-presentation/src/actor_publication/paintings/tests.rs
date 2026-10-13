use super::*;
use protocol::{WorldBootstrap, WorldEvent};

/// Produces original painting rasters with a different colour for each image source.
fn pack() -> assets::SessionEntityPack {
    let mut files = Vec::new();
    for motive in PAINTING_MOTIVES {
        let texture = motive.texture();
        let size = if motive.atlas_origin.is_some() {
            [256; 2]
        } else {
            motive.size.map(u32::from)
        };
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(size[0], size[1], image::Rgba([75, 140, 210, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        files.push((format!("{texture}.png").into_boxed_str(), png.into_inner()));
    }
    let compiled = pack_entity_compiler::compile_actor_pack(files)
        .unwrap()
        .unwrap();
    assets::SessionEntityPack {
        assets: Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap()),
        textures: compiled.textures.into(),
        bindings: compiled.bindings.into(),
        equipment: None,
    }
}

/// Creates authority with no animation substitute for a dedicated painting packet.
fn world() -> client_world::WorldAuthority {
    client_world::WorldAuthority::new(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 999,
            local_player_unique_id: 999,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    )
}

/// Starts an empty publication while preserving independently textured parts.
fn batch() -> ActorPresentationBatch {
    ActorPresentationBatch {
        submissions: Vec::new(),
        artwork: Default::default(),
        skin_layers: Vec::new(),
    }
}

#[test]
fn admitted_paintings_publish_both_surfaces_in_all_wall_directions() {
    let pack = pack();
    let pages = ActorArtworkPages::default().with_pack_artwork(&pack.textures, &pack.bindings);
    let mut routes = PaintingRoutes::default();
    routes.refresh(None, Some(&pack), &pages);
    let mut builder =
        render_model::ActorRigFrameBuilder::new(render_model::pack_geometries(&pack.assets))
            .unwrap();
    for motive in PAINTING_MOTIVES {
        for (direction, expected_normal) in [
            (0, [0.0, 0.0, 1.0]),
            (1, [-1.0, 0.0, 0.0]),
            (2, [0.0, 0.0, -1.0]),
            (3, [1.0, 0.0, 0.0]),
        ] {
            let mut world = world();
            let event = protocol::test_support::packets::add_painting_event(
                -7,
                7,
                motive.title,
                direction,
                [3.5, 70.5, 8.5],
            )
            .unwrap()
            .unwrap();
            world.apply_ordered_event(event, Some(1)).unwrap();
            let mut output = batch();
            routes.append(&world, 0.5, None, &mut output);
            assert_eq!(output.submissions.len(), 2, "{} {direction}", motive.title);
            assert_eq!(output.artwork.len(), 2);
            assert!(
                output
                    .submissions
                    .iter()
                    .all(|part| part.input.identity.is_exact())
            );
            let frame = builder.build_located(
                0.5,
                None,
                output.submissions.iter().cloned().map(|submission| {
                    let location = output.artwork.get(&submission.input.identity).copied();
                    (submission, location)
                }),
                |_, location| location.unwrap().page,
            );
            assert_eq!(frame.rejects, render_model::ActorRigRejects::default());
            assert_eq!(
                frame.instances.len(),
                2,
                "both painting parts reach the actual draw builder"
            );
            assert!(output.skin_layers.is_empty());
            let front = &output.submissions[0];
            let rows = front.world_from_actor;
            for (axis, row) in rows.into_iter().enumerate() {
                assert!((-row[2] - expected_normal[axis]).abs() < 0.00001);
                assert_eq!(row[3], [3.5, 70.5, 8.5][axis]);
            }
            let vertices = render_model::geometry_from_runtime_assets(
                &pack.assets,
                (front.input.rig.0 - assets::PACK_RIG_ID_BASE) as usize,
            )
            .unwrap();
            assert_eq!(
                vertices.vertices.len(),
                6,
                "front is one single-sided image"
            );
            assert!(
                vertices
                    .vertices
                    .iter()
                    .all(|vertex| (vertex.position[2] + 1.0 / 32.0).abs() < 0.00001)
            );
            assert!(Arc::ptr_eq(
                &front.input.current_bones,
                &output.submissions[1].input.current_bones
            ));
            let mut again = batch();
            routes.append(&world, 0.75, None, &mut again);
            assert_eq!(output.submissions, again.submissions);
            world
                .apply_ordered_event(
                    WorldEvent::Actor(protocol::ActorEvent::Remove(protocol::ActorRemoveEvent {
                        dimension: 0,
                        unique_id: -7,
                    })),
                    Some(2),
                )
                .unwrap();
            let mut removed = batch();
            routes.append(&world, 0.5, None, &mut removed);
            assert!(removed.submissions.is_empty());
        }
    }
}

#[test]
fn invisible_paintings_publish_neither_front_nor_border() {
    let pack = pack();
    let pages = ActorArtworkPages::default().with_pack_artwork(&pack.textures, &pack.bindings);
    let mut routes = PaintingRoutes::default();
    routes.refresh(None, Some(&pack), &pages);
    let mut world = world();
    let WorldEvent::Actor(protocol::ActorEvent::PaintingSpawn {
        mut spawn,
        painting,
    }) = protocol::test_support::packets::add_painting_event(-7, 7, "Kebab", 0, [3.5, 70.5, 8.5])
        .unwrap()
        .unwrap()
    else {
        panic!("dedicated painting spawn")
    };
    spawn.metadata = Arc::from([protocol::ActorMetadata {
        key: 0,
        value: protocol::ActorMetadataValue::Flags(1 << 5),
    }]);
    world
        .apply_ordered_event(
            WorldEvent::Actor(protocol::ActorEvent::PaintingSpawn { spawn, painting }),
            Some(1),
        )
        .unwrap();
    let mut output = batch();
    routes.append(&world, 0.5, None, &mut output);
    assert!(
        output.submissions.is_empty(),
        "all surfaces of an invisible painting stay hidden"
    );
}

#[test]
fn painting_edges_remain_visible_until_the_rendered_surface_leaves_the_viewport() {
    use bevy::math::{Mat4, Vec3, Vec4};
    let pack = pack();
    let pages = ActorArtworkPages::default().with_pack_artwork(&pack.textures, &pack.bindings);
    let mut routes = PaintingRoutes::default();
    routes.refresh(None, Some(&pack), &pages);
    for direction in 0..4 {
        let across_z = direction % 2 == 1;
        let clip_from_world = if across_z {
            Mat4::from_cols(Vec4::Z, Vec4::Y, Vec4::X, Vec4::W)
        } else {
            Mat4::IDENTITY
        };
        let view = ActorCullView {
            clip_from_world,
            camera_position: Vec3::ZERO,
            max_distance: 64.0,
        };
        for (edge, expected) in [(1.484375, 2), (1.515625, 0)] {
            let position = if across_z {
                [0.5, 0.0, edge]
            } else {
                [edge, 0.0, 0.5]
            };
            let mut world = world();
            world
                .apply_ordered_event(
                    protocol::test_support::packets::add_painting_event(
                        -7, 7, "Kebab", direction, position,
                    )
                    .unwrap()
                    .unwrap(),
                    Some(1),
                )
                .unwrap();
            let mut output = batch();
            routes.append(&world, 0.5, Some(view), &mut output);
            assert_eq!(
                output.submissions.len(),
                expected,
                "direction {direction}, edge {edge}"
            );
        }
    }
}
