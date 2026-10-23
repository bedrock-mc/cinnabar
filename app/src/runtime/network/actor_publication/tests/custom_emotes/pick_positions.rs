//! Interaction picks remote actors at this frame's tick positions without disturbing their
//! per-tick animation or the actor clock.
use gameplay::movement::MovementEffectSource;
use protocol_ids::ActorDataKey;
use {super::*, client_presentation::actor_publication::ActorFramePartialTick};

#[path = "pick_positions/paused_motion.rs"]
mod paused_motion;

#[derive(Resource, Default)]
struct Picked(Option<u64>);

#[derive(Resource, Default)]
struct PredictedBox(Option<([f32; 3], [f32; 3])>);

/// Captures the custom interaction box before live actor ticks advance.
fn capture_predicted_box(world: Res<ClientWorld>, mut predicted: ResMut<PredictedBox>) {
    let authority = world.stream.as_ref().unwrap().authority();
    predicted.0 = authority.pick_hit_boxes(authority.actor(2).unwrap()).next();
}

/// Casts a melee pick along +Z from between the actor's old and current-tick positions.
fn pick_ahead(world: Res<ClientWorld>, mut picked: ResMut<Picked>) {
    let authority = world.stream.as_ref().unwrap().authority();
    picked.0 = gameplay::melee::pick_actor_by(
        authority.remote_actors(),
        |actor| authority.pick_hit_boxes(actor),
        None,
        [2.0, 65.0, 1.5],
        [0.0, 0.0, 1.0],
        3.0,
    )
    .map(|hit| hit.runtime_id);
}

/// Moves interpolate over at least three ticks, so one tick covers a third of the way.
fn move_actor(runtime_id: u64, position: [f32; 3], yaw: f32) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Move(protocol::ActorMoveEvent {
        dimension: 0,
        runtime_id,
        position: position.map(Some),
        position_origin: protocol::ActorPositionOrigin::Feet,
        pitch: None,
        yaw: Some(yaw),
        head_yaw: Some(yaw),
        on_ground: None,
        teleported: false,
        force_local_move: false,
        player_mode: None,
        source_tick: None,
        interpolation: protocol::ActorInterpolation {
            ticks: 3,
            force_completion: false,
        },
    }))
}

/// Actor-only production boundaries use the fallback clock unless a test adds prediction.
fn production(app: &mut App) -> (bevy::ecs::schedule::Schedule, World) {
    crate::game_frame::configure_client_frame_schedule(app);
    crate::game_frame::configure_actor_render_systems(app);
    let schedule = app
        .world_mut()
        .resource_mut::<bevy::ecs::schedule::Schedules>()
        .remove(Update)
        .unwrap();
    let mut world = fixture();
    world.insert_resource(client_interaction::movement::LocalPhysicsController::default());
    let mut movement = client_interaction::movement::MovementTicker::default();
    *movement = gameplay::test_support::survival_mining::ticker_with_ticks(1);
    world.insert_resource(movement);
    world.init_resource::<client_interaction::movement::LocalMovementEffectTimeline>();
    world.init_resource::<client_interaction::melee::SwingTracker>();
    world.init_resource::<render_scene::ActorRenderFrame>();
    world.init_resource::<render_scene::ActorRuntimeWitness>();
    (schedule, world)
}

/// A melee press must hit an actor where its due interpolation tick puts it this frame.
#[test]
fn production_melee_picks_remote_actors_at_current_frame_positions() {
    let mut app = App::new();
    app.add_systems(
        Update,
        pick_ahead.in_set(client_state::ClientFrameSet::NetworkSend),
    );
    let (mut schedule, mut world) = production(&mut app);
    world.init_resource::<Picked>();
    {
        let mut client = world.resource_mut::<ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        stream
            .submit(3, move_actor(2, [2.0, 64.0, 9.0], 0.0))
            .unwrap();
        stream.poll([0.0, 64.0, 0.0], 0);
        assert_eq!(
            stream.authority().actor(2).unwrap().position[2],
            0.0,
            "the move waits for its interpolation tick"
        );
    }
    world
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(50));
    schedule.run(&mut world);

    assert_eq!(world.resource::<Picked>().0, Some(2));
}

/// A stalled frame predicts custom hitboxes at the same capped tick that live actors reach.
#[test]
fn stalled_frame_caps_live_ticks_and_custom_hitbox_prediction_together() {
    let mut app = App::new();
    app.add_systems(
        Update,
        capture_predicted_box.in_set(client_state::ClientFrameSet::NetworkSend),
    );
    let (mut schedule, mut world) = production(&mut app);
    world.init_resource::<PredictedBox>();
    let cap = u32::try_from(
        simulation_clock::MAX_FRAME_ELAPSED.as_nanos() / simulation_clock::TICK_DURATION.as_nanos(),
    )
    .unwrap();
    let initial_tick = {
        let mut client = world.resource_mut::<ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        let mut hitbox = world::NbtCompound::default();
        for axis in ["X", "Y", "Z"] {
            hitbox.insert(format!("Max{axis}"), world::NbtValue::Float(1.0));
            hitbox.insert(format!("Pivot{axis}"), world::NbtValue::Float(2.0));
        }
        let mut root = world::NbtCompound::default();
        root.insert(
            "Hitboxes",
            world::NbtValue::List(vec![world::NbtValue::Compound(hitbox)]),
        );
        stream
            .submit(
                3,
                WorldEvent::Actor(ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 2,
                    metadata: Arc::from([protocol::ActorMetadata {
                        key: ActorDataKey::HitBox.wire_value(),
                        value: protocol::ActorMetadataValue::Compound(
                            root.encode_root().unwrap().into(),
                        ),
                    }]),
                    properties: Arc::from([]),
                    tick: 0,
                })),
            )
            .unwrap();
        let WorldEvent::Actor(ActorEvent::Move(mut movement)) =
            move_actor(2, [2.0, 64.0, (cap * 2) as f32], 90.0)
        else {
            unreachable!()
        };
        movement.interpolation.ticks = u64::from(cap) * 2;
        stream
            .submit(4, WorldEvent::Actor(ActorEvent::Move(movement)))
            .unwrap();
        stream.poll([0.0, 64.0, 0.0], 0);
        stream.authority().actor_rig(2).unwrap().completed_tick
    };
    let tick = simulation_clock::TICK_DURATION;
    world
        .resource_mut::<Time<Real>>()
        .advance_by(tick * (cap * 4) + tick / 2);
    schedule.run(&mut world);

    let client = world.resource::<ClientWorld>();
    let authority = client.stream.as_ref().unwrap().authority();
    let actor = authority.actor(2).unwrap();
    assert_eq!(
        authority.actor_rig(2).unwrap().completed_tick - initial_tick,
        u64::from(cap)
    );
    assert!((actor.position[2] - cap as f32).abs() < 1e-4);
    assert_eq!(world.resource::<PredictedBox>().0, actor.hit_boxes().next());
    assert_eq!(world.resource::<ActorFramePartialTick>().0, 0.0);
}

/// Predicting picks for a three-tick frame leaves every per-tick input of that frame unchanged:
/// positions, status, dragon history, walk cycle and body yaw.
#[test]
fn pick_prediction_keeps_multi_tick_animation_inputs() {
    let advance = |predict: bool| {
        let mut world = fixture();
        let mut client = world.resource_mut::<ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        stream
            .submit(
                3,
                WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                    dimension: 0,
                    unique_id: 4,
                    runtime_id: 4,
                    kind: ActorKind::Entity {
                        identifier: "minecraft:ender_dragon".into(),
                    },
                    position: [0.0, 70.0, 0.0],
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
        stream.prepare_actor_appearance_fixture();
        stream.advance_actor_interpolation_frame(1);
        stream
            .submit(4, move_actor(2, [2.0, 64.0, 9.0], 90.0))
            .unwrap();
        stream
            .submit(5, move_actor(4, [6.0, 72.0, 3.0], 45.0))
            .unwrap();
        stream.poll([0.0, 64.0, 0.0], 0);
        if predict {
            let before = [2, 4].map(|id| stream.authority().actor(id).cloned());
            stream.predict_remote_actor_motion(3);
            assert_eq!(
                [2, 4].map(|id| stream.authority().actor(id).cloned()),
                before,
                "pick prediction leaves authoritative actor state unchanged"
            );
        }
        stream.advance_actor_interpolation_frame(3);
        let authority = stream.authority();
        let rig = authority.actor_rig(2).expect("the remote player has a rig");
        (
            without_attachment_instant(authority.actor(2)),
            without_attachment_instant(authority.actor(4)),
            rig.java,
            rig.previous_body_yaw,
            rig.body_yaw,
        )
    };
    let (interleaved, predicted) = (advance(false), advance(true));
    assert_eq!(predicted.0, interleaved.0, "remote player state");
    assert_eq!(predicted.1, interleaved.1, "dragon state and history");
    assert_eq!(predicted.2, interleaved.2, "walk cycle and Java body yaw");
    assert_eq!(
        (predicted.3, predicted.4),
        (interleaved.3, interleaved.4),
        "body yaw"
    );
}

/// An actor's state without its wall-clock terrain attachment, which separately built worlds
/// record at different instants before either advances.
fn without_attachment_instant(actor: Option<&impl std::fmt::Debug>) -> String {
    let state = format!("{:?}", actor.expect("spawned actor"));
    let Some(start) = state.find("attached_at: ") else {
        return state;
    };
    let end = start + state[start..].find('}').expect("instant fields close") + 1;
    format!("{}{}", &state[..start], &state[end..])
}

/// The first frame of a new actor session keeps its fractional tick for the next frame.
#[test]
fn new_session_keeps_the_first_frame_tick_remainder() {
    let mut app = App::new();
    let (mut schedule, mut world) = production(&mut app);
    for millis in [70, 10] {
        world
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(millis));
        schedule.run(&mut world);
    }
    let partial = world.resource::<ActorFramePartialTick>().0;
    assert!((partial - 0.6).abs() < 1e-4, "partial tick {partial}");
}

/// Steady frames predict picks in the retained buffer without allocating.
#[test]
fn pick_prediction_reuses_its_buffer_across_frames() {
    let mut world = fixture();
    let mut client = world.resource_mut::<ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let mut hitbox = world::NbtCompound::default();
    for axis in ["X", "Y", "Z"] {
        hitbox.insert(format!("Max{axis}"), world::NbtValue::Float(1.0));
        hitbox.insert(format!("Pivot{axis}"), world::NbtValue::Float(2.0));
    }
    let mut root = world::NbtCompound::default();
    root.insert(
        "Hitboxes",
        world::NbtValue::List(vec![world::NbtValue::Compound(hitbox)]),
    );
    stream
        .submit(
            3,
            WorldEvent::Actor(ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 2,
                metadata: Arc::from([protocol::ActorMetadata {
                    key: ActorDataKey::HitBox.wire_value(),
                    value: protocol::ActorMetadataValue::Compound(
                        root.encode_root().unwrap().into(),
                    ),
                }]),
                properties: Arc::from([]),
                tick: 0,
            })),
        )
        .unwrap();
    stream.poll([0.0, 64.0, 0.0], 0);
    stream.predict_remote_actor_motion(1);
    let before = test_alloc::thread_allocations();
    for ticks in [1, 0, 2, 0, 1] {
        stream.predict_remote_actor_motion(ticks);
        let authority = stream.authority();
        for actor in authority.remote_actors() {
            std::hint::black_box(authority.pick_hit_boxes(actor).last());
        }
    }
    assert_eq!(test_alloc::thread_allocations() - before, 0);
}

/// Remote picks and publication share elapsed frame time even when local ticks have another phase.
#[test]
fn remote_actor_motion_keeps_its_clock_when_local_ticks_differ() {
    let mut app = App::new();
    app.add_systems(
        Update,
        capture_predicted_box.in_set(client_state::ClientFrameSet::NetworkSend),
    );
    let (mut schedule, mut world) = production(&mut app);
    world.init_resource::<PredictedBox>();
    let anchor = world
        .resource::<ClientWorld>()
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .resolved_server_position()
        .position;
    let mut physics = client_interaction::movement::LocalPhysicsController::default();
    physics.reanchor_network_position(anchor, 0, true);
    world.insert_resource(physics);
    {
        let mut client = world.resource_mut::<ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        stream
            .submit(3, move_actor(2, [2.0, 64.0, 9.0], 0.0))
            .unwrap();
        stream.poll([0.0, 64.0, 0.0], 0);
    }
    for (millis, local_ticks, expected_z, expected_alpha) in [
        (25, 0, 0.0, 0.5),
        (75, 1, 6.0, 0.0),
        (25, 0, 6.0, 0.5),
        (25, 0, 9.0, 0.0),
    ] {
        {
            let mut effects =
                world.resource_mut::<client_interaction::movement::LocalMovementEffectTimeline>();
            effects.begin_frame();
            for _ in 0..local_ticks {
                effects.commit_successful_tick();
            }
        }
        world
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(millis));
        schedule.run(&mut world);
        let client = world.resource::<ClientWorld>();
        let actor = client
            .stream
            .as_ref()
            .unwrap()
            .authority()
            .actor(2)
            .unwrap();
        assert!(
            (actor.position[2] - expected_z).abs() < 1e-4,
            "remote position {:?} after {millis} ms and {local_ticks} local ticks",
            actor.position
        );
        assert_eq!(world.resource::<PredictedBox>().0, actor.hit_boxes().next());
        assert_eq!(world.resource::<ActorFramePartialTick>().0, expected_alpha);
    }
}

/// Without local prediction, actor time still uses the client's admitted frame interval.
#[test]
fn remote_actor_motion_clamps_unowned_frame_time() {
    let mut app = App::new();
    let (mut schedule, mut world) = production(&mut app);
    world.insert_resource(client_interaction::movement::LocalPhysicsController::default());
    {
        let mut client = world.resource_mut::<ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        stream
            .submit(3, move_actor(2, [2.0, 64.0, 9.0], 0.0))
            .unwrap();
        stream.poll([0.0, 64.0, 0.0], 0);
    }
    world
        .resource_mut::<Time<Real>>()
        .advance_by(simulation_clock::MAX_FRAME_ELAPSED + simulation_clock::TICK_DURATION);
    schedule.run(&mut world);
    let client = world.resource::<ClientWorld>();
    let actor = client
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor(2)
        .unwrap();
    assert!(
        (actor.position[2] - 6.0).abs() < 1e-4,
        "the admitted interval must advance two interpolation steps: {:?}",
        actor.position
    );
}
