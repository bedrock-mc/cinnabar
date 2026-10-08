//! Local Java torso observations joined to actual committed physics history.
use super::*;

/// Admits one attack after early actor preparation and leaves later ticks to complete it.
fn admit_once(
    movement: Res<crate::movement::MovementTicker>,
    effects: Res<crate::movement::LocalMovementEffectTimeline>,
    mut swings: ResMut<crate::melee::SwingTracker>,
) {
    swings.sync_ticks(
        movement.interaction_authority_identity(),
        movement.completed_tick(),
        &effects,
    );
    if movement.completed_tick() == 101 {
        assert!(swings.try_swing(
            101,
            gameplay::melee::swing_duration(effects.mining_effects())
        ));
    }
}

/// Advances real fixed physics and admits its retained samples through movement authority.
fn physics(world: &mut World, millis: u64, yaw: f32) {
    let terrain = sim::ScenarioWorld {
        name: "stationary torso".into(),
        origin: [0; 3],
        revision: 1,
        boxes: Box::new([]),
        physics: sim::BlockPhysicsFacts {
            friction: 1.0,
            horizontal_speed_factor: 1.0,
            vertical_speed_factor: 1.0,
            fluid_height_blocks: 0.0,
            flags: Default::default(),
            surface_response: sim::SurfaceResponse::None,
        },
        unloaded: false,
    };
    let frame = world.resource_scope(
        |world, mut effects: Mut<crate::movement::LocalMovementEffectTimeline>| {
            effects.begin_frame();
            world
                .resource_mut::<crate::movement::LocalPhysicsController>()
                .advance_with_context_and_effects(
                    Duration::from_millis(millis),
                    sim::MovementInput {
                        immobile: true,
                        yaw_degrees: f64::from(yaw),
                        ..Default::default()
                    },
                    gameplay::movement::PhysicsSampleContext {
                        head_yaw: yaw,
                        ..Default::default()
                    },
                    &terrain,
                    &mut **effects,
                )
        },
    );
    assert!(frame.blocked.is_none());
    for sample in frame.samples {
        assert_eq!(sample.movement, [0.0; 3]);
        assert_eq!(sample.yaw, yaw);
        world
            .resource_mut::<crate::movement::MovementTicker>()
            .enqueue_completed_physics(sample)
            .unwrap();
    }
}

#[test]
fn production_java_torso_joins_physics_history_after_same_frame_admission() {
    for (effect, duration, final_heading) in [
        (None, client_world::ACTOR_SWING_TICKS, 24.9579),
        (Some((3, 1)), 4, 19.71),
        (Some((4, 0)), 8, 24.9579),
    ] {
        let mut app = App::new();
        crate::app::configure_client_frame_schedule(&mut app);
        crate::app::configure_actor_render_systems(&mut app);
        app.add_systems(
            Update,
            admit_once.in_set(crate::app::ClientFrameSet::NetworkSend),
        );
        let mut schedule = app
            .world_mut()
            .resource_mut::<bevy::ecs::schedule::Schedules>()
            .remove(Update)
            .unwrap();
        let mut world = fixture_with_skin(true);
        perspective(&mut world, PerspectiveMode::ThirdPersonBack, 1);
        prepare(&mut world, 100);
        let anchor = world
            .resource::<crate::movement::LocalPhysicsController>()
            .network_position()
            .unwrap();
        world
            .resource_mut::<crate::movement::LocalPhysicsController>()
            .reanchor_network_position(anchor, 100, true);
        let mut movement = crate::movement::MovementTicker::default();
        movement.reset(1, 100, anchor);
        movement.set_source(gameplay::movement::MovementSource::Physics);
        world.insert_resource(movement);
        world.init_resource::<crate::movement::LocalMovementEffectTimeline>();
        world.init_resource::<crate::melee::SwingTracker>();
        world.init_resource::<render::ActorRenderFrame>();
        world.init_resource::<render::ActorRuntimeWitness>();
        {
            let mut effects = world.resource_mut::<crate::movement::LocalMovementEffectTimeline>();
            effects.begin_session(1);
            if let Some((effect_id, amplifier)) = effect {
                effects.apply(
                    1,
                    1,
                    protocol::ActorEffectEvent {
                        dimension: 0,
                        actor_runtime_id: 1,
                        action: protocol::ActorEffectAction::Add,
                        effect_id,
                        amplifier,
                        particles: false,
                        ambient: false,
                        duration_ticks: 100,
                        tick: 100,
                    },
                );
            }
        }
        for (physical_millis, actor_millis, yaw, expected) in [
            (75, 0, 0.0, 0.0),
            (50, 250, 30.0, 9.0),
            (200, 0, 30.0, final_heading),
        ] {
            physics(&mut world, physical_millis, yaw);
            world
                .resource_mut::<Time<Real>>()
                .advance_by(Duration::from_millis(actor_millis));
            let actor_tick = world
                .resource::<ClientWorld>()
                .stream
                .as_ref()
                .unwrap()
                .authority()
                .actor_rig(1)
                .unwrap()
                .completed_tick;
            schedule.run(&mut world);
            let stream = world.resource::<ClientWorld>().stream.as_ref().unwrap();
            let rig = stream.authority().actor_rig(1).unwrap();
            assert_eq!(rig.completed_tick, actor_tick + actor_millis / 50);
            assert!(
                (rig.java.body_yaw[1] - expected).abs() < 1e-4,
                "duration {duration}: {:?}",
                rig.java.body_yaw
            );
            assert_eq!(rig.java.body_frame_alpha, Some(0.5));
            if expected == 9.0 {
                assert_eq!(rig.java.swing, [0.0, 1.0 / duration as f32]);
                assert!((rig.java.body_yaw_at(0.0) - 4.5).abs() < 1e-5);
            }
        }
    }
}
