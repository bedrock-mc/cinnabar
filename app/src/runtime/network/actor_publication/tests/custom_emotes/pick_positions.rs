//! Interaction picks remote actors at this frame's tick positions without disturbing their
//! per-tick animation or the actor clock.
use super::*;

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
        player_mode: None,
        source_tick: None,
        interpolation: protocol::ActorInterpolation {
            ticks: 3,
            force_completion: false,
        },
    }))
}

/// The production frame schedule over the actor fixture, with the resources its systems read.
fn production(app: &mut App) -> (bevy::ecs::schedule::Schedule, World) {
    crate::app::configure_client_frame_schedule(app);
    crate::app::configure_actor_render_systems(app);
    let schedule = app
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
    world.init_resource::<render::ActorRenderFrame>();
    world.init_resource::<render::ActorRuntimeWitness>();
    (schedule, world)
}

/// A melee press must hit an actor where its due interpolation tick puts it this frame.
#[test]
fn production_melee_picks_remote_actors_at_current_frame_positions() {
    let mut app = App::new();
    app.add_systems(
        Update,
        pick_ahead.in_set(crate::app::ClientFrameSet::NetworkSend),
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
        capture_predicted_box.in_set(crate::app::ClientFrameSet::NetworkSend),
    );
    let (mut schedule, mut world) = production(&mut app);
    world.init_resource::<PredictedBox>();
    let cap = world::MAX_TICKS_PER_FRAME;
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
                        key: client_world::HITBOX_METADATA_KEY,
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
    let tick = client_world::ACTOR_TICK_DURATION;
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
    assert_eq!(world.resource::<ActorFramePartialTick>().0, 0.5);
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
            stream.predict_remote_actor_motion(3);
        }
        stream.advance_actor_interpolation_frame(3);
        let authority = stream.authority();
        let rig = authority.actor_rig(2).expect("the remote player has a rig");
        (
            authority.actor(2).cloned(),
            authority.actor(4).cloned(),
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
                    key: client_world::HITBOX_METADATA_KEY,
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
    let before = crate::tests::alloc_count::thread_allocations();
    for ticks in [1, 0, 2, 0, 1] {
        stream.predict_remote_actor_motion(ticks);
        let authority = stream.authority();
        for actor in authority.remote_actors() {
            std::hint::black_box(authority.pick_hit_boxes(actor).last());
        }
    }
    assert_eq!(crate::tests::alloc_count::thread_allocations() - before, 0);
}
