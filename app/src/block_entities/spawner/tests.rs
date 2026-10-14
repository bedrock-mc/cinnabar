use super::*;

mod rescan;

/// Builds an authored display with two controller layers and an explicit pack texture.
fn fixture() -> (Pool, chunk_pipeline::WorldStream, ActorArtworkPages) {
    fixture_with_scripts(serde_json::json!({}))
}

/// Builds the same display with supplied model-scale expressions.
fn fixture_with_scripts(
    scripts: serde_json::Value,
) -> (Pool, chunk_pipeline::WorldStream, ActorArtworkPages) {
    fixture_with_light_query(scripts, "1")
}

/// Adds a render-controller query to the original display for frame sampling checks.
fn fixture_with_light_query(
    scripts: serde_json::Value,
    light_query: &str,
) -> (Pool, chunk_pipeline::WorldStream, ActorArtworkPages) {
    fixture_with_identifier(scripts, light_query, "custom:display")
}

/// Builds an original display catalog under the supplied entity identifier.
fn fixture_with_identifier(
    scripts: serde_json::Value,
    light_query: &str,
    identifier: &str,
) -> (Pool, chunk_pipeline::WorldStream, ActorArtworkPages) {
    let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"custom:display","materials":{"default":"entity"},"textures":{"default":"textures/entity/display"},"geometry":{"default":"geometry.display"},"render_controllers":["controller.render.lit","controller.render.unlit"]}}}"#;
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.display","texture_width":16,"texture_height":16},"bones":[{"name":"body","cubes":[{"origin":[-4,0,-4],"size":[8,32,8],"uv":[0,0]}]}]}]}"#;
    let controllers = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.lit":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]},"controller.render.unlit":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"],"ignore_lighting":true}}}"#;
    let mut controllers: serde_json::Value = serde_json::from_slice(controllers).unwrap();
    for controller in controllers["render_controllers"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        controller["light_color_multiplier"] = serde_json::json!(light_query);
    }
    let mut entity: serde_json::Value = serde_json::from_slice(entity).unwrap();
    entity["minecraft:client_entity"]["description"]["identifier"] = serde_json::json!(identifier);
    entity["minecraft:client_entity"]["description"]["scripts"] = scripts;
    let compiled = pack_entity_compiler::compile_entity_pack(vec![
        (
            "entity/display.json".into(),
            serde_json::to_vec(&entity).unwrap(),
        ),
        ("models/entity/display.geo.json".into(), geometry.to_vec()),
        (
            "render_controllers/display.json".into(),
            serde_json::to_vec(&controllers).unwrap(),
        ),
        (
            "textures/entity/display.png".into(),
            b"synthetic-raster".to_vec(),
        ),
    ])
    .unwrap()
    .unwrap();
    let texture_source = compiled.assets.render.candidates[0].source;
    let source = Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.assets).unwrap());
    let mut actors = DisplayActorStore::new(1, Arc::clone(&source));
    actors.set_pack(Some((Arc::clone(&source), vec![0])));
    let stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 1,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let pack = Arc::new(assets::SessionEntityPack {
        assets: Arc::clone(&source),
        textures: Arc::from([assets::ActorTexture {
            source: texture_source,
            width: 16,
            height: 16,
            pixel_sha256: [0; 32],
            rgba8: vec![255; 16 * 16 * 4].into(),
        }]),
        bindings: Arc::from([assets::ActorArtworkBinding {
            rig: 0,
            geometry_candidate: 0,
            entity_symbol: 0,
            geometry: 0,
            render_controller: 0,
            texture: 0,
            material: "entity".into(),
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
        }]),
        equipment: None,
    });
    let artwork = ActorArtworkPages::default().with_pack_artwork(&pack.textures, &pack.bindings);
    let pool = Pool {
        source,
        session: (stream.authority().actor_session_id(), 1),
        pack: Some(pack),
        actors,
        columns: Default::default(),
        rescans: Default::default(),
        spins: Default::default(),
        poses: Default::default(),
        layers: Default::default(),
    };
    (pool, stream, artwork)
}

/// Publishes one resolved display using the production conversion and controller path.
fn draw(
    pool: &mut Pool,
    stream: &chunk_pipeline::WorldStream,
    artwork: &ActorArtworkPages,
) -> ActorPresentationBatch {
    let position = [0, 512, 0];
    let runtime = pool.actors.resolve(position, "custom:display").unwrap();
    pool.actors.advance(1);
    display_batch(
        pool,
        stream,
        artwork,
        0.5,
        vec![(runtime, position, Default::default())],
    )
}

#[test]
fn regression_retired_display_poses_release_their_bone_allocations() {
    let (mut pool, stream, artwork) = fixture();
    let position = [0, 512, 0];
    let runtime = pool.actors.resolve(position, "custom:display").unwrap();
    pool.actors.advance(1);
    let presentation = entity_rig_presentation_cached(
        &pool.actors.rig(runtime).unwrap(),
        pool.actors.actor(runtime).unwrap(),
        &artwork,
        0.5,
        Some(&mut pool.poses),
    )
    .unwrap();
    let bones = Arc::downgrade(&presentation.submission.input.current_bones);
    drop(presentation);
    pool.actors.retain(&BTreeSet::new());
    assert!(
        bones.upgrade().is_some(),
        "recently drawn poses remain cached"
    );
    for _ in 0..256 {
        display_batch(&mut pool, &stream, &artwork, 0.5, Vec::new());
    }
    assert!(
        bones.upgrade().is_none(),
        "retired display poses must be released"
    );
    assert!(!draw(&mut pool, &stream, &artwork).submissions.is_empty());
}

#[test]
fn regression_display_controller_lighting_overrides_survive_solved_world_light() {
    let (mut pool, stream, artwork) = fixture();
    // An out-of-height sample supplies resolved darkness without a terrain fixture.
    assert_eq!(stream.solved_light_at([0.5, 512.5, 0.5]), Some((0, 0)));
    let batch = draw(&mut pool, &stream, &artwork);
    assert_eq!(batch.submissions.len(), 2);
    assert_eq!(
        batch.submissions[0].light,
        render_model::pack_actor_light(0, 0)
    );
    assert_eq!(
        batch.submissions[1].light,
        render_model::pack_actor_light_without_lightmap()
    );
}

/// Encodes a signed network integer used by the synthetic terrain and NBT fixture.
fn signed(value: i32, bytes: &mut Vec<u8>) {
    let mut value = ((value << 1) ^ (value >> 31)) as u32;
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        bytes.push(byte | if value == 0 { 0 } else { 0x80 });
        if value == 0 {
            return;
        }
    }
}

/// Encodes a network block-entity compound with an id, entity identifier and position.
fn entity_nbt(id: &str, identifier: &str, position: [i32; 3]) -> Vec<u8> {
    let mut nbt = vec![10, 0];
    for (name, value) in [("id", id), ("EntityIdentifier", identifier)] {
        nbt.extend([8, name.len() as u8]);
        nbt.extend_from_slice(name.as_bytes());
        nbt.push(value.len() as u8);
        nbt.extend_from_slice(value.as_bytes());
    }
    for (name, value) in ["x", "y", "z"].into_iter().zip(position) {
        nbt.extend([3, 1, name.as_bytes()[0]]);
        signed(value, &mut nbt);
    }
    nbt.push(0);
    nbt
}

/// Admits one spawner through normal terrain decoding in the stream's current dimension.
fn admit_spawner(stream: &mut chunk_pipeline::WorldStream, position: [i32; 3], sequence: u64) {
    admit_spawners(stream, position, &[(position, "custom:display")], sequence);
}

/// Admits configured displays in one column with spawner backing in one section.
fn admit_spawners(
    stream: &mut chunk_pipeline::WorldStream,
    position: [i32; 3],
    displays: &[([i32; 3], &str)],
    sequence: u64,
) {
    use protocol::{LevelChunkEvent, LevelChunkMode, WorldEvent};
    let dimension = stream.current_dimension();
    let version = assets::active_content_registry_protocol();
    let records =
        assets::read_registry_for_protocol(assets::pinned_block_registry_bytes(), version).unwrap();
    let id = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:mob_spawner")
        .unwrap()
        .sequential_id;
    let range = protocol::vanilla_dimension_range(dimension).unwrap();
    let side = SUB_CHUNK_SIDE as i32;
    let section_y = position[1].div_euclid(side);
    let mut payload = Vec::new();
    for y in range.base_sub_chunk_y..section_y {
        payload.extend([9, 0, y as u8]);
    }
    payload.extend([9, 1, section_y as u8, 1]);
    signed(id as i32, &mut payload);
    payload.push(1);
    signed(0, &mut payload);
    payload.extend(std::iter::repeat_n(0xff, range.sub_chunk_count - 1));
    payload.push(0);
    for &(display, identifier) in displays {
        payload.extend(entity_nbt("MobSpawner", identifier, display));
    }
    stream
        .submit(
            sequence,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension,
                x: position[0].div_euclid(side),
                z: position[2].div_euclid(side),
                mode: LevelChunkMode::Inline {
                    count: (section_y - range.base_sub_chunk_y + 1) as usize,
                },
                payload,
            }),
        )
        .unwrap();
    let key = BlockEntityKey::new(dimension, position[0], position[1], position[2]);
    test_time::eventually("synthetic spawner terrain", || {
        stream.poll(position.map(|axis| axis as f32), 0);
        stream.collision_store().block_entity(key).is_some()
    });
}

/// Reuses the detached actor schedule with an original display catalog and a distant camera.
fn system_fixture(scripts: serde_json::Value, light_query: &str) -> bevy::prelude::World {
    let (pool, _, artwork) = fixture_with_light_query(scripts, light_query);
    let eye = [1024.0, 66.0, 1024.0];
    let mut stream = chunk_pipeline::WorldStream::new_with_assets(
        protocol::WorldBootstrap {
            dimension: 1,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: eye,
            world_spawn_position: [1024, 64, 1024],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        eye,
        None,
    );
    admit_spawner(&mut stream, [1024, 64, 1024], 1);
    let mut world = crate::runtime::network::actor_publication_test_world();
    {
        let mut client = world.resource_mut::<ClientWorld>();
        client.entity_assets = Some(pool.source);
        client.pack_entities = pool.pack;
        client.stream = Some(stream);
    }
    world.insert_resource(render_scene::ActorArtworkResource(artwork));
    let rotation = bevy::prelude::Quat::from_euler(
        bevy::prelude::EulerRot::YXZ,
        40f32.to_radians(),
        -20f32.to_radians(),
        0.0,
    );
    world.insert_resource(LocalViewPose::new(
        Vec3::new(1024.0, 66.0, 1024.0),
        rotation,
    ));
    let camera = world.query_filtered::<bevy::prelude::Entity, bevy::prelude::With<client_presentation::camera::FlyCamera>>().single(&world).unwrap();
    *world.get_mut::<bevy::prelude::Transform>(camera).unwrap() = bevy::prelude::Transform {
        translation: Vec3::new(1027.0, 68.0, 1024.0),
        rotation,
        ..Default::default()
    };
    world
}

/// Runs both actor preparation stages before appending this frame's display actors.
fn prepare_displays(world: &mut bevy::prelude::World, delta: std::time::Duration) {
    use bevy::prelude::{Real, Time};
    world.resource_mut::<Time<Real>>().advance_by(delta);
    world
        .run_system_cached(crate::runtime::network::advance_actor_frame)
        .unwrap();
    world
        .run_system_cached(crate::runtime::network::prepare_actor_render_frame)
        .unwrap();
    world.run_system_cached(prepare_spawner_actors).unwrap();
}

#[test]
fn regression_dimension_change_reuses_publication_ticks_and_fraction() {
    let mut world = system_fixture(serde_json::json!({}), "query.frame_alpha");
    prepare_displays(&mut world, simulation_clock::TICK_DURATION * 4 / 5);
    let session = world
        .resource::<ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor_session_id();
    {
        let mut client = world.resource_mut::<ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        stream
            .submit(
                2,
                protocol::WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
                    dimension: 0,
                    position: [1024.0, 66.0, 1024.0],
                    ..Default::default()
                }),
            )
            .unwrap();
        stream.poll([1024.0, 66.0, 1024.0], 0);
        assert_eq!(stream.authority().actor_session_id(), session);
        admit_spawner(stream, [1024, 64, 1024], 3);
    }
    prepare_displays(&mut world, simulation_clock::TICK_DURATION * 2 / 5);
    let pool = world
        .resource::<SpawnerDisplayPublisher>()
        .0
        .as_ref()
        .unwrap();
    let rig = pool.actors.rig(1).unwrap();
    assert_eq!(
        rig.completed_tick, 1,
        "the retained remainder and elapsed time must advance one display tick"
    );
    let body = world
        .resource::<PreparedActorPublication>()
        .submissions()
        .unwrap()
        .iter()
        .find(|body| body.input.identity.session_id == rig.actor.session_id)
        .unwrap();
    assert!(
        (body.material.light_color_multiplier - 0.2).abs() < 1e-5,
        "display controllers must sample the same fraction as published GPU bones: {}",
        body.material.light_color_multiplier
    );
}

#[test]
fn regression_display_queries_receive_resolved_camera_before_tick_and_frame_sampling() {
    let mut world = system_fixture(
        serde_json::json!({
            "scale": "query.distance_from_camera",
            "scaleX": "1 + query.camera_rotation(0) / 100",
            "scaleY": "1 + query.camera_rotation(1) / 100"
        }),
        "query.distance_from_camera + query.camera_rotation(0) / 100 + query.camera_rotation(1) / 100",
    );
    prepare_displays(&mut world, simulation_clock::TICK_DURATION);
    let pool = world
        .resource::<SpawnerDisplayPublisher>()
        .0
        .as_ref()
        .unwrap();
    let rig = pool.actors.rig(1).unwrap();
    assert!(
        (rig.scale - 5.0).abs() < 1e-5,
        "distance uses the camera near the distant spawner"
    );
    assert!((rig.axis_scale[0] - 1.2).abs() < 1e-5);
    assert!((rig.axis_scale[1] - 2.4).abs() < 1e-5);
    let camera = world.query_filtered::<bevy::prelude::Entity, bevy::prelude::With<client_presentation::camera::FlyCamera>>().single(&world).unwrap();
    world
        .get_mut::<bevy::prelude::Transform>(camera)
        .unwrap()
        .translation
        .x = 1030.0;
    world
        .get_mut::<bevy::prelude::Transform>(camera)
        .unwrap()
        .rotation = bevy::prelude::Quat::IDENTITY;
    world
        .resource_mut::<LocalViewPose>()
        .set_rotation(bevy::prelude::Quat::IDENTITY);
    prepare_displays(&mut world, simulation_clock::TICK_DURATION / 5);
    let pool = world
        .resource::<SpawnerDisplayPublisher>()
        .0
        .as_ref()
        .unwrap();
    let rig = pool.actors.rig(1).unwrap();
    assert_eq!(
        rig.completed_tick, 1,
        "camera changes do not require another tick"
    );
    let layers = pool.actors.frame(0.2).layers(1).unwrap();
    for layer in layers.iter() {
        assert!(
            (layer.light_color_multiplier - (52f32.sqrt() + 1.8)).abs() < 1e-5,
            "camera queries must also update between actor ticks"
        );
    }
}

#[test]
fn regression_cage_fit_includes_sampled_uniform_and_axis_scale() {
    for (scripts, scale, axes) in [
        (serde_json::json!({"scale": 8}), 8.0, [1.0f32; 3]),
        (
            serde_json::json!({"scale": 2, "scaleX": 3, "scaleY": 4, "scaleZ": 2}),
            2.0,
            [3.0, 4.0, 2.0],
        ),
        (
            serde_json::json!({"scale": 8, "scaleX": -2, "scaleY": 4}),
            8.0,
            [-2.0, 4.0, 1.0],
        ),
    ] {
        let (mut pool, stream, artwork) = fixture_with_scripts(scripts);
        let batch = draw(&mut pool, &stream, &artwork);
        let body = &batch.submissions[0];
        let dimensions = std::array::from_fn(|axis| {
            body.culling_bounds.half_extents[axis] * 2.0 * scale * axes[axis].abs()
        });
        let fit =
            spawner_actor_transform([0, 512, 0], dimensions, Default::default(), 0.5).unwrap();
        let normal = Mat4::from_scale(Vec3::new(
            -scale * axes[0],
            scale * axes[1],
            -scale * axes[2],
        ));
        let expected = matrix_rows(fit * normal);
        for (actual, expected) in body
            .world_from_actor
            .iter()
            .flatten()
            .zip(expected.iter().flatten())
        {
            assert!(
                (actual - expected).abs() < 1e-5,
                "scaled dimensions must control fitting and vertical placement: {actual} != {expected}"
            );
        }
        let extent = dimensions.into_iter().fold(0.1f32, f32::max);
        assert!((extent * fit.x_axis.truncate().length() - 1.0).abs() < 1e-5);
    }
}

#[test]
fn regression_display_limit_follows_distance_and_backing_filters() {
    let side = SUB_CHUNK_SIDE as i32;
    for stale_backing in [false, true] {
        let mut world = system_fixture(serde_json::json!({}), "1");
        let target = [1039, 64, 1024];
        let prefix_y = if stale_backing { 32 } else { 64 };
        let mut displays: Vec<_> = (0..client_world::MAX_DISPLAY_ACTORS)
            .map(|index| {
                let index = index as i32;
                [1024, prefix_y + index / side, 1024 + index % side]
            })
            .collect();
        displays.push(target);
        {
            let mut client = world.resource_mut::<ClientWorld>();
            let stream = client.stream.as_mut().unwrap();
            let displays: Vec<_> = displays
                .iter()
                .map(|&display| (display, "custom:display"))
                .collect();
            admit_spawners(stream, target, &displays, 2);
            let key =
                BlockEntityKey::new(stream.current_dimension(), target[0], target[1], target[2]);
            assert_eq!(
                stream
                    .collision_store()
                    .chunk(key.chunk())
                    .unwrap()
                    .shared_block_entities()
                    .len(),
                displays.len(),
                "the column must contain more configured displays than the actor budget"
            );
        }
        let eye = if stale_backing {
            Vec3::new(1039.0, 66.0, 1024.0)
        } else {
            Vec3::new(1090.0, 66.0, 1024.0)
        };
        world.insert_resource(LocalViewPose::new(eye, bevy::prelude::Quat::IDENTITY));
        for _ in 0..2 {
            prepare_displays(&mut world, simulation_clock::TICK_DURATION);
            let pool = world
                .resource::<SpawnerDisplayPublisher>()
                .0
                .as_ref()
                .unwrap();
            assert_eq!(pool.spins.keys().copied().collect::<Vec<_>>(), vec![target]);
            assert_eq!(
                world
                    .resource::<PreparedActorPublication>()
                    .submissions()
                    .unwrap()
                    .len(),
                2,
                "the eligible display must publish both controller layers"
            );
        }
        if !stale_backing {
            world.insert_resource(LocalViewPose::new(
                Vec3::new(1039.0, 66.0, 1024.0),
                bevy::prelude::Quat::IDENTITY,
            ));
            prepare_displays(&mut world, simulation_clock::TICK_DURATION);
            let pool = world
                .resource::<SpawnerDisplayPublisher>()
                .0
                .as_ref()
                .unwrap();
            assert_eq!(pool.spins.len(), client_world::MAX_DISPLAY_ACTORS);
        }
    }
}

/// Places one full actor budget of displays on the x plane of the backed section.
fn display_plane(x: i32, identifier: &str) -> Vec<([i32; 3], &str)> {
    let side = SUB_CHUNK_SIDE as i32;
    (0..client_world::MAX_DISPLAY_ACTORS as i32)
        .map(|index| ([x, 64 + index / side, 1024 + index % side], identifier))
        .collect()
}

/// Returns this frame's published actor submissions.
fn published(world: &bevy::prelude::World) -> usize {
    world
        .resource::<PreparedActorPublication>()
        .submissions()
        .unwrap()
        .len()
}

#[test]
fn regression_rejected_or_unrigged_entities_do_not_consume_the_display_limit() {
    for rejected in ["custom:\u{7}display", "custom:unknown"] {
        let mut world = system_fixture(serde_json::json!({}), "1");
        let target = [1039, 64, 1024];
        let mut displays = display_plane(1024, rejected);
        displays.push((target, "custom:display"));
        {
            let mut client = world.resource_mut::<ClientWorld>();
            admit_spawners(client.stream.as_mut().unwrap(), target, &displays, 2);
        }
        world.insert_resource(LocalViewPose::new(
            Vec3::new(1039.0, 66.0, 1024.0),
            bevy::prelude::Quat::IDENTITY,
        ));
        for _ in 0..2 {
            prepare_displays(&mut world, simulation_clock::TICK_DURATION);
            let pool = world
                .resource::<SpawnerDisplayPublisher>()
                .0
                .as_ref()
                .unwrap();
            assert_eq!(pool.spins.keys().copied().collect::<Vec<_>>(), vec![target]);
            assert_eq!(
                published(&world),
                2,
                "{rejected:?} must not starve the valid display"
            );
        }
    }
}

#[test]
fn regression_rigless_cache_eviction_preserves_unchanged_display_lifetimes() {
    let mut world = system_fixture(serde_json::json!({}), "1");
    let side = SUB_CHUNK_SIDE as i32;
    let identifiers: Vec<_> = (0..client_world::MAX_DISPLAY_ACTORS * 2)
        .map(|index| format!("custom:missing_{index}"))
        .collect();
    let mut displays: Vec<_> = identifiers
        .iter()
        .enumerate()
        .map(|(index, identifier)| {
            let index = index as i32;
            (
                [
                    1024 + index / (side * side),
                    64 + index / side % side,
                    1024 + index % side,
                ],
                identifier.as_str(),
            )
        })
        .collect();
    let target = [1039, 64, 1024];
    displays.push((target, "custom:display"));
    {
        let mut client = world.resource_mut::<ClientWorld>();
        admit_spawners(client.stream.as_mut().unwrap(), target, &displays, 2);
    }
    world.insert_resource(LocalViewPose::new(
        Vec3::new(1039.0, 66.0, 1024.0),
        bevy::prelude::Quat::IDENTITY,
    ));
    let camera = world
        .query_filtered::<bevy::prelude::Entity, With<FlyCamera>>()
        .single(&world)
        .unwrap();
    // All candidates lie behind this camera, preserving their column scan order.
    *world.get_mut::<Transform>(camera).unwrap() = Transform {
        translation: Vec3::new(1039.0, 66.0, 1000.0),
        ..Default::default()
    };
    prepare_displays(&mut world, simulation_clock::TICK_DURATION);
    assert_eq!(published(&world), 2);
    let runtime = world
        .resource::<PreparedActorPublication>()
        .submissions()
        .unwrap()[0]
        .input
        .identity
        .runtime_id;
    let identity = world
        .resource::<SpawnerDisplayPublisher>()
        .0
        .as_ref()
        .unwrap()
        .actors
        .rig(runtime)
        .unwrap()
        .actor;
    for _ in 0..3 {
        prepare_displays(&mut world, simulation_clock::TICK_DURATION);
        let pool = world
            .resource::<SpawnerDisplayPublisher>()
            .0
            .as_ref()
            .unwrap();
        assert_eq!(pool.spins.keys().copied().collect::<Vec<_>>(), vec![target]);
        let submissions = world
            .resource::<PreparedActorPublication>()
            .submissions()
            .unwrap();
        assert_eq!(submissions.len(), 2);
        assert_eq!(
            pool.actors.rig(runtime).map(|rig| rig.actor),
            Some(identity),
            "unchanged displays must keep their lifetime after rigless cache eviction"
        );
    }
}

#[test]
fn regression_pack_changes_retry_rigless_column_requests() {
    let mut world = system_fixture(serde_json::json!({}), "1");
    let position = [1025, 64, 1024];
    {
        let mut client = world.resource_mut::<ClientWorld>();
        admit_spawners(
            client.stream.as_mut().unwrap(),
            position,
            &[(position, "custom:missing")],
            2,
        );
    }
    prepare_displays(&mut world, simulation_clock::TICK_DURATION);
    assert_eq!(published(&world), 0);
    let (replacement, _, _) = fixture_with_identifier(serde_json::json!({}), "1", "custom:missing");
    world.resource_mut::<ClientWorld>().pack_entities = replacement.pack;
    prepare_displays(&mut world, simulation_clock::TICK_DURATION);
    assert_eq!(
        published(&world),
        2,
        "a replacement pack must admit an entity rejected by its predecessor"
    );
}

#[test]
fn regression_full_display_pool_admits_a_disjoint_set_in_one_frame() {
    let mut world = system_fixture(serde_json::json!({}), "1");
    world.insert_resource(LocalViewPose::new(
        Vec3::new(1032.0, 66.0, 1032.0),
        bevy::prelude::Quat::IDENTITY,
    ));
    let first = display_plane(1024, "custom:display");
    {
        let mut client = world.resource_mut::<ClientWorld>();
        admit_spawners(client.stream.as_mut().unwrap(), first[1].0, &first, 2);
    }
    prepare_displays(&mut world, simulation_clock::TICK_DURATION);
    let full = published(&world);
    assert!(full > 0, "the first display set must fill the pool");
    let second = display_plane(1025, "custom:display");
    {
        let mut client = world.resource_mut::<ClientWorld>();
        admit_spawners(client.stream.as_mut().unwrap(), second[1].0, &second, 3);
    }
    prepare_displays(&mut world, simulation_clock::TICK_DURATION);
    assert_eq!(
        published(&world),
        full,
        "replacement displays must publish on the frame their predecessors retire"
    );
}

#[cfg(feature = "developer-control")]
#[test]
fn regression_developer_control_inspection_keeps_display_and_network_lifetimes_apart() {
    let mut world = system_fixture(serde_json::json!({}), "1");
    prepare_displays(&mut world, simulation_clock::TICK_DURATION);
    let display_session = world
        .resource::<SpawnerDisplayPublisher>()
        .0
        .as_ref()
        .unwrap()
        .actors
        .session_id();
    let (network_session, colliding) = {
        let authority = world
            .resource::<ClientWorld>()
            .stream
            .as_ref()
            .unwrap()
            .authority();
        (
            authority.actor_session_id(),
            authority.local_player_runtime_id(),
        )
    };
    assert_ne!(display_session, network_session);
    let (actor, display) =
        crate::developer_control::actors::manifest_actor(&world, display_session, colliding)
            .expect("a display sharing the local player's runtime id must stay inspectable");
    assert!(display);
    assert!(matches!(
        &actor.kind,
        protocol::ActorKind::Entity { identifier } if identifier.as_ref() == "custom:display"
    ));
    assert!(
        crate::developer_control::actors::manifest_actor(&world, network_session, colliding)
            .is_none_or(|(_, display)| !display),
        "network lifetimes never resolve to display actors"
    );
}

#[test]
fn regression_column_rebuild_reuses_unchanged_spawners_and_skips_other_entities() {
    let decoded = |id, position| {
        Arc::new(
            BlockEntityNbt::decode_prefix(&entity_nbt(id, "custom:display", position))
                .unwrap()
                .0,
        )
    };
    let mut index = EntityIndex::new();
    index.insert(
        BlockEntityKey::new(1, 0, 64, 0),
        decoded("MobSpawner", [0, 64, 0]),
    );
    index.insert(
        BlockEntityKey::new(1, 1, 64, 0),
        decoded("Chest", [1, 64, 0]),
    );
    let mut first = column(Arc::new(index.clone()), None);
    assert_eq!(
        first.displays.len(),
        1,
        "only spawners become display requests"
    );
    first.displays[0].rigless = true;
    index.insert(
        BlockEntityKey::new(1, 1, 64, 0),
        decoded("Chest", [1, 64, 0]),
    );
    let second = column(Arc::new(index.clone()), Some(&first));
    assert_eq!(second.displays.len(), 1);
    assert!(
        Arc::ptr_eq(
            &first.displays[0].identifier,
            &second.displays[0].identifier
        ),
        "an unchanged spawner snapshot must not be parsed again"
    );
    assert!(
        second.displays[0].rigless,
        "unchanged snapshots retain rig rejection"
    );
    let changed = BlockEntityKey::new(1, 0, 64, 0);
    index.insert(
        changed,
        Arc::new(
            BlockEntityNbt::decode_prefix(&entity_nbt(
                "MobSpawner",
                "custom:changed",
                changed.position(),
            ))
            .unwrap()
            .0,
        ),
    );
    let third = column(Arc::new(index), Some(&second));
    assert!(
        !third.displays[0].rigless,
        "changed snapshots retry rig admission"
    );
}

#[test]
fn regression_column_invalidation_does_not_retain_the_entity_index() {
    for id in ["Chest", "MobSpawner"] {
        let mut store = world::ChunkStore::default();
        let key = BlockEntityKey::new(1, 0, 64, 0);
        let initial =
            BlockEntityNbt::decode_prefix(&entity_nbt(id, "custom:display", key.position()))
                .unwrap()
                .0;
        store
            .commit_block_entity_update(key, initial.clone())
            .unwrap();
        let source = store.chunk(key.chunk()).unwrap().shared_block_entities();
        let index = Arc::downgrade(&source);
        let first = column(source, None);
        assert_eq!(
            index.strong_count(),
            1,
            "cached columns must allow entity updates without copying the index"
        );
        assert!(first.is_current(&store.chunk(key.chunk()).unwrap().shared_block_entities()));
        store.commit_block_entity_update(key, initial).unwrap();
        assert!(first.is_current(&store.chunk(key.chunk()).unwrap().shared_block_entities()));
        let other = BlockEntityKey::new(1, 1, 64, 0);
        store
            .commit_block_entity_update(
                other,
                BlockEntityNbt::decode_prefix(&entity_nbt("Sign", "", other.position()))
                    .unwrap()
                    .0,
            )
            .unwrap();
        let source = store.chunk(key.chunk()).unwrap().shared_block_entities();
        assert!(
            index.upgrade().is_none(),
            "the old index must be released on edits"
        );
        assert!(!first.is_current(&source));
        let second = column(source, Some(&first));
        assert_eq!(second.displays.len(), usize::from(id == "MobSpawner"));
        if id == "MobSpawner" {
            assert!(Arc::ptr_eq(
                &first.displays[0].identifier,
                &second.displays[0].identifier
            ));
        }
    }
}

#[test]
fn regression_display_slots_prefer_spawners_in_camera_view() {
    let mut world = system_fixture(serde_json::json!({}), "1");
    let target = [1039, 64, 1031];
    let mut displays = display_plane(1024, "custom:display");
    displays.push((target, "custom:display"));
    {
        let mut client = world.resource_mut::<ClientWorld>();
        admit_spawners(client.stream.as_mut().unwrap(), target, &displays, 2);
    }
    // The camera looks along +X, so the full plane at x = 1024 is behind it.
    let eye = Vec3::new(1036.0, 65.0, 1031.5);
    let facing = bevy::prelude::Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
    world.insert_resource(LocalViewPose::new(eye, facing));
    let camera = world.query_filtered::<bevy::prelude::Entity, bevy::prelude::With<client_presentation::camera::FlyCamera>>().single(&world).unwrap();
    *world.get_mut::<bevy::prelude::Transform>(camera).unwrap() = bevy::prelude::Transform {
        translation: eye,
        rotation: facing,
        ..Default::default()
    };
    prepare_displays(&mut world, simulation_clock::TICK_DURATION);
    let pool = world
        .resource::<SpawnerDisplayPublisher>()
        .0
        .as_ref()
        .unwrap();
    assert!(
        pool.spins.contains_key(&target),
        "an on-screen spawner must not be crowded out by displays behind the camera"
    );
    assert_eq!(pool.spins.len(), client_world::MAX_DISPLAY_ACTORS);
}
