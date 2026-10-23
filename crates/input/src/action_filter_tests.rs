//! Generic action-verdict and projection regressions.

use super::*;

/// Creates a physical frame without a device, window or clock.
fn frame(attack: ActionPhase, item_use: ActionPhase) -> ActionSnapshot {
    let mut phases = [ActionPhase::default(); Action::COUNT];
    phases[Action::Attack as usize] = attack;
    phases[Action::Use as usize] = item_use;
    ActionSnapshot {
        ordered: None,
        frame_sequence: crate::identity::InputSequence::new(1),
        authority_generation: crate::identity::InputAuthorityGeneration::INITIAL,
        movement: [0.; 2],
        raw_movement: [0.; 2],
        analogue_movement: [0.; 2],
        movement_buttons: Default::default(),
        look_delta: [0.; 2],
        input_mode: crate::InputMode::KeyboardMouse,
        phases,
        release_reasons: [None; Action::COUNT],
        authority_released: false,
    }
}

/// Projects attack/use ownership while leaving the other buttons up.
fn buttons(held: [bool; 2]) -> ActionButtons {
    project(ActionButtons::default(), held)
}

/// Adds desktop transitions with consistent physical event-time button snapshots.
fn ordered(initial: [bool; 2], events: &[(Action, bool)]) -> ActionSnapshot {
    let mut held = initial;
    let mut order = crate::OrderedActions::default();
    order.initial_buttons = buttons(held);
    let mut input = frame(Default::default(), Default::default());
    for &(action, pressed) in events {
        let phase = &mut input.phases[action as usize];
        if pressed {
            phase.pressed = true;
        } else {
            phase.released = true;
        }
        if let Some(i) = FilterAction::ALL
            .map(|a| a.action())
            .iter()
            .position(|a| *a == action)
        {
            held[i] = pressed;
        }
        order.push(ActionEvent {
            action,
            pressed,
            buttons: buttons(held),
        });
    }
    for (i, action) in FilterAction::ALL
        .map(|a| a.action())
        .into_iter()
        .enumerate()
    {
        input.phases[action as usize].held = held[i];
    }
    input.ordered = Some(order);
    input
}

/// Accepts an arbitrary transition mask without any native timing policy.
fn decide(mask: &[bool], release: [bool; 2]) -> Option<ActionVerdict> {
    Some(ActionVerdict {
        accepted: mask.to_vec(),
        release,
    })
}

#[test]
fn suppressed_release_and_press_preserve_hold_and_unrelated_snapshots() {
    let mut state = ActionFilterState::default();
    let mut input = ordered(
        [false; 2],
        &[
            (Action::Attack, true),
            (Action::Attack, false),
            (Action::Attack, true),
            (Action::Jump, true),
        ],
    );
    state.filter(&mut input, Some(1), 0, false, |_| {
        decide(&[true, false, false], [false; 2])
    });
    assert_eq!(
        input.phases[Action::Attack as usize],
        ActionPhase {
            pressed: true,
            held: true,
            released: false
        }
    );
    let events = input.ordered.unwrap().events;
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|e| e.buttons.held(Action::Attack)));
}
#[test]
fn retained_release_completes_before_a_new_press() {
    let mut state = ActionFilterState::default();
    let mut first = ordered(
        [false; 2],
        &[(Action::Attack, true), (Action::Attack, false)],
    );
    state.filter(&mut first, Some(1), 0, false, |_| {
        decide(&[true, false], [false; 2])
    });
    let mut next = ordered([false; 2], &[(Action::Jump, true), (Action::Attack, true)]);
    state.filter(&mut next, Some(1), 1, false, |f| {
        assert_eq!(f.logical, [true, false]);
        decide(&[true], [false; 2])
    });
    let events = next.ordered.unwrap().events;
    assert_eq!(
        events
            .iter()
            .map(|e| (e.action, e.pressed))
            .collect::<Vec<_>>(),
        [
            (Action::Jump, true),
            (Action::Attack, false),
            (Action::Attack, true)
        ]
    );
    assert!(events[0].buttons.held(Action::Attack));
    assert!(!events[1].buttons.held(Action::Attack));
    assert!(events[2].buttons.held(Action::Attack));
}
#[test]
fn explicit_releases_keep_both_channels_consistent() {
    let mut state = ActionFilterState::default();
    let mut first = ordered([true; 2], &[(Action::Attack, false), (Action::Use, false)]);
    state.filter(&mut first, Some(1), 0, false, |_| {
        decide(&[false, false], [false; 2])
    });
    let mut next = ordered([false; 2], &[(Action::Jump, true)]);
    state.filter(&mut next, Some(1), 1, false, |_| decide(&[], [true; 2]));
    let events = next.ordered.unwrap().events;
    assert_eq!(events.len(), 3);
    assert!(!events[0].buttons.held(Action::Attack));
    assert!(events[0].buttons.held(Action::Use));
    assert!(!events[1].buttons.held(Action::Use));
    assert!(!events[2].buttons.held(Action::Attack));
}
#[test]
fn suppressed_press_remains_blocked_across_failure_until_release() {
    let mut state = ActionFilterState::default();
    let mut first = ordered([false; 2], &[(Action::Attack, true)]);
    state.filter(&mut first, Some(1), 0, false, |_| {
        decide(&[false], [false; 2])
    });
    state.reset();
    let mut held = ordered([true, false], &[]);
    state.filter(&mut held, None, 1, false, |_| panic!("no owner"));
    assert_eq!(held.phases[Action::Attack as usize], ActionPhase::default());
    let mut end = ordered([true, false], &[(Action::Attack, false)]);
    state.filter(&mut end, None, 2, false, |_| panic!("no owner"));
    assert!(end.ordered.unwrap().events.is_empty());
    let mut press = ordered([false; 2], &[(Action::Attack, true)]);
    state.filter(&mut press, None, 3, false, |_| panic!("no owner"));
    assert!(press.phases[Action::Attack as usize].pressed);
}
#[test]
fn failure_and_malformed_verdict_complete_retained_release_without_new_presses() {
    for malformed in [false, true] {
        let mut state = ActionFilterState::default();
        let mut first = ordered([true, false], &[(Action::Attack, false)]);
        state.filter(&mut first, Some(1), 0, false, |_| {
            decide(&[false], [false; 2])
        });
        let mut idle = ordered([false; 2], &[]);
        state.filter(&mut idle, Some(1), 1, false, |_| {
            if malformed {
                decide(&[true], [false; 2])
            } else {
                None
            }
        });
        assert_eq!(
            idle.phases[Action::Attack as usize],
            ActionPhase {
                released: true,
                ..Default::default()
            }
        );
    }
}
#[test]
fn ownership_and_authority_changes_reconcile_before_the_new_callback() {
    for reset in 0..4 {
        let mut state = ActionFilterState::default();
        let mut first = ordered([true, false], &[(Action::Attack, false)]);
        let mut epoch = 0;
        state.filter(&mut first, Some(1), 0, false, |f| {
            epoch = f.epoch;
            decide(&[false], [false; 2])
        });
        let mut next = ordered([false; 2], &[(Action::Attack, true)]);
        if reset == 1 {
            state.reset();
        }
        if reset == 2 {
            next.authority_generation = crate::identity::InputAuthorityGeneration::new(
                core::num::NonZeroU64::new(2).unwrap(),
            );
        }
        if reset == 3 {
            next.ordered.as_mut().unwrap().invalidate();
        }
        state.filter(
            &mut next,
            Some(if reset == 0 { 2 } else { 1 }),
            1,
            false,
            |f| {
                assert_ne!(f.epoch, epoch);
                assert_eq!(f.logical, [false; 2]);
                decide(&[true], [false; 2])
            },
        );
        assert!(next.phases[Action::Attack as usize].released);
        assert_eq!(next.phases[Action::Attack as usize].pressed, reset != 3);
    }
}
#[test]
fn authorized_repeat_is_independent_of_physical_edges_and_rejection_keeps_hold() {
    for accept in [true, false] {
        let mut state = ActionFilterState::default();
        let mut input = ordered([true, false], &[]);
        let admitted = state.filter(&mut input, Some(1), 0, true, |f| {
            assert_eq!(
                f.transitions,
                [ActionTransition {
                    action: FilterAction::Attack,
                    kind: TransitionKind::Repeat
                }]
            );
            decide(&[accept], [false; 2])
        });
        assert_eq!(admitted, accept);
        assert!(input.phases[Action::Attack as usize].held);
        assert_eq!(input.phases[Action::Attack as usize].pressed, accept);
    }
    let mut state = ActionFilterState::default();
    let mut released = ordered([false; 2], &[]);
    assert!(!state.filter(&mut released, Some(1), 0, true, |_| panic!("unheld repeat")));
}
#[test]
fn phase_only_devices_preserve_masks_and_idle_physical_holds_skip_callbacks() {
    let mut state = ActionFilterState::default();
    let mut input = frame(
        ActionPhase {
            pressed: true,
            held: true,
            ..Default::default()
        },
        Default::default(),
    );
    state.filter(&mut input, Some(1), 0, false, |_| {
        decide(&[true], [false; 2])
    });
    let mut idle = frame(
        ActionPhase {
            held: true,
            ..Default::default()
        },
        Default::default(),
    );
    state.filter(&mut idle, Some(1), 1, false, |_| panic!("idle hold"));
    assert!(idle.phases[Action::Attack as usize].held);
    let mut release = frame(
        ActionPhase {
            released: true,
            ..Default::default()
        },
        Default::default(),
    );
    state.filter(&mut release, Some(1), 2, false, |_| {
        decide(&[false], [false; 2])
    });
    assert!(release.phases[Action::Attack as usize].held);
    let mut done = frame(Default::default(), Default::default());
    state.filter(&mut done, Some(1), 3, false, |_| decide(&[], [true, false]));
    assert!(done.phases[Action::Attack as usize].released);
}

/// Failures restore physical input while dropping callback-generated pulses.
#[test]
fn failed_callback_cannot_publish_an_earlier_mod_pulse() {
    let mut state = ActionFilterState::default();
    let mut input = ordered([true, false], &[]);
    assert!(!state.filter(&mut input, Some(1), 0, true, |_| None));
    assert!(input.phases[Action::Attack as usize].held);
    assert!(!input.phases[Action::Attack as usize].pressed);
}

/// A suppressed gesture cannot resume from repeated press notifications alone.
#[test]
fn duplicate_physical_press_does_not_restart_a_suppressed_hold() {
    let mut state = ActionFilterState::default();
    let mut first = ordered([false; 2], &[(Action::Attack, true)]);
    state.filter(&mut first, Some(1), 0, false, |_| {
        decide(&[false], [false; 2])
    });
    let mut duplicate = ordered([true, false], &[(Action::Attack, true)]);
    state.filter(&mut duplicate, Some(1), 1, false, |_| {
        decide(&[true], [false; 2])
    });
    assert_eq!(
        duplicate.phases[Action::Attack as usize],
        ActionPhase::default()
    );
}
