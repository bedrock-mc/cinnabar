use super::*;
use crate::environment::{CameraMediumState, EnvironmentContext, FogBindings};
use crate::runtime::world::{ClientWorld, update_camera_medium};
use client_presentation::camera::FlyCamera;

#[test]
fn camera_environment_samples_the_live_biome_for_atmosphere_profiles() {
    let position = [4.5, 1.0, 4.5];
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: position,
        world_spawn_position: [0; 3],
        air_network_id: protocol::air_network_id(false),
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                definitions: Arc::from([BiomeDefinitionEvent {
                    biome_id: Some(42),
                    name: Arc::from("test:camera"),
                    temperature: -0.5,
                    downfall: 0.5,
                    snow_foliage: 0.0,
                    max_snow_accumulation: None,
                    map_water_color: 0xff44_6688,
                }]),
            }),
        )
        .unwrap();
    let mut payload = overworld_biome_payload();
    payload[1] = 84;
    stream
        .submit(
            2,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload,
            }),
        )
        .unwrap();
    stream
        .submit(3, WorldEvent::ChunkRadiusUpdated(16))
        .unwrap();
    complete_world_stream_decodes(&mut stream);

    let base = assets::RuntimeAtmosphereAssets::decode(
        &assets::carriers::startup::test_carriers::synthetic_atmosphere_blob(),
    )
    .unwrap();
    let atmosphere = Arc::new(
        base.with_resource_pack_overrides(
            &[],
            &[assets::BiomeVisualProfile {
                biome_identifier: "test:camera".into(),
                fog_identifier: "test:fog".into(),
                atmosphere_identifier: "test:atmosphere".into(),
                lighting_identifier: "test:lighting".into(),
                sky_rgb8: Some(0x0c2238),
            }],
            &[assets::FogProfile {
                identifier: "test:fog".into(),
                distances: Box::new([assets::FogDistance {
                    medium: assets::FogMedium::Air,
                    mode: assets::FogDistanceMode::RenderRelative,
                    start_bits: 0.25_f32.to_bits(),
                    end_bits: 0.75_f32.to_bits(),
                    rgb8: 0x445566,
                    transition: None,
                }]),
            }],
        )
        .unwrap(),
    );
    let identity = [7; 32];
    let mut app = App::new();
    app.insert_resource(ClientWorld {
        stream: Some(stream),
        ..Default::default()
    })
    .init_resource::<CameraMediumState>()
    .init_resource::<EnvironmentContext>()
    .init_resource::<FogBindings>()
    .insert_resource(render::AtmosphereTextureAssets::new(
        atmosphere.clone(),
        identity,
    ))
    .add_systems(Update, update_camera_medium);
    let camera = app
        .world_mut()
        .spawn((
            FlyCamera::default(),
            Transform::from_translation(Vec3::from_array(position)),
        ))
        .id();
    app.update();

    let context = app.world().resource::<EnvironmentContext>();
    assert_eq!(
        context.camera_biome_identifier.as_deref(),
        Some("test:camera")
    );
    assert_eq!(context.camera_biome_temperature, Some(-0.5));
    assert_eq!(context.render_distance_blocks, Some(256.0));
    assert_eq!(context.fog_registry_generation, Some(identity));
    let (frame, route) = crate::environment::atmosphere::derive_profiled_atmosphere_frame(
        Default::default(),
        Default::default(),
        0.0,
        app.world().resource::<CameraMediumState>().0,
        context,
        atmosphere.biome_profiles(),
        atmosphere.fog_profiles(),
        identity,
        None,
        0.0,
    );
    assert_eq!(route.biome_identifier.as_deref(), Some("test:camera"));
    assert_eq!(route.fog_identifier.as_deref(), Some("test:fog"));
    let unprofiled = crate::environment::atmosphere::derive_profiled_atmosphere_frame(
        Default::default(),
        Default::default(),
        0.0,
        app.world().resource::<CameraMediumState>().0,
        context,
        &[],
        &[],
        identity,
        None,
        0.0,
    )
    .0;
    assert_ne!(frame, unprofiled);

    app.world_mut()
        .get_mut::<Transform>(camera)
        .unwrap()
        .translation
        .x = 20.5;
    app.update();
    let context = app.world().resource::<EnvironmentContext>();
    assert!(context.camera_biome_identifier.is_none());
    assert!(context.camera_biome_temperature.is_none());
    app.world_mut().resource_mut::<ClientWorld>().stream = None;
    app.update();
    assert_eq!(
        *app.world().resource::<EnvironmentContext>(),
        EnvironmentContext::default()
    );
}
