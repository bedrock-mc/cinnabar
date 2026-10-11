//! Composes committed debug drawing with the session, player pose and active font.

use bevy::{ecs::system::SystemParam, prelude::*};
use client_presentation::{
    actor_publication::ActorFramePartialTick, primitive_shapes::PrimitiveShapePublisher,
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use render::{PrimitiveShapesRenderPlugin, PrimitiveShapesScene};
use {
    crate::{app::ClientFrameSet, runtime::world::ClientWorld},
    client_presentation::local_player::LocalViewPose,
};

pub(crate) fn configure(app: &mut App) {
    app.add_plugins(PrimitiveShapesRenderPlugin);
    schedule_publication(app);
}

/// Samples attachments after actor interpolation advances, so shapes share the actors' rendered pose.
fn schedule_publication(app: &mut App) {
    app.add_systems(
        Update,
        publish
            .after(ClientFrameSet::ActorPreparation)
            .before(ClientFrameSet::UiPreparation),
    );
}

/// The clock and viewer pose a frame's shapes are placed with.
#[derive(SystemParam)]
struct FrameClock<'w> {
    time: Res<'w, Time>,
    partial: Res<'w, ActorFramePartialTick>,
    view: Res<'w, LocalViewPose>,
}

/// Shares retained state across render extraction without cloning or walking shape records.
fn publish(
    mut world: ResMut<ClientWorld>,
    mut scene: ResMut<PrimitiveShapesScene>,
    clock: FrameClock,
    mut presentation: ResMut<UiPresentationRuntime>,
    runtime: Res<UiRuntime>,
    mut publisher: Local<PrimitiveShapePublisher>,
) {
    publisher.publish(
        world.stream.as_mut(),
        &mut scene,
        clock.time.elapsed_secs(),
        clock.partial.0,
        Some(clock.view.feet_translation().to_array()),
    );
    let mut store = scene.store.lock().expect("primitive store lock poisoned");
    presentation.prepare_primitive_text(&mut store, &runtime);
}

/// Exposes retained drawing counters only when a developer-control snapshot is requested.
#[cfg(feature = "developer-control")]
pub(crate) fn snapshot(world: &World) -> serde_json::Value {
    let Some(scene) = world.get_resource::<PrimitiveShapesScene>() else {
        return serde_json::Value::Null;
    };
    let store = scene.store.lock().expect("primitive store lock poisoned");
    serde_json::json!({
        "shapes": store.len(),
        "skipped_entries": store.skipped_entries,
        "instance_rebuilds": store.instance_rebuilds,
        "mesh_batches": store.batches().iter().filter(|batch| !batch.instances.is_empty()).count(),
        "pending_uploads": store.has_changes(),
        "dimension": scene.dimension,
        "render_distance": scene.render_distance,
    })
}

#[cfg(test)]
mod tests {
    use chunk_pipeline::WorldStream;
    use protocol::{
        ActorEvent, ActorKind, ActorMoveEvent, ActorPositionOrigin, ActorSpawnEvent,
        PrimitiveShapeChange, PrimitiveShapeData, PrimitiveShapeKind, PrimitiveShapeUpdate,
        PrimitiveShapesEvent, WorldBootstrap, WorldEvent,
    };
    use std::sync::Arc;
    use {super::*, client_presentation::local_player::LocalViewPose};

    const ACTOR: i64 = 17;

    /// Stands in for actor preparation: advances interpolation and the frame's partial tick.
    fn advance_actor(mut world: ResMut<ClientWorld>, mut partial: ResMut<ActorFramePartialTick>) {
        world
            .stream
            .as_mut()
            .unwrap()
            .advance_actor_interpolation_frame(1);
        partial.0 = 0.5;
    }

    fn rendered(app: &App) -> [f32; 3] {
        let stream = app
            .world()
            .resource::<ClientWorld>()
            .stream
            .as_ref()
            .unwrap();
        let partial = app.world().resource::<ActorFramePartialTick>().0;
        stream
            .authority()
            .actor_by_unique_id(ACTOR)
            .unwrap()
            .interpolated_position(partial)
            .unwrap()
    }

    fn stream() -> WorldStream {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        });
        let spawn = ActorSpawnEvent {
            dimension: 0,
            unique_id: ACTOR,
            runtime_id: 7,
            kind: ActorKind::Entity {
                identifier: "minecraft:pig".into(),
            },
            position: [0.0, 64.0, 0.0],
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
        };
        stream
            .submit(1, WorldEvent::Actor(ActorEvent::Spawn(spawn)))
            .unwrap();
        stream
            .submit(
                2,
                WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
                    dimension: 0,
                    runtime_id: 7,
                    position: [Some(8.0), Some(64.0), Some(0.0)],
                    position_origin: ActorPositionOrigin::Feet,
                    pitch: None,
                    yaw: None,
                    head_yaw: None,
                    on_ground: Some(true),
                    teleported: false,
                    player_mode: None,
                    source_tick: None,
                    interpolation: Default::default(),
                })),
            )
            .unwrap();
        stream
            .submit(
                3,
                WorldEvent::PrimitiveShapes(PrimitiveShapesEvent {
                    changes: vec![PrimitiveShapeChange::Upsert(PrimitiveShapeUpdate {
                        network_id: 1,
                        kind: PrimitiveShapeKind::Line,
                        location: None,
                        rotation: None,
                        scale: None,
                        color: None,
                        total_time_left: None,
                        maximum_render_distance: None,
                        dimension: None,
                        attached_actor: Some(ACTOR),
                        data: PrimitiveShapeData::Line { end: [1.0; 3] },
                    })],
                    skipped_entries: 0,
                }),
            )
            .unwrap();
        stream
    }

    #[test]
    fn attached_shape_uses_the_actor_pose_rendered_in_the_same_frame() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<PrimitiveShapesScene>()
            .init_resource::<ActorFramePartialTick>()
            .init_resource::<LocalViewPose>()
            .insert_resource(UiRuntime::new(1))
            .insert_resource(
                UiPresentationRuntime::new(client_ui::test_support::fixture_font()).unwrap(),
            )
            .insert_resource(ClientWorld {
                stream: Some(stream()),
                ..Default::default()
            })
            .add_systems(
                Update,
                advance_actor.in_set(ClientFrameSet::ActorPreparation),
            );
        crate::app::configure_client_frame_schedule(&mut app);
        schedule_publication(&mut app);
        let before = rendered(&app);
        app.update();
        let scene = app.world().resource::<PrimitiveShapesScene>();
        let published = scene.store.lock().unwrap().actors.values[0];
        assert_ne!(rendered(&app), before, "the frame must move the actor");
        assert_eq!(published.position, rendered(&app));
        assert_eq!(published.valid, 1);
    }
}
