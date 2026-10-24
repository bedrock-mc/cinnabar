//! Actual guest callbacks exercise clock reads, scheduling and cancellation across WIT.
use crate::common;
use experience_runtime::{
    callback::run,
    load::{engine, load},
    protocol::*,
};

/// An immutable fixed-step request without a player or block anchor.
fn request(call: Call, step: SimulationStep, handles: Vec<ScheduleHandle>) -> Request {
    let world = WorldHandle {
        id: "probe:clock".into(),
        generation: 7,
    };
    Request::Callback {
        seq: 2,
        info: Info {
            context: Some(WorldContext {
                current: world.clone(),
                player: None,
                instances: vec![world],
                stores: Vec::new(),
            }),
            simulation: Some(SimulationContext {
                step,
                sequence: 2,
                schedules: handles,
            }),
            world_id: "world".into(),
            dimension_id: "overworld".into(),
            tick: 0,
            event_sequence: 2,
        },
        actor: None,
        world_min_y: -64,
        world_max_y: 319,
        data_budget: 4096,
        snapshot: Vec::new(),
        network: None,
        inventory: None,
        call,
    }
}

#[test]
fn guest_simulation_callbacks_stage_and_cancel_timers() {
    let dir = common::probe_dir();
    let (engine, _ticker) = engine().unwrap();
    let loaded = load(&engine, dir.path()).unwrap();
    let step = SimulationStep {
        tick: 1,
        duration_nanos: experience_runtime::limits::SIMULATION_STEP_NANOS,
    };
    let Outcome::Committed { ops } = run(
        &engine,
        &loaded,
        &request(Call::Tick { step }, step, Vec::new()),
    ) else {
        panic!("tick failed")
    };
    let [
        Op::Schedule {
            handle,
            after: 2,
            period: 3,
        },
    ] = ops.as_slice()
    else {
        panic!("timer missing: {ops:?}")
    };
    let step = SimulationStep { tick: 3, ..step };
    let outcome = run(
        &engine,
        &loaded,
        &request(
            Call::Timer {
                handle: handle.clone(),
                step,
            },
            step,
            vec![handle.clone()],
        ),
    );
    assert_eq!(
        outcome,
        Outcome::Committed {
            ops: vec![Op::CancelSchedule {
                handle: handle.clone()
            }]
        }
    );
}
