//! Crouching scales the move vector sprint intent reads, so it blocks and ends a sprint.

use super::{LowCeiling, ModeIntent, MovementInput, VersionedFloor, settled_controller, step};

/// Forward movement with the sprint request held.
fn sprint_forward(sneaking: bool, swift_sneak: u8) -> MovementInput {
    MovementInput {
        forward: 1.0,
        sprinting: true,
        sneaking,
        swift_sneak,
        ..MovementInput::default()
    }
}

#[test]
fn a_crouch_request_refuses_a_sprint_start() {
    let mut physics = settled_controller();
    for tick in 0..5 {
        let sample = step(
            &mut physics,
            sprint_forward(true, 0),
            ModeIntent::default(),
            &VersionedFloor(1),
        );
        assert!(
            !sample.processed.sprinting,
            "tick {tick} sprinted while crouching"
        );
    }
}

#[test]
fn crouching_ends_a_running_sprint() {
    let mut physics = settled_controller();
    let running = step(
        &mut physics,
        sprint_forward(false, 0),
        ModeIntent::default(),
        &VersionedFloor(1),
    );
    assert!(running.processed.sprinting);
    let crouched = step(
        &mut physics,
        sprint_forward(true, 0),
        ModeIntent::default(),
        &VersionedFloor(1),
    );
    assert!(!crouched.processed.sprinting);
}

/// Swift Sneak III keeps a crouched vector above the sprint threshold once the pose is
/// active; lower levels cannot.
#[test]
fn swift_sneak_three_sprints_once_the_crouch_pose_is_active() {
    for (level, sprints) in [(2, false), (3, true)] {
        let mut physics = settled_controller();
        let intent = ModeIntent {
            swift_sneak: level,
            ..ModeIntent::default()
        };
        step(
            &mut physics,
            MovementInput {
                sneaking: true,
                swift_sneak: level,
                ..MovementInput::default()
            },
            intent,
            &VersionedFloor(1),
        );
        let sample = step(
            &mut physics,
            sprint_forward(true, level),
            intent,
            &VersionedFloor(1),
        );
        assert_eq!(sample.processed.sprinting, sprints, "Swift Sneak {level}");
    }
}

/// A ceiling that first forces the crouch leaves that tick's sprint intent to the sneak request
/// and the standing pose before it, so correction replay reproduces the live tick.
#[test]
fn replay_of_a_ceiling_forced_crouch_tick_matches_live_prediction() {
    let mut physics = settled_controller();
    let running = step(
        &mut physics,
        sprint_forward(false, 0),
        ModeIntent::default(),
        &VersionedFloor(1),
    );
    assert!(running.processed.sprinting);
    let ceiling = LowCeiling(2.6);
    let forced = step(
        &mut physics,
        sprint_forward(false, 0),
        ModeIntent::default(),
        &ceiling,
    );
    assert!(forced.processed.forced_sneak && forced.sneaking);
    let replay = physics
        .replay_retained_from(running.tick, &ceiling)
        .unwrap();
    let [replayed] = replay.replayed_samples.as_slice() else {
        panic!("expected one replayed tick")
    };
    assert_eq!(replayed.processed, forced.processed);
    assert_eq!(replayed.position, forced.position);
}
