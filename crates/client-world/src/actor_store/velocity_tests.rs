use super::*;

fn store() -> ActorStore {
    let ActorEvent::Spawn(mut event) = tests::spawn(7, 70) else {
        unreachable!();
    };
    event.position = [0.0; 3];
    event.velocity = [0.25, -0.5, 0.0];
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, ActorEvent::Spawn(event));
    store
}

fn movement(x: Option<f32>, tick: Option<u64>, teleported: bool) -> ActorEvent {
    ActorEvent::Move(ActorMoveEvent {
        dimension: 0,
        runtime_id: 7,
        position: [x, None, None],
        position_origin: ActorPositionOrigin::Feet,
        pitch: None,
        yaw: Some(10.0),
        head_yaw: None,
        on_ground: Some(false),
        teleported,
        player_mode: None,
        source_tick: tick,
        interpolation: Default::default(),
    })
}

#[test]
fn native_animation_velocity_keeps_motion_separate_from_derived_query_speed() {
    let mut store = store();
    assert_eq!(store.get(7).unwrap().native_velocity(), [0.25, -0.5, 0.0]);
    store.apply(1, 2, movement(Some(0.25), None, false));
    let actor = store.get(7).unwrap();
    assert_eq!(actor.velocity, [5.0, 0.0, 0.0]);
    assert_eq!(actor.native_velocity(), [0.25, -0.5, 0.0]);
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(7).unwrap().native_velocity(), [0.0; 3]);
    assert_eq!(store.get(7).unwrap().velocity, [5.0, 0.0, 0.0]);

    store.apply_motion(
        3,
        protocol::ActorMotionEvent {
            actor_runtime_id: 7,
            motion: [0.3, -0.4, 0.0],
            tick: 1,
        },
    );
    assert_eq!(store.get(7).unwrap().native_velocity(), [0.3, -0.4, 0.0]);
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(7).unwrap().native_velocity(), [0.0; 3]);
}

#[test]
fn final_interpolation_tick_clears_native_velocity_until_new_motion_arrives() {
    let mut store = store();
    store.apply(1, 2, movement(Some(1.0), Some(10), false));
    store.advance_interpolation_ticks(2);
    assert_eq!(store.get(7).unwrap().interpolation_ticks_remaining, 1);
    let motion = protocol::ActorMotionEvent {
        actor_runtime_id: 7,
        motion: [0.3, -0.4, 0.0],
        tick: 1,
    };
    store.apply_motion(3, motion);
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(7).unwrap().interpolation_ticks_remaining, 0);
    assert_eq!(store.get(7).unwrap().native_velocity(), [0.0; 3]);
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(7).unwrap().native_velocity(), [0.0; 3]);
    store.apply_motion(4, motion);
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(7).unwrap().native_velocity(), [0.3, -0.4, 0.0]);
}

#[test]
fn direct_position_teleport_preserves_native_motion_but_zeroes_derived_speed() {
    let mut store = store();
    store.apply(1, 2, movement(Some(100.0), None, true));
    assert_eq!(store.get(7).unwrap().velocity, [0.0; 3]);
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(7).unwrap().native_velocity(), [0.25, -0.5, 0.0]);
}

#[test]
fn observed_motion_preserves_native_spawn_pose_and_damage_timers() {
    let ActorEvent::Spawn(mut spawn) = tests::spawn(7, 70) else {
        unreachable!();
    };
    spawn.velocity = [0.25, -0.5, 0.0];
    let mut observed = ActorSnapshot::from_observation(spawn.clone(), 1);
    let mut native = ActorStore::new(1, 0);
    native.apply(1, 1, ActorEvent::Spawn(spawn));

    assert_eq!(
        observed.native_velocity(),
        native.get(7).unwrap().native_velocity()
    );
    assert_eq!(
        observed.interpolated_position(0.5),
        Some(native.get(7).unwrap().position)
    );
    assert_eq!(observed.status, native.get(7).unwrap().status);
    observed.status.die();
    native.apply(
        1,
        2,
        ActorEvent::Status(protocol::ActorStatusEvent {
            runtime_id: 7,
            kind: protocol::ActorStatusKind::Death,
            data: 0,
        }),
    );
    let native = native.actors.get_mut(&7).unwrap();
    assert!(observed.hurt_overlay_active());
    assert!(observed.status.dead);
    assert_eq!(observed.status.death_ticks(), native.status.death_ticks());

    observed.status.tick();
    native.status.tick();
    let timers = observed.status;
    observed.observe_velocity([0.3, -0.4, 0.0]);

    assert_eq!(observed.native_velocity(), [0.3, -0.4, 0.0]);
    assert_eq!(observed.velocity, observed.native_velocity());
    assert_eq!(observed.interpolated_position(0.5), Some(native.position));
    assert_eq!(observed.status.hurt_time, timers.hurt_time);
    assert_eq!(observed.status.death_ticks(), native.status.death_ticks());
    assert_eq!(observed.status.age_ticks, native.status.age_ticks);
}
