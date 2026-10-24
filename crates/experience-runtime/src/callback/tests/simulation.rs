//! Simulation staging rejects stale handles and shares callback limits.
use super::*;
use crate::protocol::{
    ScheduleHandle, SimulationContext, SimulationStep, WorldContext, WorldHandle,
};

/// A callback with an instance clock and no previously scheduled work.
fn callback() -> CallbackRes {
    let mut res = Fixture::new().res();
    res.context = Some(WorldContext {
        current: WorldHandle {
            id: "probe:clock".into(),
            generation: 3,
        },
        player: None,
        instances: Vec::new(),
        stores: Vec::new(),
    });
    res.simulation = Some(SimulationContext {
        step: SimulationStep {
            tick: 9,
            duration_nanos: crate::limits::SIMULATION_STEP_NANOS,
        },
        sequence: 12,
        schedules: Vec::new(),
    });
    res
}

#[test]
fn scheduling_and_cancellation_use_typed_opening_handles() {
    let mut res = callback();
    let handle = res.schedule(2, 3).unwrap().unwrap();
    let wire: ScheduleHandle = handle.clone().into();
    assert_eq!(wire.world.generation, 3);
    assert_eq!(wire.sequence, 12);
    assert_eq!(res.simulation().unwrap().unwrap().tick, 9);
    let mut stale = handle.clone();
    stale.world.generation -= 1;
    assert!(matches!(
        res.cancel_schedule(stale).unwrap(),
        Err(WorldError::Unavailable)
    ));
    res.cancel_schedule(handle.clone()).unwrap().unwrap();
    assert!(matches!(
        res.cancel_schedule(handle).unwrap(),
        Err(WorldError::Unavailable)
    ));
    assert!(matches!(
        res.ops.as_slice(),
        [
            Op::Schedule {
                after: 2,
                period: 3,
                ..
            },
            Op::CancelSchedule { .. }
        ]
    ));
}

#[test]
fn schedules_share_callback_budget_and_refuse_invalid_delays() {
    let mut res = callback();
    for (after, period) in [(0, 0), (u32::MAX, 0), (1, u32::MAX)] {
        assert!(matches!(
            res.schedule(after, period).unwrap(),
            Err(WorldError::OutOfBounds)
        ));
    }
    res.host_calls = MAX_HOST_CALLS;
    assert!(res.schedule(1, 0).unwrap_err().is::<LimitExceeded>());
    let mut res = callback();
    res.ops = vec![
        Op::Tell {
            player: ACTOR.into(),
            text: String::new()
        };
        MAX_STAGED_OPS
    ];
    assert!(res.schedule(1, 0).unwrap_err().is::<LimitExceeded>());
}
