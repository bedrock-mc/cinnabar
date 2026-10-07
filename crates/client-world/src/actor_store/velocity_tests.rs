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
