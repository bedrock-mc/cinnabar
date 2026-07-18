use sim::{
    Aabb, CollisionQuery, CollisionWorld, MovementInput, PlayerState, PredictionError,
    PredictionHistory, Simulator, Vec3, WorldQueryError,
};

struct Floor;

impl CollisionWorld for Floor {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-64.0, 0.0, -64.0), Vec3::new(64.0, 1.0, 64.0));
        Ok(CollisionQuery::synthetic(
            floor
                .intersects(query)
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }
}

fn initial_state() -> PlayerState {
    let mut state = PlayerState::new(Vec3::new(0.0, 1.0, 0.0));
    state.on_ground = true;
    state
}

fn forward() -> MovementInput {
    MovementInput {
        forward: 1.0,
        ..MovementInput::default()
    }
}

#[test]
fn bounded_history_evicts_whole_oldest_frames_in_tick_order() {
    let mut state = initial_state();
    let mut history = PredictionHistory::new(2).unwrap();
    for _ in 0..3 {
        history
            .predict(&mut state, forward(), &Simulator::default(), &Floor)
            .unwrap();
    }

    assert_eq!(history.len(), 2);
    assert_eq!(history.oldest_tick(), Some(2));
    assert_eq!(history.newest_tick(), Some(3));
}

#[test]
fn correction_replaces_post_tick_state_and_replays_every_later_input() {
    let simulator = Simulator::default();
    let mut state = initial_state();
    let mut history = PredictionHistory::new(8).unwrap();
    for _ in 0..3 {
        history
            .predict(&mut state, forward(), &simulator, &Floor)
            .unwrap();
    }

    let mut corrected = history.state_at(1).unwrap().clone();
    corrected.position.x = 0.25;
    let replay = history
        .rewind_and_replay(&mut state, corrected.clone(), &simulator, &Floor)
        .unwrap();

    let mut independently_replayed = corrected;
    simulator
        .tick(&mut independently_replayed, forward(), &Floor)
        .unwrap();
    simulator
        .tick(&mut independently_replayed, forward(), &Floor)
        .unwrap();
    assert_eq!(state, independently_replayed);
    assert_eq!(replay.corrected_tick, 1);
    assert_eq!(replay.replayed_ticks, 2);
    assert_eq!(history.state_at(3), Some(&state));
}

#[test]
fn traced_replay_returns_each_fresh_tick_result_in_order() {
    let simulator = Simulator::default();
    let mut state = initial_state();
    let mut history = PredictionHistory::new(8).unwrap();
    for _ in 0..3 {
        history
            .predict(&mut state, forward(), &simulator, &Floor)
            .unwrap();
    }

    let mut corrected = history.state_at(1).unwrap().clone();
    corrected.position.x = 0.25;
    let (replay, ticks) = history
        .rewind_and_replay_traced(&mut state, corrected, &simulator, &Floor)
        .unwrap();

    assert_eq!(replay.corrected_tick, 1);
    assert_eq!(replay.replayed_ticks, 2);
    assert_eq!(
        ticks.iter().map(|tick| tick.tick).collect::<Vec<_>>(),
        [2, 3]
    );
    assert!(
        ticks
            .iter()
            .all(|tick| tick.world_identity == ticks[0].world_identity)
    );
    assert_eq!(ticks.last().unwrap().position, state.position);
}

#[test]
fn correction_older_than_retained_history_is_rejected_transactionally() {
    let simulator = Simulator::default();
    let mut state = initial_state();
    let mut history = PredictionHistory::new(2).unwrap();
    for _ in 0..3 {
        history
            .predict(&mut state, forward(), &simulator, &Floor)
            .unwrap();
    }
    let before_state = state.clone();
    let before_history = history.clone();
    let mut stale = initial_state();
    stale.tick = 1;

    assert_eq!(
        history.rewind_and_replay(&mut state, stale, &simulator, &Floor),
        Err(PredictionError::CorrectionNotRetained {
            tick: 1,
            oldest: Some(2),
            newest: Some(3),
        })
    );
    assert_eq!(state, before_state);
    assert_eq!(history, before_history);
}

#[test]
fn zero_capacity_is_rejected_instead_of_silently_disabling_rewind() {
    assert_eq!(
        PredictionHistory::new(0),
        Err(PredictionError::ZeroCapacity)
    );
}

#[test]
fn newest_tick_non_finite_correction_is_rejected_transactionally() {
    let simulator = Simulator::default();
    let mut state = initial_state();
    let mut history = PredictionHistory::new(2).unwrap();
    history
        .predict(&mut state, forward(), &simulator, &Floor)
        .unwrap();
    let before_state = state.clone();
    let before_history = history.clone();
    let mut corrected = state.clone();
    corrected.position.x = f64::NAN;

    assert!(matches!(
        history.rewind_and_replay(&mut state, corrected, &simulator, &Floor),
        Err(PredictionError::Simulation(
            sim::SimulationError::NonFiniteState { .. }
        ))
    ));
    assert_eq!(state, before_state);
    assert_eq!(history, before_history);
}

#[test]
fn newest_tick_oversized_sweep_correction_is_rejected_transactionally() {
    let simulator = Simulator::default();
    let mut state = initial_state();
    let mut history = PredictionHistory::new(2).unwrap();
    history
        .predict(&mut state, forward(), &simulator, &Floor)
        .unwrap();
    let before_state = state.clone();
    let before_history = history.clone();
    let mut corrected = state.clone();
    corrected.velocity.x = 1_000_000.0;

    assert_eq!(
        history.rewind_and_replay(&mut state, corrected, &simulator, &Floor),
        Err(PredictionError::Simulation(sim::SimulationError::World(
            WorldQueryError::QueryExtentExceeded,
        )))
    );
    assert_eq!(state, before_state);
    assert_eq!(history, before_history);
}
