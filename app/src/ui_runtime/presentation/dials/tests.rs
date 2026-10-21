//! Dial publication regressions through real completed physics observations.

use super::*;
use bevy::{
    math::Quat,
    prelude::{App, Update},
};

#[test]
fn reanchor_tick_indices_do_not_synthesize_needle_updates() {
    let reading = |needles: &mut DialNeedles| {
        needles.advance(1, [0, 64, 10], None, [0.0, 64.0, 0.0], 0.0, 0.25)
    };
    let mut reanchored = DialNeedles::default();
    let mut consecutive = DialNeedles::default();
    assert_eq!(
        reanchored.advance_tick(0, reading),
        consecutive.advance_tick(0, reading)
    );
    assert_eq!(
        reanchored.advance_tick(u64::MAX, reading),
        consecutive.advance_tick(1, reading)
    );
    assert_eq!(reanchored.compass, consecutive.compass);
    assert_eq!(reanchored.recovery_compass, consecutive.recovery_compass);
    assert_eq!(reanchored.clock, consecutive.clock);
    assert_eq!(reanchored.random, consecutive.random);
    assert_eq!(reanchored.advance_tick(u64::MAX, reading), None);
    assert_eq!(reanchored.random, consecutive.random);
    assert!(
        reanchored.advance_tick(0, reading).is_some(),
        "a restarted clock takes a reading"
    );
}

#[test]
fn overworld_dials_read_spawn_and_sky_while_other_dimensions_spin() {
    let mut needles = DialNeedles::default();
    let mut overworld = needles.advance(0, [0, 64, 10], None, [0.0, 64.0, 0.0], 0.0, 0.25);
    for _ in 0..200 {
        overworld = needles.advance(0, [0, 64, 10], None, [0.0, 64.0, 0.0], 0.0, 0.25);
    }
    assert_eq!(
        overworld.compass,
        Some(16),
        "settles on the spawn straight ahead"
    );
    assert_eq!(overworld.clock, Some(16), "settles on the sky reading");

    let mut nether = DialNeedles::default();
    let readings: Vec<_> = (0..40)
        .map(|_| nether.advance(1, [0, 64, 10], None, [0.0, 64.0, 0.0], 0.0, 0.25))
        .collect();
    assert!(
        readings
            .windows(2)
            .any(|pair| pair[0].compass != pair[1].compass),
        "the compass wanders without a target"
    );
}

#[test]
fn recovery_compasses_aim_at_a_death_in_this_dimension_and_spin_otherwise() {
    let mut needles = DialNeedles::default();
    let mut frames = DialFrames::default();
    for _ in 0..200 {
        frames = needles.advance(
            0,
            [0, 64, -10],
            Some(([10, 64, 0], 0)),
            [0.0, 64.0, 0.0],
            0.0,
            0.25,
        );
    }
    assert_eq!(frames.recovery_compass, Some(8), "a death to the east");
    assert_ne!(
        frames.recovery_compass, frames.compass,
        "spawn lies elsewhere"
    );

    let mut elsewhere = DialNeedles::default();
    let readings: Vec<_> = (0..40)
        .map(|_| {
            elsewhere.advance(
                0,
                [0, 64, -10],
                Some(([10, 64, 0], 1)),
                [0.0, 64.0, 0.0],
                0.0,
                0.25,
            )
        })
        .collect();
    assert!(
        readings
            .windows(2)
            .any(|pair| pair[0].recovery_compass != pair[1].recovery_compass),
        "a death in another dimension leaves the needle wandering"
    );
}

use client_interaction::movement::LocalPhysicsController;
use client_presentation::local_player::LocalViewPose;
use client_ui::ui_runtime::presentation::UiPresentationRuntime;
use std::sync::Arc;

/// A loaded, empty collision field lets the real controller complete every requested tick.
struct EmptyWorld;

impl sim::CollisionWorld for EmptyWorld {
    /// Returns a coherent empty collision query without loading world carriers.
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }
}

/// Runs the production dial system with a real local actor and a frozen presentation clock.
fn dial_app(dimension: i32) -> App {
    let position = [0.0, 64.0, 0.0];
    let assets = Arc::new(assets::RuntimeAssets::diagnostic());
    let mut stream = chunk_pipeline::WorldStream::new_with_assets(
        protocol::WorldBootstrap {
            dimension,
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            player_position: position,
            world_spawn_position: [0, 64, 10],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        assets.clone(),
        position,
        None,
    );
    stream
        .submit(
            1,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension,
                unique_id: 1,
                runtime_id: 1,
                kind: protocol::ActorKind::Player {
                    uuid: [1; 16],
                    username: "Tester".into(),
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
    assert!(stream.authority().actor(1).is_some());
    let mut world = ClientWorld::new(assets);
    world.stream = Some(stream);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(position, 0, false);
    let mut app = App::new();
    app.insert_resource(world)
        .insert_resource(physics)
        .init_resource::<LocalViewPose>()
        .init_resource::<client_presentation::environment::WorldClock>()
        .init_resource::<Time<Real>>()
        .insert_resource(UiPresentationResource(
            UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap(),
        ))
        .add_systems(Update, advance_dials);
    app
}

/// Advances actual local prediction and verifies that the fixture produced the intended ticks.
fn complete_ticks(app: &mut App, ticks: usize) {
    let frame = app
        .world_mut()
        .resource_mut::<LocalPhysicsController>()
        .advance(
            simulation_clock::TICK_DURATION * ticks as u32,
            sim::MovementInput::default(),
            &EmptyWorld,
        );
    assert_eq!(frame.samples.len(), ticks);
}

/// Reads the strip frames published to every inventory dial icon.
fn published_frames(app: &App) -> DialFrames {
    app.world()
        .resource::<UiPresentationResource>()
        .dial_frames()
}

/// Replaces the network anchor without pretending that the numeric gap was simulated.
fn reanchor(app: &mut App, tick: u64) {
    app.world_mut()
        .resource_mut::<LocalPhysicsController>()
        .reanchor_network_position([0.0, 64.0, 0.0], tick, false);
}

#[test]
fn production_dials_consume_each_completed_tick_once_and_bound_reanchors() {
    let mut batched = dial_app(1);
    let mut consecutive = dial_app(1);
    batched.update();
    consecutive.update();
    assert_eq!(published_frames(&batched), published_frames(&consecutive));
    assert!(published_frames(&batched).compass.is_some());

    for _ in 0..16 {
        complete_ticks(&mut batched, 3);
        batched.update();
        for _ in 0..3 {
            complete_ticks(&mut consecutive, 1);
            consecutive.update();
        }
        assert_eq!(published_frames(&batched), published_frames(&consecutive));
        let before = published_frames(&batched);
        batched.update();
        assert_eq!(
            published_frames(&batched),
            before,
            "no new tick keeps the reading"
        );
    }

    let tick = consecutive
        .world()
        .resource::<LocalPhysicsController>()
        .state()
        .unwrap()
        .tick;
    reanchor(&mut batched, tick + 1024);
    reanchor(&mut consecutive, tick + 1);
    batched.update();
    consecutive.update();
    assert_eq!(published_frames(&batched), published_frames(&consecutive));
    let before = published_frames(&batched);
    batched.update();
    assert_eq!(
        published_frames(&batched),
        before,
        "an anchor is consumed once"
    );

    reanchor(&mut batched, 0);
    reanchor(&mut consecutive, tick + 2);
    batched.update();
    consecutive.update();
    assert_eq!(published_frames(&batched), published_frames(&consecutive));
    complete_ticks(&mut batched, 1);
    complete_ticks(&mut consecutive, 1);
    batched.update();
    consecutive.update();
    assert_eq!(published_frames(&batched), published_frames(&consecutive));
}

#[test]
fn production_compass_reads_current_view_facing_instead_of_actor_store_yaw() {
    let mut app = dial_app(0);
    let mut expected = DialNeedle::default();
    let reading = |yaw| CompassReading::Target {
        target: [0, 10],
        position: [0.0, 0.0],
        yaw,
    };
    app.update();
    assert_eq!(
        published_frames(&app).compass,
        Some(expected.update_compass(reading(180.0), false))
    );
    let before = published_frames(&app);
    app.world_mut()
        .resource_mut::<LocalViewPose>()
        .set_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
    let (yaw, _, _) = app
        .world()
        .resource::<LocalViewPose>()
        .rotation()
        .to_euler(EulerRot::YXZ);
    let bedrock_yaw = (180.0 - yaw.to_degrees()).rem_euclid(360.0);
    app.update();
    assert_eq!(
        published_frames(&app),
        before,
        "look changes wait for a completed tick"
    );
    for _ in 0..200 {
        complete_ticks(&mut app, 1);
        app.update();
        assert_eq!(
            published_frames(&app).compass,
            Some(expected.update_compass(reading(bedrock_yaw), false))
        );
    }
}
