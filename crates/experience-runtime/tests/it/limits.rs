//! Containment: each limit stops a guest that runs away, in a callback or in `register`, and what
//! stays within the limits still commits.

use crate::common;

use std::time::{Duration, Instant};

use common::{interact, looping_register_dir, outcome, probe, tell};
use experience_runtime::limits::REGISTER_DEADLINE;
use experience_runtime::load::{engine, load};
use experience_runtime::protocol::{FailKind, Outcome};
use wasmtime::Trap;

#[test]
fn endless_loop_fails_with_fuel() {
    let request = interact(2);
    // Load first, so that only the callback is timed.
    probe();
    let start = Instant::now();
    let outcome = outcome(&request);
    let elapsed = start.elapsed();
    assert!(
        matches!(
            outcome,
            Outcome::Failed {
                kind: FailKind::Fuel,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert!(elapsed < Duration::from_secs(1), "the loop ran {elapsed:?}");
}

#[test]
fn memory_growth_fails_with_limit() {
    let outcome = outcome(&interact(3));
    assert!(
        matches!(
            outcome,
            Outcome::Failed {
                kind: FailKind::Limit,
                ..
            }
        ),
        "{outcome:?}"
    );
}

#[test]
fn host_call_flood_fails_with_limit() {
    let outcome = outcome(&interact(4));
    assert!(
        matches!(
            outcome,
            Outcome::Failed {
                kind: FailKind::Limit,
                ..
            }
        ),
        "{outcome:?}"
    );
}

#[test]
fn op_flood_fails_with_limit() {
    let outcome = outcome(&interact(16));
    assert!(
        matches!(
            outcome,
            Outcome::Failed {
                kind: FailKind::Limit,
                ..
            }
        ),
        "{outcome:?}"
    );
}

/// The probe logs 1000 lines; the ones past the log cap are dropped, and the callback goes on.
#[test]
fn log_flood_still_commits() {
    assert_eq!(
        outcome(&interact(11)),
        Outcome::Committed {
            ops: vec![tell("logged")]
        }
    );
}

/// The probe counts its calls in a static, which a fresh instance starts at zero.
#[test]
fn fresh_instance_per_callback() {
    let first = Outcome::Committed {
        ops: vec![tell("mem 1")],
    };
    assert_eq!(outcome(&interact(12)), first);
    assert_eq!(outcome(&interact(12)), first);
}

/// `register` runs under its own fuel and deadline, so a guest that never returns from it fails
/// to load in time instead of hanging the helper.
#[test]
fn register_loop_is_refused() {
    let dir = looping_register_dir();
    let (engine, _ticker) = engine().unwrap();
    let start = Instant::now();
    let Err(error) = load(&engine, dir.path()) else {
        panic!("{} loaded", dir.path().display());
    };
    let elapsed = start.elapsed();
    let shown = format!("{error:#}");
    let path = dir.path().display().to_string();
    assert!(
        shown.contains(&path),
        "the error does not name {path}: {shown}"
    );
    assert!(
        matches!(
            error.downcast_ref::<Trap>(),
            Some(Trap::OutOfFuel | Trap::Interrupt)
        ),
        "{shown}"
    );
    // Compiling the module takes a fraction of the slack.
    let bound = REGISTER_DEADLINE + Duration::from_secs(1);
    assert!(elapsed < bound, "loading took {elapsed:?}");
}
