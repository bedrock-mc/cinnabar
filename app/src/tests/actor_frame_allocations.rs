//! Steady actor frames must not allocate per drawn actor.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use bevy::{
    math::Vec3,
    prelude::{PerspectiveProjection, Projection, Transform, World},
    time::{Real, Time},
};
use protocol::{ActorEvent, ActorKind, ActorSpawnEvent, WorldEvent};
use render::{ActorArtworkPages, ActorRenderFrame, ActorRenderScene};

use super::actor_rest_presentation::{compiled_fixture, stream};
use crate::runtime::network::{
    ActorFramePartialTick, HandRigBuilder, prepare_actor_render_frame, publish_actor_render_frame,
};

/// A world holding every resource the actor publication system reads, with a perspective camera
/// at `eye` looking at `target`.
pub(crate) fn actor_frame_world(
    client_world: crate::runtime::world::ClientWorld,
    scene: ActorRenderScene,
    artwork: ActorArtworkPages,
    hand: HandRigBuilder,
    (eye, target): (Vec3, Vec3),
) -> World {
    let mut world = World::new();
    world.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
    world.insert_resource(client_world);
    world.insert_resource(Time::<Real>::new(Instant::now()));
    world.insert_resource(scene);
    world.insert_resource(ActorRenderFrame::default());
    world.insert_resource(crate::local_player::LocalAvatarPresentation::default());
    world.insert_resource(crate::local_player::LocalAvatarVisibilityCarrier::default());
    world.insert_resource(crate::camera::CameraSettingsAuthority::default());
    world.insert_resource(crate::movement::LocalPhysicsController::default());
    world.insert_resource(render::ActorRuntimeWitness::default());
    world.insert_resource(artwork);
    world.insert_resource(hand);
    world.insert_resource(render::HandRigScene::default());
    world.insert_resource(crate::player_skin::LocalPlayerSkin::generated_default(
        "bench",
    ));
    world.insert_resource(ActorFramePartialTick::default());
    world.init_resource::<crate::runtime::network::PreparedActorPublication>();
    let camera = Transform::from_translation(eye).looking_at(target, Vec3::Y);
    world.spawn((
        camera,
        Projection::Perspective(PerspectiveProjection {
            fov: 70f32.to_radians(),
            aspect_ratio: 16.0 / 9.0,
            ..Default::default()
        }),
        crate::camera::FlyCamera::default(),
    ));
    world.insert_resource(crate::local_player::LocalViewPose::new(
        eye,
        camera.rotation,
    ));
    world
}

fn spawn(runtime_id: u64, position: [f32; 3]) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id: -(runtime_id as i64),
        runtime_id,
        kind: ActorKind::Entity {
            identifier: "minecraft:example".into(),
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
    }))
}

/// The most allocations any frame without an animation tick made, once `actors` animated
/// actors stand in view.
fn steady_frame_allocations(actors: u64) -> u64 {
    let (_pack, artwork, entities) =
        compiled_fixture("1.0", 1, assets::ActorPoseMode::CompiledLiteral);
    let mut stream = stream(Arc::clone(&entities));
    for index in 0..actors {
        let position = [
            (index % 8) as f32 * 2.0 - 7.0,
            64.0,
            (index / 8) as f32 * 2.0,
        ];
        stream
            .submit(index + 1, spawn(index + 100, position))
            .unwrap();
    }
    let mut scene =
        ActorRenderScene::with_runtime_entity_assets_and_equipment(&entities, &[]).unwrap();
    scene.configure_artwork(artwork.clone());
    let mut client_world = crate::runtime::world::ClientWorld::new_with_entity_assets(
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Arc::clone(&entities),
    );
    client_world.stream = Some(stream);
    let mut world = actor_frame_world(
        client_world,
        scene,
        artwork,
        HandRigBuilder::from_runtime_assets(&entities).unwrap(),
        (Vec3::new(0.0, 66.0, -12.0), Vec3::new(0.0, 64.0, 4.0)),
    );
    let tick = |world: &World| {
        world
            .resource::<crate::runtime::world::ClientWorld>()
            .stream
            .as_ref()
            .and_then(|stream| stream.actor_rig(100))
            .map(|rig| rig.completed_tick)
    };
    let mut clock = Instant::now();
    let mut worst = 0;
    for frame in 0..60 {
        clock += Duration::from_nanos(16_666_667);
        world
            .resource_mut::<Time<Real>>()
            .update_with_instant(clock);
        let before_tick = tick(&world);
        let before = super::alloc_count::thread_allocations();
        world.run_system_cached(prepare_actor_render_frame).unwrap();
        world.run_system_cached(publish_actor_render_frame).unwrap();
        let allocated = super::alloc_count::thread_allocations() - before;
        if frame >= 30 && tick(&world) == before_tick {
            worst = worst.max(allocated);
        }
    }
    let drawn = world.resource::<ActorRenderFrame>().rig.instances.len();
    assert_eq!(drawn as u64, actors, "every actor stands in view");
    worst
}

/// Converted poses, layer poses, bone matrices and build buffers persist across the frames of a
/// tick, so a frame without a tick allocates a bounded amount however many actors it draws.
#[test]
fn steady_actor_frames_do_not_allocate_per_actor() {
    let few = steady_frame_allocations(4);
    let many = steady_frame_allocations(48);
    // Only buffer growth may differ; one allocation per actor would add 44.
    assert!(
        many <= few + 8,
        "4 actors: {few} allocations, 48 actors: {many}"
    );
    assert!(many <= 24, "48 actors: {many} allocations");
}
