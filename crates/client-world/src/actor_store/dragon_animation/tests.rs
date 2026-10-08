use super::*;

#[test]
fn flap_rate_uses_fixed_tick_displacement_and_sitting_state() {
    let mut state = State::default();
    state.advance([0.0, 64.0, 0.0], [0.0; 3], 0.0, false, false);
    assert_eq!(state.flap_phase, 0.2);
    state.advance([0.0, 65.0, 1.0], [0.0, 1.0, 1.0], 0.0, false, false);
    assert!((state.flap_phase - (0.2 + 0.4 / 11.0)).abs() < 1e-6);
    let flying = state.flap_phase;
    state.advance([0.0, 65.0, 1.0], [0.0, 10.0, 10.0], 0.0, true, false);
    assert_eq!(state.flap_phase, flying + 0.1);
    state.advance([0.0, 65.0, 1.0], [f32::NAN; 3], 0.0, false, false);
    assert_eq!(state.flap_phase, flying + 0.1);
    state.advance([0.0, 65.0, 1.0], [0.0; 3], 0.0, false, true);
    assert_eq!(state.flap_phase, 0.0);
}

#[test]
fn history_starts_filled_and_wraps_yaw_before_sampling_each_fixed_tick() {
    let mut state = State::default();
    state.advance([0.0, 64.0, 0.0], [0.0; 3], 179.0, false, false);
    assert_eq!(state.historical_frame(23, false), [179.0, 64.0]);
    state.advance([0.0, 65.0, 0.0], [0.0, 1.0, 0.0], 181.0, false, false);
    assert_eq!(state.historical_frame(0, false), [-179.0, 65.0]);
    assert_eq!(state.historical_frame(1, false), [179.0, 64.0]);
    assert_eq!(state.historical_frame(0, true), [179.0, 64.0]);
    for height in 66..140 {
        state.advance(
            [0.0, height as f32, 0.0],
            [0.0, 1.0, 0.0],
            181.0,
            false,
            false,
        );
    }
    assert_eq!(state.historical_frame(23, false), [-179.0, 116.0]);
    state.advance([0.0, 200.0, 0.0], [0.0; 3], 0.0, false, true);
    assert_eq!(state.historical_frame(0, false), [-179.0, 139.0]);
}

#[test]
fn actor_ticks_advance_flap_without_an_admitted_rig_and_dimension_reset_drops_history() {
    let protocol::ActorEvent::Spawn(mut dragon) = super::super::tests::spawn(1, 1) else {
        unreachable!()
    };
    dragon.kind = ActorKind::Entity {
        identifier: "minecraft:ender_dragon".into(),
    };
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, protocol::ActorEvent::Spawn(dragon));
    store.advance_interpolation_ticks(3);
    let dragon = store.actors.get(&1).unwrap();
    let state = dragon.dragon_animation.as_ref().unwrap();
    assert!((state.flap_phase - 0.6).abs() < 1e-6);
    assert_eq!(state.historical_frame(0, false)[1], dragon.position[1]);
    store.reset_dimension(1, 2, 2);
    assert!(store.actors.is_empty());
}
