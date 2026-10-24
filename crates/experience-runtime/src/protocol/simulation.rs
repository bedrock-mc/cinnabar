//! Simulation time and callback identities shared with the adapter.
use super::WorldHandle;
use serde::{Deserialize, Serialize};

/// A fixed simulation step; time advances only while its instance is bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimulationStep {
    pub tick: u64,
    pub duration_nanos: u64,
}

/// One callback identity, scoped to its installing mod and instance opening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleHandle {
    pub world: WorldHandle,
    pub sequence: u64,
    pub slot: u32,
}

/// Active handles are snapshotted so cancellation can reject stale identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimulationContext {
    pub step: SimulationStep,
    pub sequence: u64,
    pub schedules: Vec<ScheduleHandle>,
}

/// Golden records cover both call variants and both staged operations across Rust and Go.
pub(super) fn fixtures() -> Vec<(&'static str, String)> {
    use super::{Call, Info, Op, Outcome, Request, Response};
    let world = WorldHandle {
        id: "probe:clock".into(),
        generation: 9,
    };
    let handle = ScheduleHandle {
        world: world.clone(),
        sequence: 1,
        slot: 0,
    };
    let step = SimulationStep {
        tick: 3,
        duration_nanos: crate::limits::SIMULATION_STEP_NANOS,
    };
    let mut out = Vec::new();
    for (name, call) in [
        ("request_callback_tick", Call::Tick { step }),
        (
            "request_callback_timer",
            Call::Timer {
                handle: handle.clone(),
                step,
            },
        ),
    ] {
        let request = Request::Callback {
            seq: 2,
            info: Info {
                context: Some(super::WorldContext {
                    entities: Vec::new(),
                    current: world.clone(),
                    player: None,
                    instances: vec![world.clone()],
                    stores: Vec::new(),
                }),
                simulation: Some(SimulationContext {
                    step,
                    sequence: 2,
                    schedules: vec![handle.clone()],
                }),
                world_id: "world".into(),
                dimension_id: "overworld".into(),
                tick: 3,
                event_sequence: 2,
            },
            actor: None,
            world_min_y: -64,
            world_max_y: 319,
            data_budget: 1024,
            snapshot: Vec::new(),
            network: None,
            inventory: None,
            call,
        };
        out.push((name, serde_json::to_string_pretty(&request).unwrap() + "\n"));
    }
    let response = Response::Result {
        seq: 1,
        outcome: Outcome::Committed {
            ops: vec![
                Op::Schedule {
                    handle: handle.clone(),
                    after: 2,
                    period: 3,
                },
                Op::CancelSchedule { handle },
            ],
        },
    };
    out.push((
        "response_result_simulation",
        serde_json::to_string_pretty(&response).unwrap() + "\n",
    ));
    out
}
