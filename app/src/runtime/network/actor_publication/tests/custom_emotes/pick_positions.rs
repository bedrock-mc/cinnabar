//! Interaction picks remote actors at this frame's tick positions.
use super::*;

#[derive(Resource, Default)]
struct Picked(Option<u64>);

/// Casts a melee pick along +Z from between the actor's old and current-tick positions.
fn pick_ahead(world: Res<ClientWorld>, mut picked: ResMut<Picked>) {
    let stream = world.stream.as_ref().unwrap();
    picked.0 = gameplay::melee::pick_actor(
        stream.authority().remote_actors(),
        None,
        [2.0, 65.0, 1.5],
        [0.0, 0.0, 1.0],
        3.0,
    )
    .map(|hit| hit.runtime_id);
}

/// A melee press must hit an actor where its due interpolation tick puts it this frame.
#[test]
fn production_melee_picks_remote_actors_at_current_frame_positions() {
    let mut app = App::new();
    crate::app::configure_client_frame_schedule(&mut app);
    crate::app::configure_actor_render_systems(&mut app);
    app.add_systems(
        Update,
        pick_ahead.in_set(crate::app::ClientFrameSet::NetworkSend),
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
    world.init_resource::<render::ActorRenderFrame>();
    world.init_resource::<render::ActorRuntimeWitness>();
    world.init_resource::<Picked>();
    {
        let mut client = world.resource_mut::<ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        stream
            .submit(
                3,
                WorldEvent::Actor(ActorEvent::Move(protocol::ActorMoveEvent {
                    dimension: 0,
                    runtime_id: 2,
                    // Moves interpolate over at least three ticks: one tick covers a third.
                    position: [Some(2.0), Some(64.0), Some(9.0)],
                    position_origin: protocol::ActorPositionOrigin::Feet,
                    pitch: None,
                    yaw: None,
                    head_yaw: None,
                    on_ground: None,
                    teleported: false,
                    player_mode: None,
                    source_tick: None,
                    interpolation: protocol::ActorInterpolation {
                        ticks: 1,
                        force_completion: false,
                    },
                })),
            )
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
