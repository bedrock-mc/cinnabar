use super::*;

fn cube_world_assets(
    entities: &assets::RuntimeEntityAssets,
) -> std::sync::Arc<assets::RuntimeAssets> {
    use assets::*;
    use sha2::{Digest, Sha256};
    let count = entities.block_visual_count() as usize;
    let mut visuals = vec![
        BlockVisual {
            faces: [1; 6],
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            kind: VisualKind::Cube,
            support: VisualSupport::Exact,
            contributor_role: ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        };
        count
    ];
    visuals[0] = BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary);
    let source = CompiledAssets {
        visuals: visuals.into(),
        light_properties: vec![LightProperties::default(); count].into(),
        hashed: Box::new([]),
        materials: vec![
            Material {
                texture: TextureRef::new(0, 0).unwrap(),
                flags: 0,
                animation: NO_ANIMATION,
                ..assets::Material::unvaried()
            };
            2
        ]
        .into(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: 1,
            mips: [16, 8, 4, 2, 1]
                .into_iter()
                .map(|size| TextureMip {
                    size,
                    rgba8: vec![255; size as usize * size as usize * 4].into(),
                })
                .collect::<Vec<_>>()
                .into(),
        })]
        .into(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: BlobProvenance {
            source_manifest_sha256: entities.source_manifest_sha256(),
            block_registry_sha256: Sha256::digest(
                crate::asset_startup::pinned_block_registry_bytes(),
            )
            .into(),
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    };
    std::sync::Arc::new(RuntimeAssets::decode(&assets::encode_blob(&source).unwrap()).unwrap())
}

#[test]
fn real_selected_block_provider_and_rotated_ui_publisher_bind_cube_and_clear_rejection() {
    let mut player_runtime = PlayerRuntime::new(1);

    use crate::ui_runtime::presentation::tests::{fixture_font, fixture_hud};
    use crate::{
        camera::FlyCamera,
        presentation::viewmodel::{HandAdapter, HandFallback, ViewmodelPublish},
        runtime::world::ClientWorld,
    };
    use bevy::{
        camera::{Camera, ComputedCameraValues, RenderTarget, RenderTargetInfo},
        ecs::system::RunSystemOnce,
        prelude::*,
    };
    use client_ui::ui_runtime::presentation::{UiPresentationRuntime, refresh_hud_frame};
    use protocol::WorldBootstrap;
    use std::sync::Arc;
    let (pack, _geometry, _) = hand_fixture();
    // Match the decoded carrier's unsupported player-controller route: retain
    // the player symbol, authored geometry and item routes, but no resolved rig.
    let mut compiled = pack_compiler::compile_entity_assets(
        &pack.0,
        include_bytes!("../../../../assets/vanilla-source.json"),
    )
    .unwrap();
    compiled.rig_bindings = Box::new([]);
    compiled.rig_geometries = Box::new([]);
    compiled.rig_animations = Box::new([]);
    compiled.rig_controllers = Box::new([]);
    compiled.render = Default::default();
    let entities = Arc::new(
        assets::RuntimeEntityAssets::decode(&assets::encode_entity_blob(&compiled).unwrap())
            .unwrap(),
    );
    assert!(
        !entities
            .geometry_candidates("geometry.humanoid.custom")
            .is_empty()
    );
    assert!(entities.rig_bindings().is_empty());
    assert!(entities.rig_geometries().is_empty());
    let assets = cube_world_assets(&entities);
    let bootstrap = WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0., 64., 0.],
        world_spawn_position: [0, 64, 0],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    };
    let stream = chunk_pipeline::WorldStream::new_with_asset_sets(
        bootstrap,
        assets.clone(),
        entities.clone(),
        [0., 64., 0.],
        None,
    );
    assert!(stream.authority().actor(1).is_none());
    assert!(stream.authority().actor_rig(1).is_none());
    let mut world = ClientWorld::new_with_entity_assets(assets, entities);
    world.stream = Some(stream);
    let mut runtime = UiRuntime::new(1);
    runtime
        .publish_local_runtime_id(&mut player_runtime, 1, 1)
        .unwrap();
    let item = protocol::vanilla_item_registry()
        .iter()
        .find(|item| item.identifier.as_ref() == "minecraft:dirt")
        .unwrap()
        .network_id;
    let mut stack = NetworkItemStack::empty();
    stack.network_id = item;
    stack.count = 1;
    // Equipment never carries a stack network id.
    stack.stack_network_id = -1;
    stack.extra_data = Arc::from([0; 10]);
    stack.nbt_digest = {
        use sha2::Digest;
        sha2::Sha256::digest(&stack.extra_data).into()
    };
    let session = protocol::BedrockSession { shield_item_id: 0 };
    let wire = protocol::encode(
        &protocol::select_hotbar_slot_packet(1, 0, &stack).unwrap(),
        &session,
    )
    .unwrap();
    let decoded = protocol::decode_batch(wire, &session)
        .unwrap()
        .pop()
        .unwrap();
    let Some(protocol::WorldEvent::Equipment(decoded)) =
        protocol::into_world_event(decoded, 0).unwrap()
    else {
        panic!("expected selected equipment");
    };
    assert_eq!(decoded.stack, stack);
    let held = EquipmentEvent {
        actor_runtime_id: 1,
        stack: decoded.stack,
        inventory_slot: 0,
        selected_slot: 0,
        window_id: 0,
        handedness: Some(ActorHandedness::Right),
    };
    runtime.retain_local_selected_equipment(&mut player_runtime, 1, held.clone());
    runtime.retain_local_selected_equipment(
        &mut player_runtime,
        2,
        equipment(1, NetworkItemStack::empty()),
    );
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    presentation.set_player_preview_skin(Some(&vec![255; 64 * 64 * 4]), Default::default());
    refresh_hud_frame(
        &player_runtime,
        &mut runtime,
        &mut presentation,
        world.stream.as_ref(),
        crate::camera::CameraSettingsAuthority::default().perspective(),
        0,
    );
    // Block-routed items are absent from the sprite-only icon catalog. The
    // real right-hand carrier remains visible until current cube completion.
    assert!(presentation.hud_frame().held_item_icon.is_none());
    assert!(presentation.cpu_empty_hand_fallback().is_some());
    presentation.hud_frame_mut().viewmodel_pitch_degrees = 30.;
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [640, 480],
            ui::DpiScale::new(1.).unwrap(),
        )
        .unwrap();
    let empty = presentation.cpu_empty_hand_fallback();
    let mut app = App::new();
    let mut movement = crate::movement::MovementTicker::default();
    let mut physics = crate::movement::LocalPhysicsController::default();
    crate::movement::reset_start_game_prediction(&mut movement, &mut physics, 1, [0., 64., 0.]);
    movement.set_source(crate::movement::MovementSource::Physics);
    let mut avatar = crate::local_player::LocalAvatarPresentation::default();
    let mut view = crate::local_player::LocalViewPose::default();
    let mut settings = crate::camera::CameraSettingsAuthority::default();
    crate::local_player::reset_local_player_session(
        1,
        1,
        [0., 64., 0.],
        &mut settings,
        &mut view,
        &mut avatar,
    );
    let mut visibility = crate::local_player::LocalAvatarVisibilityCarrier::default();
    avatar.publish_view_visibility(
        semantic_input::PerspectiveMode::FirstPerson,
        Vec3::new(0., 64., 0.),
        Vec3::new(0., 64. - protocol::PLAYER_NETWORK_OFFSET, 0.),
        Quat::IDENTITY,
        &mut visibility,
    );
    app.insert_resource(runtime)
        .insert_resource(player_runtime.clone())
        .insert_resource(world)
        .insert_resource(movement)
        .insert_resource(visibility)
        .init_resource::<HandAdapter>()
        .init_resource::<render::ViewmodelScene>()
        .init_resource::<render::ViewmodelCompletionGate>();
    assert!(!app.world().contains_resource::<render::ViewmodelGeometry>());
    app.world_mut().spawn((
        FlyCamera::default(),
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(640, 480),
                    scale_factor: 1.,
                }),
                ..Default::default()
            },
            ..Default::default()
        },
        RenderTarget::default(),
        Msaa::Off,
    ));
    let observed_input = input.clone();
    app.world_mut()
        .run_system_once(
            move |mut hand: ViewmodelPublish,
                  player_runtime: Res<PlayerRuntime>,
                  runtime: Res<UiRuntime>,
                  world: Res<ClientWorld>| {
                assert!(!hand.observe(&player_runtime, &runtime, &world, true, false, [640, 480]));
                hand.bind_cpu_fallback(&observed_input, empty, None);
                let (reason, values) = hand.diagnostic_snapshot(
                    &player_runtime,
                    &runtime,
                    &world,
                    true,
                    false,
                    [640, 480],
                    false,
                );
                assert_eq!(reason, 0);
                assert_eq!(values[4], 0);
                assert_eq!(values[6], 2);
                assert_eq!(values[8], i128::from(item));
                assert_eq!(values[9], 1);
                assert_eq!(values[14], 2);
                assert_eq!(values[21], 1);
                assert_eq!(values[23], 0);
                assert_eq!(values[24], 0);
            },
        )
        .unwrap();
    assert_cube_scene(&app, true);
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.mode,
        Some(render::ViewmodelMode::OpaqueCubeNeutralStaticFallback)
    );
    // Exercise the real committed-control reconciliation, not a fabricated
    // actor spawn counter. The free-camera correction branch anchors without
    // requiring loaded collision chunks; cube admission resumes only afterward.
    let mut clock = crate::environment::WorldClock::default();
    let mut weather = crate::environment::WeatherState::default();
    crate::environment::bind_session_generation(&mut clock, &mut weather, 1);
    let breg = crate::asset_startup::pinned_block_registry_bytes();
    let records = assets::read_registry_for_protocol(breg, 2193).unwrap();
    let collisions = crate::movement::PhysicsCollisionRegistries::from_assets(
        breg,
        &records,
        include_bytes!("../../../../crates/assets/data/block-physics-v2193.bin"),
        2193,
    )
    .unwrap();
    app.insert_resource(clock)
        .insert_resource(weather)
        .insert_resource(collisions)
        .insert_resource(physics)
        .insert_resource(crate::acceptance::AcceptanceRun::new(
            Some(900),
            None,
            false,
            false,
        ))
        .insert_resource(crate::acceptance::model_witness::ModelWitnessFileSource::new(None))
        .init_resource::<crate::movement::LocalMovementEffectTimeline>()
        .init_resource::<crate::movement::LocalMovementSpeedAuthority>()
        .init_resource::<Time<bevy::time::Real>>()
        .init_resource::<render::ChunkUploadBudget>()
        .init_resource::<crate::camera::CameraSettingsAuthority>()
        .init_resource::<crate::local_player::LocalViewPose>()
        .init_resource::<crate::local_player::LocalPlayerFrameCarrier>()
        .init_resource::<crate::local_player::InteractionOriginSnapshot>()
        .init_resource::<crate::runtime::phase3_evidence::Phase3EvidenceEmitter>()
        .init_resource::<crate::runtime::world::WorldStreamFramePoll>()
        .init_resource::<client_presentation::server_camera::ServerCameraInstructions>()
        .add_message::<client_presentation::audio_ingress::SequencedAudioEvent>();
    let controls = [
        protocol::WorldEvent::Respawn(protocol::RespawnEvent {
            position: [1., 64., 0.],
            state: 1,
            runtime_entity_id: 1,
        }),
        protocol::WorldEvent::MovePlayer(protocol::MovePlayerEvent {
            runtime_id: 1,
            position: [2., 64., 0.],
            teleported: true,
            ..Default::default()
        }),
        protocol::WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
            dimension: 1,
            position: [3., 64., 0.],
            ..Default::default()
        }),
        protocol::WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
            dimension: 0,
            position: [4., 64., 0.],
            ..Default::default()
        }),
    ];
    for (index, event) in controls.into_iter().enumerate() {
        app.world_mut()
            .resource_mut::<crate::movement::MovementTicker>()
            .set_source(crate::movement::MovementSource::FreeCamera);
        let before = app
            .world()
            .resource::<crate::movement::MovementTicker>()
            .interaction_authority_identity();
        app.world_mut()
            .resource_mut::<ClientWorld>()
            .stream
            .as_mut()
            .unwrap()
            .submit(index as u64 + 1, event)
            .unwrap();
        app.world_mut()
            .run_system_once(crate::runtime::world::reconcile_world_stream_before_physics)
            .unwrap();
        let after = app
            .world()
            .resource::<crate::movement::MovementTicker>()
            .interaction_authority_identity();
        assert_eq!(before.0, after.0);
        assert!(after.1 > before.1);
        app.world_mut()
            .run_system_once(
                |mut hand: ViewmodelPublish,
                 player_runtime: Res<PlayerRuntime>,
                 runtime: Res<UiRuntime>,
                 world: Res<ClientWorld>| {
                    assert!(!hand.observe(
                        &player_runtime,
                        &runtime,
                        &world,
                        true,
                        false,
                        [640, 480]
                    ));
                },
            )
            .unwrap();
        assert_cube_scene(&app, false);
        app.world_mut()
            .resource_mut::<crate::movement::MovementTicker>()
            .set_source(crate::movement::MovementSource::Physics);
        let input = input.clone();
        app.world_mut()
            .run_system_once(
                move |mut hand: ViewmodelPublish,
                      player_runtime: Res<PlayerRuntime>,
                      runtime: Res<UiRuntime>,
                      world: Res<ClientWorld>| {
                    assert!(!hand.observe(
                        &player_runtime,
                        &runtime,
                        &world,
                        true,
                        false,
                        [640, 480]
                    ));
                    hand.bind_cpu_fallback(&input, empty, None);
                },
            )
            .unwrap();
        assert_cube_scene(&app, true);
        assert!(app.world().resource::<ClientWorld>().fatal_error.is_none());
    }
    assert!(
        input
            .vertices
            .iter()
            .any(|vertex| vertex.position[0] > 640.)
    );
    let mut bad = input.clone();
    bad.viewport_size[0] += 1;
    app.world_mut()
        .run_system_once(move |mut hand: ViewmodelPublish| {
            hand.bind_cpu_fallback(&bad, empty, None)
        })
        .unwrap();
    assert_cube_scene(&app, false);
    let mut mismatched = held.clone();
    mismatched.stack.block_runtime_id = i32::MAX;
    crate::tests::with_ui_player(&mut app, |runtime, player_runtime| {
        runtime.retain_local_selected_equipment(player_runtime, 3, mismatched);
    });
    app.world_mut()
        .run_system_once(
            |mut hand: ViewmodelPublish,
             player_runtime: Res<PlayerRuntime>,
             runtime: Res<UiRuntime>,
             world: Res<ClientWorld>| {
                assert!(!hand.observe(&player_runtime, &runtime, &world, true, false, [640, 480]));
            },
        )
        .unwrap();
    assert_cube_scene(&app, false);
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.fallback,
        Some(HandFallback::ItemsUnknownOrHeld)
    );
    crate::tests::with_ui_player(&mut app, |runtime, player_runtime| {
        runtime.retain_local_selected_equipment(player_runtime, 4, held.clone());
    });
    let recovered_input = input.clone();
    app.world_mut()
        .run_system_once(
            move |mut hand: ViewmodelPublish,
                  player_runtime: Res<PlayerRuntime>,
                  runtime: Res<UiRuntime>,
                  world: Res<ClientWorld>| {
                assert!(!hand.observe(&player_runtime, &runtime, &world, true, false, [640, 480]));
                hand.bind_cpu_fallback(&recovered_input, empty, None);
            },
        )
        .unwrap();
    assert_cube_scene(&app, true);
    crate::tests::with_ui_player(&mut app, |runtime, player_runtime| {
        runtime.retain_local_selected_equipment(
            player_runtime,
            5,
            EquipmentEvent {
                stack: NetworkItemStack::empty(),
                ..held.clone()
            },
        );
    });
    app.world_mut()
        .run_system_once(
            |mut hand: ViewmodelPublish,
             player_runtime: Res<PlayerRuntime>,
             runtime: Res<UiRuntime>,
             world: Res<ClientWorld>| {
                assert!(!hand.observe(&player_runtime, &runtime, &world, true, false, [640, 480]));
            },
        )
        .unwrap();
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.fallback,
        Some(HandFallback::Ownership)
    );
    assert!(app.world().resource::<HandAdapter>().stats.mode.is_none());
    assert_cube_scene(&app, false);
    crate::tests::with_ui_player(&mut app, |runtime, player_runtime| {
        runtime.retain_local_selected_equipment(player_runtime, 6, held.clone());
    });
    let resumed_input = input.clone();
    app.world_mut()
        .run_system_once(
            move |mut hand: ViewmodelPublish,
                  player_runtime: Res<PlayerRuntime>,
                  runtime: Res<UiRuntime>,
                  world: Res<ClientWorld>| {
                assert!(!hand.observe(&player_runtime, &runtime, &world, true, false, [640, 480]));
                hand.bind_cpu_fallback(&resumed_input, empty, None);
            },
        )
        .unwrap();
    assert_cube_scene(&app, true);
    crate::tests::with_ui_player(&mut app, |runtime, player_runtime| {
        crate::session::begin_session(runtime, player_runtime, 2);
    });
    app.world_mut()
        .run_system_once(
            |mut hand: ViewmodelPublish,
             player_runtime: Res<PlayerRuntime>,
             runtime: Res<UiRuntime>,
             world: Res<ClientWorld>| {
                assert!(!hand.observe(&player_runtime, &runtime, &world, true, false, [640, 480]));
            },
        )
        .unwrap();
    assert_cube_scene(&app, false);
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.fallback,
        Some(HandFallback::Ownership)
    );
    crate::tests::with_ui_player(&mut app, |runtime, player_runtime| {
        runtime
            .publish_local_runtime_id(player_runtime, 2, 1)
            .unwrap();
        runtime.retain_local_selected_equipment(player_runtime, 1, held);
        runtime.retain_local_selected_equipment(
            player_runtime,
            2,
            equipment(1, NetworkItemStack::empty()),
        );
    });
    app.world_mut()
        .resource_mut::<crate::movement::MovementTicker>()
        .reset(2, 0, [0., 64., 0.]);
    let mut avatar = crate::local_player::LocalAvatarPresentation::default();
    avatar.begin_session(2, 1);
    avatar.publish_view_visibility(
        semantic_input::PerspectiveMode::FirstPerson,
        Vec3::new(0., 64., 0.),
        Vec3::new(0., 64. - protocol::PLAYER_NETWORK_OFFSET, 0.),
        Quat::IDENTITY,
        &mut app
            .world_mut()
            .resource_mut::<crate::local_player::LocalAvatarVisibilityCarrier>(),
    );
    for fresh_stream in [false, true] {
        if fresh_stream {
            let mut world = app.world_mut().resource_mut::<ClientWorld>();
            let fresh = chunk_pipeline::WorldStream::new_with_asset_sets(
                bootstrap,
                world.runtime_assets.clone(),
                world.entity_assets.clone().unwrap(),
                bootstrap.player_position,
                None,
            );
            assert!(
                fresh.authority().actor_session_id()
                    > world
                        .stream
                        .as_ref()
                        .unwrap()
                        .authority()
                        .actor_session_id()
            );
            assert!(fresh.authority().actor(1).is_none());
            world.stream = Some(fresh);
        }
        let current_input = input.clone();
        app.world_mut()
            .run_system_once(
                move |mut hand: ViewmodelPublish,
                      player_runtime: Res<PlayerRuntime>,
                      runtime: Res<UiRuntime>,
                      world: Res<ClientWorld>| {
                    assert!(!hand.observe(
                        &player_runtime,
                        &runtime,
                        &world,
                        true,
                        false,
                        [640, 480]
                    ));
                    hand.bind_cpu_fallback(&current_input, empty, None);
                },
            )
            .unwrap();
        assert_cube_scene(&app, fresh_stream);
    }
    // Retiring genuine local authority withholds the cube; no remote spawn was
    // ever installed, and a visibility snapshot alone cannot grant admission.
    app.world_mut()
        .resource_mut::<crate::movement::MovementTicker>()
        .deactivate();
    app.world_mut()
        .run_system_once(
            |mut hand: ViewmodelPublish,
             player_runtime: Res<PlayerRuntime>,
             runtime: Res<UiRuntime>,
             world: Res<ClientWorld>| {
                assert!(
                    world
                        .stream
                        .as_ref()
                        .unwrap()
                        .authority()
                        .actor(1)
                        .is_none()
                );
                assert!(!hand.observe(&player_runtime, &runtime, &world, true, false, [640, 480]));
                let (reason, values) = hand.diagnostic_snapshot(
                    &player_runtime,
                    &runtime,
                    &world,
                    true,
                    false,
                    [640, 480],
                    false,
                );
                assert_eq!(reason, 3);
                assert_eq!(values[4], 0);
                assert_eq!(values[21], 0);
            },
        )
        .unwrap();
    assert_eq!(
        app.world().resource::<HandAdapter>().stats.fallback,
        Some(HandFallback::Ownership)
    );
    assert_cube_scene(&app, false);
}
