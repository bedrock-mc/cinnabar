//! Original geometry through the production frame preparation system, without GPU/assets.
use std::{sync::Arc, time::Duration};

use bevy::{prelude::*, time::Real};
use client_presentation::actor_publication::PreparedActorPublication;
use client_ui::ui_runtime::UiRuntime;
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldBootstrap, WorldEvent};
use semantic_input::PerspectiveMode;

use crate::runtime::{network::actor_publication::*, world::ClientWorld};

pub(super) fn fixture() -> World {
    fixture_with_skin(false)
}

fn fixture_with_skin(with_skin: bool) -> World {
    let entity = br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:player","materials":{"default":"entity"},"textures":{"default":"textures/entity/test_player"},"geometry":{"default":"geometry.test_player"},"render_controllers":["controller.render.test_player"]}}}"#;
    let geometry = br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.test_player","texture_width":64,"texture_height":64},"bones":[
        {"name":"root","pivot":[0,0,0]},
        {"name":"waist","parent":"root","pivot":[0,12,0]},
        {"name":"body","parent":"waist","pivot":[0,24,0],"cubes":[{"origin":[-4,12,-2],"size":[8,12,4],"uv":[16,16]}]},
        {"name":"head","parent":"body","pivot":[0,24,0]},
        {"name":"leftArm","parent":"body","pivot":[5,22,0],"cubes":[{"origin":[4,12,-2],"size":[4,12,4],"uv":[32,48]}]},
        {"name":"rightArm","parent":"body","pivot":[-5,22,0],"cubes":[{"origin":[-8,12,-2],"size":[4,12,4],"uv":[40,16]}]},
        {"name":"leftLeg","parent":"root","pivot":[2,12,0],"cubes":[{"origin":[0,0,-2],"size":[4,12,4],"uv":[16,48]}]},
        {"name":"rightLeg","parent":"root","pivot":[-2,12,0],"cubes":[{"origin":[-4,0,-2],"size":[4,12,4],"uv":[0,16]}]}]}]}"#;
    let controller = br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test_player":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#;
    let compiled = pack_compiler::compile_entity_pack(vec![
        ("entity/player.json".into(), entity.to_vec()),
        ("models/entity/player.geo.json".into(), geometry.to_vec()),
        ("render_controllers/player.json".into(), controller.to_vec()),
    ])
    .unwrap()
    .unwrap();
    let entities = Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.assets).unwrap());
    let assets = Arc::new(assets::RuntimeAssets::diagnostic());
    let anchor = [0.0, 64.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
    let mut stream = chunk_pipeline::WorldStream::new_with_asset_sets(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: anchor,
            world_spawn_position: [0, 64, 0],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        assets.clone(),
        entities.clone(),
        anchor,
        None,
    );
    for (runtime_id, position) in [(1, [0.0, 64.0, 0.0]), (2, [2.0, 64.0, 0.0])] {
        stream
            .submit(
                runtime_id,
                WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                    dimension: 0,
                    unique_id: runtime_id as i64,
                    runtime_id,
                    kind: ActorKind::Player {
                        uuid: [runtime_id as u8; 16],
                        username: "fixture".into(),
                    },
                    position,
                    velocity: [0.0; 3],
                    pitch: 0.0,
                    yaw: 0.0,
                    head_yaw: 0.0,
                    body_yaw: 0.0,
                    held_item: Default::default(),
                    metadata: Arc::from([]),
                    attributes: Arc::from([]),
                    properties: Arc::from([]),
                    links: Arc::from([]),
                })),
            )
            .unwrap();
    }
    if with_skin {
        stream
            .submit(
                3,
                WorldEvent::Actor(ActorEvent::PlayerList(protocol::PlayerListUpdateEvent {
                    entries: Arc::from([protocol::PlayerListEntry::Add {
                        uuid: [1; 16],
                        unique_id: 1,
                        username: "fixture".into(),
                        verified: true,
                        skin: protocol::PlayerSkin::Standard(protocol::StandardSkin {
                            width: 64,
                            height: 64,
                            rgba8: vec![255; 64 * 64 * 4].into(),
                            cape: None,
                            geometry: Some(Arc::new(protocol::SkinGeometrySource {
                                resource_patch:
                                    r#"{"geometry":{"default":"geometry.test_player"}}"#.into(),
                                geometry_data: std::str::from_utf8(geometry).unwrap().into(),
                                animations: Arc::from([]),
                            })),
                        }),
                    }]),
                })),
            )
            .unwrap();
        stream.prepare_actor_appearance_fixture();
        stream.advance_actor_interpolation_ticks(1);
    }
    let mut client = ClientWorld::new_with_entity_assets(assets, entities.clone());
    client.stream = Some(stream);
    let mut physics = crate::movement::LocalPhysicsController::default();
    physics.reanchor_network_position(anchor, 0, true);
    let mut avatar = crate::local_player::LocalAvatarPresentation::default();
    avatar.begin_session(1, 1);
    let icons = Arc::new(
        assets::RuntimeIconCatalog::decode(
            &assets::encode_icon_catalog([0; 32], &[], &[]).unwrap(),
        )
        .unwrap(),
    );
    let (equipment, artwork, _) =
        client_presentation::presentation::equipment::EquipmentRuntime::build(
            entities.clone(),
            None,
            icons,
            None,
            None,
            render::ActorArtworkPages::default(),
        );
    let mut world = World::new();
    world.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
    world.insert_resource(client);
    world.insert_resource(Time::<Real>::default());
    world.insert_resource(render::ActorRenderScene::with_runtime_entity_assets(&entities).unwrap());
    world.insert_resource(PreparedActorPublication::default());
    world.insert_resource(avatar);
    world.init_resource::<crate::local_player::LocalAvatarVisibilityCarrier>();
    world.init_resource::<crate::camera::CameraSettingsAuthority>();
    world.insert_resource(physics);
    world.insert_resource(artwork);
    world.insert_resource(equipment);
    world.insert_resource(HandRigBuilder::from_runtime_assets(&entities).unwrap());
    world.init_resource::<render::HandRigScene>();
    world.insert_resource(crate::player_skin::LocalPlayerSkin::generated_default(
        "emote fixture",
    ));
    world.init_resource::<client_presentation::actor_publication::ActorFrameState>();
    world.init_resource::<ActorFramePartialTick>();
    world.insert_resource(UiRuntime::new(1));
    let camera =
        Transform::from_xyz(0.0, 66.0, -8.0).looking_at(Vec3::new(0.0, 65.0, 0.0), Vec3::Y);
    world.spawn((
        camera,
        Projection::Perspective(PerspectiveProjection::default()),
        crate::camera::FlyCamera::default(),
    ));
    world.insert_resource(crate::local_player::LocalViewPose::new(
        Vec3::from_array(anchor),
        camera.rotation,
    ));
    world
}

fn perspective(world: &mut World, mode: PerspectiveMode, generation: u64) {
    let mut settings = ui::UserSettings::default();
    settings.gameplay.default_perspective = mode;
    world
        .resource_mut::<crate::camera::CameraSettingsAuthority>()
        .replace(generation, &settings)
        .unwrap();
}

fn prepare(world: &mut World, millis: u64) {
    world
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(millis));
    world
        .run_system_cached(crate::runtime::network::advance_actor_frame)
        .unwrap();
    world.run_system_cached(prepare_actor_render_frame).unwrap();
}

fn body(world: &World, id: u64) -> render::ActorRigSubmission {
    world
        .resource::<PreparedActorPublication>()
        .submissions()
        .unwrap()
        .iter()
        .find(|body| {
            body.input.identity.runtime_id == id
                && body.input.identity.layer == render::ACTOR_LAYER_BODY
        })
        .expect("visible production body")
        .clone()
}

/// Java's player pose resamples its idle arm sway every frame, so an unaffected body is compared
/// with a world that never emotes at the same frame time.
fn assert_unaffected(world: &World, plain: &World, id: u64) {
    // Each fixture opens its own session, so identities differ by session id alone.
    let [body, plain] = [world, plain].map(|world| body(world, id).input);
    assert_eq!(
        (&body.rig, &body.previous_bones, &body.current_bones),
        (&plain.rig, &plain.previous_bones, &plain.current_bones),
        "player {id} never receives the local overlay"
    );
}

fn native_pose(world: &World) -> (u64, Vec<client_world::BoneTransform>) {
    let stream = world.resource::<ClientWorld>().stream.as_ref().unwrap();
    let rig = stream.authority().actor_rig(1).unwrap();
    (rig.completed_tick, rig.current.to_vec())
}

#[test]
fn custom_emote_knees_use_matching_mesh_and_retire_with_playback() {
    let (mut world, mut plain) = (fixture_with_skin(true), fixture_with_skin(true));
    for world in [&mut world, &mut plain] {
        perspective(world, PerspectiveMode::ThirdPersonBack, 1);
        prepare(world, 100);
    }
    let native = native_pose(&world);
    let ordinary = body(&world, 1);
    {
        let mut ui = world.resource_mut::<UiRuntime>();
        ui.emotes_mut().open();
        ui.emotes_mut().activate_slot(0, 100);
    }
    prepare(&mut world, 10);
    prepare(&mut plain, 10);
    let dance = body(&world, 1);
    assert_ne!(dance.input.rig, ordinary.input.rig);
    assert_eq!(
        dance.input.current_bones.len(),
        ordinary.input.current_bones.len() + 4
    );
    assert_eq!(native_pose(&world), native);
    assert_unaffected(&world, &plain, 2);
    world.resource_mut::<UiRuntime>().emotes_mut().stop();
    prepare(&mut world, 0);
    prepare(&mut plain, 0);
    assert_eq!(body(&world, 1).input.rig, ordinary.input.rig);
    assert_unaffected(&world, &plain, 1);
    assert_eq!(native_pose(&world), native);
}

#[test]
fn custom_emote_publication_advances_between_ticks_without_changing_native_hand_or_remote_pose() {
    let (mut world, mut plain) = (fixture(), fixture());
    for world in [&mut world, &mut plain] {
        perspective(world, PerspectiveMode::ThirdPersonBack, 1);
        prepare(world, 100);
    }
    let native = native_pose(&world);
    let rest = body(&world, 1);
    {
        let mut ui = world.resource_mut::<UiRuntime>();
        ui.emotes_mut().open();
        assert_eq!(
            ui.emotes_mut().activate_slot(0, 100),
            Some(client_world::CustomEmote::Twerk)
        );
    }
    prepare(&mut world, 10);
    prepare(&mut plain, 10);
    let first = body(&world, 1);
    assert_unaffected(&world, &plain, 2);
    assert_ne!(first.input.current_bones, rest.input.current_bones);
    assert_eq!(first.input.previous_bones, first.input.current_bones);
    prepare(&mut world, 10);
    prepare(&mut plain, 10);
    let second = body(&world, 1);
    assert_eq!(
        native_pose(&world),
        native,
        "render sampling leaves the tick-owned native rig untouched"
    );
    assert_ne!(
        first.input.current_bones, second.input.current_bones,
        "a held actor tick must not freeze the dance"
    );
    assert_unaffected(&world, &plain, 2);
    assert_eq!(first.input.rig, second.input.rig);
    assert_eq!(first.world_from_actor, second.world_from_actor);
    for world in [&mut world, &mut plain] {
        perspective(world, PerspectiveMode::FirstPerson, 2);
        prepare(world, 0);
    }
    assert!(
        world.resource::<render::HandRigScene>().is_active(),
        "native hand still publishes during playback"
    );
    assert_eq!(native_pose(&world), native);
    assert!(
        world
            .resource::<PreparedActorPublication>()
            .submissions()
            .unwrap()
            .iter()
            .all(|body| body.input.identity.runtime_id != 1),
        "first person does not publish the custom body"
    );
    world.resource_mut::<UiRuntime>().emotes_mut().stop();
    prepare(&mut world, 0);
    prepare(&mut plain, 0);
    assert!(world.resource::<render::HandRigScene>().is_active());
    assert_eq!(native_pose(&world), native);
    assert_unaffected(&world, &plain, 2);
}

/// The Java toggle leaves both native emote postures and local emote playback unchanged.
#[test]
fn emotes_keep_vanilla_bones_with_java_enabled() {
    for custom in [false, true] {
        let mut worlds = [fixture_with_skin(true), fixture_with_skin(true)];
        for (world, java) in worlds.iter_mut().zip([false, true]) {
            let mut settings = ui::UserSettings::default();
            settings.gameplay.default_perspective = PerspectiveMode::ThirdPersonBack;
            settings.video.java_animations = java;
            world
                .resource_mut::<crate::camera::CameraSettingsAuthority>()
                .replace(1, &settings)
                .unwrap();
            if custom {
                world.resource_mut::<UiRuntime>().emotes_mut().open();
                world
                    .resource_mut::<UiRuntime>()
                    .emotes_mut()
                    .activate_slot(0, 0);
            } else {
                let stream = world
                    .resource_mut::<ClientWorld>()
                    .into_inner()
                    .stream
                    .as_mut()
                    .unwrap();
                stream
                    .submit(
                        4,
                        WorldEvent::Actor(ActorEvent::Metadata(
                            protocol::ActorMetadataUpdateEvent {
                                dimension: 0,
                                runtime_id: 2,
                                metadata: Arc::from([protocol::ActorMetadata {
                                    key: 92,
                                    value: protocol::ActorMetadataValue::FlagsExtended(
                                        1 << (92 - 64),
                                    ),
                                }]),
                                properties: Arc::from([]),
                                tick: 0,
                            },
                        )),
                    )
                    .unwrap();
            }
        }
        for millis in [50, 10, 10, 30] {
            for world in &mut worlds {
                prepare(world, millis);
            }
            assert_unaffected(&worlds[1], &worlds[0], if custom { 1 } else { 2 });
        }
    }
}

/// Models the committed-tick admission used by the four production interaction owners.
fn admit_frame_swing(
    movement: Res<crate::movement::MovementTicker>,
    effects: Res<crate::movement::LocalMovementEffectTimeline>,
    mut swings: ResMut<crate::melee::SwingTracker>,
    mut accepted: ResMut<SwingAdmission>,
) {
    swings.sync_ticks(
        movement.interaction_authority_identity(),
        movement.completed_tick(),
        &effects,
    );
    accepted.0 = swings.try_swing(
        movement.completed_tick(),
        gameplay::melee::swing_duration(effects.mining_effects()),
    );
}

#[derive(Resource, Default)]
struct SwingAdmission(bool);

#[test]
fn production_actor_preparation_preserves_same_frame_swing_admission() {
    let mut app = App::new();
    crate::app::configure_client_frame_schedule(&mut app);
    crate::app::configure_actor_render_systems(&mut app);
    app.add_systems(
        Update,
        admit_frame_swing.in_set(crate::app::ClientFrameSet::NetworkSend),
    );
    let mut schedule = app
        .world_mut()
        .resource_mut::<bevy::ecs::schedule::Schedules>()
        .remove(Update)
        .unwrap();
    let mut world = fixture();
    let mut movement = crate::movement::MovementTicker::default();
    *movement = gameplay::test_support::survival_mining::ticker_with_ticks(1);
    world.insert_resource(movement);
    world.init_resource::<crate::movement::LocalMovementEffectTimeline>();
    world.init_resource::<crate::melee::SwingTracker>();
    world.init_resource::<SwingAdmission>();
    world.init_resource::<render::ActorRenderFrame>();
    world.init_resource::<render::ActorRuntimeWitness>();
    let states = [
        (true, [0.0, 0.0]),
        (false, [0.0, 1.0 / client_world::ACTOR_SWING_TICKS as f32]),
        (
            false,
            [
                1.0 / client_world::ACTOR_SWING_TICKS as f32,
                2.0 / client_world::ACTOR_SWING_TICKS as f32,
            ],
        ),
        (
            false,
            [
                2.0 / client_world::ACTOR_SWING_TICKS as f32,
                3.0 / client_world::ACTOR_SWING_TICKS as f32,
            ],
        ),
        (true, [3.0 / client_world::ACTOR_SWING_TICKS as f32, 0.0]),
        (false, [0.0, 1.0 / client_world::ACTOR_SWING_TICKS as f32]),
    ];
    for (index, (admitted, expected)) in states.into_iter().enumerate() {
        *world.resource_mut::<crate::movement::MovementTicker>() = {
            let mut movement = crate::movement::MovementTicker::default();
            *movement =
                gameplay::test_support::survival_mining::ticker_with_ticks(index as u64 + 1);
            movement
        };
        let before = world
            .resource::<ClientWorld>()
            .stream
            .as_ref()
            .unwrap()
            .authority()
            .actor_rig(2)
            .unwrap()
            .completed_tick;
        world
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(50));
        schedule.run(&mut world);
        assert_eq!(
            world.resource::<SwingAdmission>().0,
            admitted,
            "the current committed tick must admit before publication at frame {index}"
        );
        let stream = world.resource::<ClientWorld>().stream.as_ref().unwrap();
        let local = stream.authority().actor_rig(1).unwrap();
        assert_eq!(
            local.java.swing, expected,
            "same-frame Java samples at frame {index}"
        );
        assert_eq!(
            local.hand.map(|hand| hand.attack_time),
            expected,
            "same-frame Bedrock samples at frame {index}"
        );
        assert_eq!(
            stream.authority().actor_rig(2).unwrap().completed_tick,
            before + 1,
            "final pose refresh must not advance remote ticks"
        );
    }
}

/// Observes the exact current-frame hand readiness consumed by the UI owner.
fn observe_hand_readiness(
    actor: Res<client_presentation::actor_publication::ActorFrameState>,
    mut readiness: ResMut<HandReadiness>,
) {
    readiness.0 = actor.hand_is_active();
}

#[derive(Resource, Default)]
struct HandReadiness(bool);

#[test]
fn production_early_ui_readiness_matches_the_current_hand_source() {
    let mut app = App::new();
    crate::app::configure_client_frame_schedule(&mut app);
    crate::app::configure_actor_render_systems(&mut app);
    app.add_systems(
        Update,
        observe_hand_readiness.in_set(crate::app::ClientFrameSet::UiPreparation),
    );
    let mut schedule = app
        .world_mut()
        .resource_mut::<bevy::ecs::schedule::Schedules>()
        .remove(Update)
        .unwrap();
    let mut world = fixture_with_skin(true);
    world.init_resource::<HandReadiness>();
    world.init_resource::<render::ActorRenderFrame>();
    world.init_resource::<render::ActorRuntimeWitness>();
    for (index, (mode, active)) in [
        (PerspectiveMode::FirstPerson, true),
        (PerspectiveMode::ThirdPersonBack, false),
        (PerspectiveMode::FirstPerson, true),
    ]
    .into_iter()
    .enumerate()
    {
        perspective(&mut world, mode, index as u64 + 1);
        world
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(50));
        schedule.run(&mut world);
        assert_eq!(world.resource::<HandReadiness>().0, active);
        assert_eq!(world.resource::<render::HandRigScene>().is_active(), active);
    }
}

#[path = "custom_emotes/local_torso.rs"]
mod local_torso;
#[path = "custom_emotes/pick_positions.rs"]
mod pick_positions;
