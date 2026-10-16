use super::*;
use crate::block_use::PlacementTarget;

/// The first eligible repeat keeps its forward support rather than waiting another tick.
#[test]
fn a_fractional_deadline_preserves_the_forward_placement_opportunity() {
    let mut runtime = BlockUseRuntime::default();
    let start = RepeatClock {
        now: Duration::from_millis(1_000) + Duration::from_micros(123),
        ..clock(0, 4.4)
    };
    runtime.intention.record(
        false,
        [0, 63, 0],
        LocalUse::Place,
        true,
        false,
        [0.5, 64.0, 0.5],
    );
    runtime.record(
        ItemUseTrigger::PlayerInput,
        start.now,
        1,
        LocalUse::Place,
        start,
    );
    let deadline = RepeatClock {
        now: start.now + Duration::from_millis(300),
        ..start
    };
    assert_eq!(runtime.due(true, 7, deadline), None);
    let next = RepeatClock {
        now: deadline.now + Duration::from_nanos(1),
        ..deadline
    };
    let (trigger, due) = runtime.due(true, 7, next).expect("the deadline has passed");
    assert_eq!(
        (trigger, due),
        (ItemUseTrigger::SimulationTick, deadline.now)
    );
    assert_eq!(
        runtime.intention.target(
            Some(PlacementTarget {
                position: [0, 63, 0],
                face: 4
            }),
            [1.1, 65.62, 0.5],
            [0.0, 63.5, 0.5],
            [0.22, 0.0, 0.0],
            false,
        ),
        Some(PlacementTarget {
            position: [0, 63, 0],
            face: 5
        })
    );
    runtime.record(trigger, due, 7, LocalUse::Nothing, next);
    assert_eq!(runtime.due(true, 7, next), None);
    assert_eq!(runtime.due(true, 8, next), Some((trigger, due)));
}

/// Successful moving repeats retain fractional deadlines through bounded catch-up.
#[test]
fn moving_repeats_preserve_fractional_schedule_and_catchup_limit() {
    let mut runtime = BlockUseRuntime::default();
    let start = RepeatClock {
        now: Duration::from_millis(1_000) + Duration::from_micros(123),
        ..clock(0, 10.0)
    };
    runtime.record(
        ItemUseTrigger::PlayerInput,
        start.now,
        1,
        LocalUse::Place,
        start,
    );
    let due = start.now + Duration::from_millis(100);
    let next = RepeatClock {
        now: due + Duration::from_nanos(1),
        ..start
    };
    assert_eq!(
        runtime.due(true, 3, next),
        Some((ItemUseTrigger::SimulationTick, due))
    );
    runtime.record(
        ItemUseTrigger::SimulationTick,
        due,
        3,
        LocalUse::Place,
        next,
    );
    let deadline = RepeatClock {
        now: due + Duration::from_millis(100),
        ..next
    };
    assert_eq!(runtime.due(true, 5, deadline), None);
    assert!(
        runtime
            .due(
                true,
                5,
                RepeatClock {
                    now: deadline.now + Duration::from_nanos(1),
                    ..deadline
                }
            )
            .is_some()
    );
    let late = RepeatClock {
        now: start.now + Duration::from_secs(1),
        ..start
    };
    runtime.record(
        ItemUseTrigger::SimulationTick,
        deadline.now,
        20,
        LocalUse::Place,
        late,
    );
    assert_eq!(
        runtime.last_use,
        Some(late.now - super::super::REPEAT_MAX_LAG)
    );
}
