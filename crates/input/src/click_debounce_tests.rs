//! Deterministic switch-chatter regressions for ordered and aggregate input.

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
    project(ActionButtons::default(), held, [true; 2])
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
        if let Some(i) = ACTIONS.iter().position(|a| *a == action) {
            held[i] = pressed;
        }
        order.push(ActionEvent {
            action,
            pressed,
            buttons: buttons(held),
        });
    }
    for (i, action) in ACTIONS.into_iter().enumerate() {
        input.phases[action as usize].held = held[i];
    }
    input.ordered = Some(order);
    input
}

/// A bouncing press remains one accepted press followed by an uninterrupted hold.
#[test]
fn same_frame_press_bounce_preserves_hold_and_event_snapshots() {
    let mut filter = ClickDebouncer::default();
    let mut input = ordered(
        [false; 2],
        &[
            (Action::Attack, true),
            (Action::Attack, false),
            (Action::Attack, true),
            (Action::Jump, true),
        ],
    );
    filter.apply(&mut input, [Some(25), None], 0.);
    assert_eq!(
        input.phases[Action::Attack as usize],
        ActionPhase {
            pressed: true,
            held: true,
            released: false
        }
    );
    let events = input.ordered.as_ref().unwrap().events();
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .all(|event| event.buttons.held(Action::Attack))
    );
    let mut next = ordered([true, false], &[]);
    filter.apply(&mut next, [Some(25), None], 1.);
    assert_eq!(
        next.phases[Action::Attack as usize],
        ActionPhase {
            held: true,
            ..Default::default()
        }
    );
    assert!(next.ordered.unwrap().initial_buttons.held(Action::Attack));
}

/// Activating debounce on an existing hold does not add an attack or affect use clicks.
#[test]
fn activation_preserves_existing_hold_and_unfiltered_channel() {
    let mut filter = ClickDebouncer::default();
    let mut input = ordered([true, false], &[(Action::Use, true), (Action::Use, false)]);
    let original_use = input.phases[Action::Use as usize];
    filter.apply(&mut input, [Some(25), None], 0.);
    assert_eq!(
        input.phases[Action::Attack as usize],
        ActionPhase {
            held: true,
            ..Default::default()
        }
    );
    assert_eq!(input.phases[Action::Use as usize], original_use);
    assert_eq!(input.ordered.as_ref().unwrap().events().len(), 2);
}

/// A new physical press after the quiet interval follows the previous release once.
#[test]
fn settled_release_precedes_a_fresh_press_in_the_next_batch() {
    let mut filter = ClickDebouncer::default();
    let mut click = ordered(
        [false; 2],
        &[(Action::Attack, true), (Action::Attack, false)],
    );
    filter.apply(&mut click, [Some(25), None], 0.);
    let mut next = ordered([false; 2], &[(Action::Attack, true)]);
    filter.apply(&mut next, [Some(25), None], 0.03);
    assert_eq!(
        next.phases[Action::Attack as usize],
        ActionPhase {
            pressed: true,
            released: true,
            held: true
        }
    );
    let order = next.ordered.unwrap();
    assert_eq!(order.events().len(), 2);
    assert!(!order.events()[0].pressed && order.events()[1].pressed);
    assert!(!order.events()[0].buttons.held(Action::Attack));
    assert!(order.events()[1].buttons.held(Action::Attack));
}

/// Authorized pulses survive physical debounce while remaining subject to CPS caps.
#[test]
fn mod_attack_pulses_follow_debounce_and_keep_caps() {
    use crate::finalized::FinalizedInput;
    for desktop in [true, false] {
        let make_frame = |pressed, released, held| {
            if desktop {
                let events = if pressed {
                    vec![(Action::Attack, true)]
                } else if released {
                    vec![(Action::Attack, false)]
                } else {
                    vec![]
                };
                ordered([!pressed && (released || held), false], &events)
            } else {
                frame(
                    ActionPhase {
                        pressed,
                        released,
                        held,
                    },
                    Default::default(),
                )
            }
        };
        let mut debouncer = ClickDebouncer::default();
        let mut limiter = crate::ClickLimiter::default();
        let mut press = FinalizedInput::from_finalized(make_frame(true, false, true));
        press.debounce_clicks(&mut debouncer, [Some(25), None], 0.);
        press.limit_clicks(&mut limiter, [Some(10), None], 0.);
        let mut held = FinalizedInput::from_finalized(make_frame(false, false, true));
        let physical_held = held.phase(Action::Attack).held;
        held.debounce_clicks(&mut debouncer, [Some(25), None], 0.1);
        assert!(physical_held && held.request_mod_attack_press());
        held.limit_clicks(&mut limiter, [Some(10), None], 0.1);
        assert!(held.phase(Action::Attack).pressed);
        if desktop {
            assert!(
                held.snapshot()
                    .unwrap()
                    .ordered
                    .as_ref()
                    .unwrap()
                    .events()
                    .iter()
                    .any(|event| event.action == Action::Attack && event.pressed)
            );
        }
        let mut capped = FinalizedInput::from_finalized(make_frame(false, false, true));
        capped.debounce_clicks(&mut debouncer, [Some(25), None], 0.001);
        assert!(capped.request_mod_attack_press());
        capped.limit_clicks(&mut limiter, [Some(10), None], 0.001);
        assert!(!capped.phase(Action::Attack).pressed);
        let mut released = FinalizedInput::from_finalized(make_frame(false, true, false));
        let physical_held = released.phase(Action::Attack).held;
        released.debounce_clicks(&mut debouncer, [Some(25), None], 0.001);
        assert!(released.phase(Action::Attack).held);
        assert!(!(physical_held && released.request_mod_attack_press()));
        assert!(!released.phase(Action::Attack).pressed);
    }
}

/// Cross-frame chatter cancels the release instead of hiding the following hold.
#[test]
fn cross_frame_press_bounce_and_final_release_are_coalesced() {
    let mut filter = ClickDebouncer::default();
    let mut press = ordered([false; 2], &[(Action::Use, true)]);
    filter.apply(&mut press, [None, Some(25)], 0.);
    let mut release = ordered([false, true], &[(Action::Use, false)]);
    filter.apply(&mut release, [None, Some(25)], 0.005);
    assert!(release.phases[Action::Use as usize].held);
    assert!(!release.phases[Action::Use as usize].released);
    let mut bounce = ordered([false; 2], &[(Action::Use, true)]);
    filter.apply(&mut bounce, [None, Some(25)], 0.005);
    assert_eq!(
        bounce.phases[Action::Use as usize],
        ActionPhase {
            held: true,
            ..Default::default()
        }
    );
    let mut holding = ordered([false, true], &[]);
    filter.apply(&mut holding, [None, Some(25)], 0.1);
    assert!(holding.phases[Action::Use as usize].held);
    let mut release = ordered([false, true], &[(Action::Use, false)]);
    filter.apply(&mut release, [None, Some(25)], 0.1);
    let mut settling = ordered([false; 2], &[]);
    filter.apply(&mut settling, [None, Some(25)], 0.024);
    assert!(settling.phases[Action::Use as usize].held);
    let mut settled = ordered([false; 2], &[(Action::Jump, true)]);
    filter.apply(&mut settled, [None, Some(25)], 0.001);
    assert_eq!(
        settled.phases[Action::Use as usize],
        ActionPhase {
            released: true,
            ..Default::default()
        }
    );
    let order = settled.ordered.as_ref().unwrap();
    assert_eq!(order.events()[0].action, Action::Use);
    assert!(!order.events()[0].pressed);
    assert!(order.initial_buttons.held(Action::Use));
    assert!(
        order
            .events()
            .iter()
            .all(|event| !event.buttons.held(Action::Use))
    );
    let mut idle = ordered([false; 2], &[]);
    filter.apply(&mut idle, [None, Some(25)], 1.);
    assert_eq!(idle.phases[Action::Use as usize], ActionPhase::default());
}

/// Quick complete clicks are immediate presses with delayed, independently timed releases.
#[test]
fn aggregate_clicks_and_channel_intervals_preserve_real_gestures() {
    let mut filter = ClickDebouncer::default();
    let click = ActionPhase {
        pressed: true,
        released: true,
        held: false,
    };
    let mut input = frame(click, click);
    filter.apply(&mut input, [Some(10), Some(50)], 0.);
    for action in ACTIONS {
        assert_eq!(
            input.phases[action as usize],
            ActionPhase {
                pressed: true,
                held: true,
                released: false
            }
        );
    }
    let mut idle = frame(Default::default(), Default::default());
    filter.apply(&mut idle, [Some(10), Some(50)], 0.01);
    assert!(idle.phases[Action::Attack as usize].released);
    assert!(idle.phases[Action::Use as usize].held);
    let mut next = frame(click, Default::default());
    filter.apply(&mut next, [Some(10), Some(50)], 0.01);
    assert!(next.phases[Action::Attack as usize].pressed);
    let mut idle = frame(Default::default(), Default::default());
    filter.apply(&mut idle, [Some(10), Some(50)], 0.03);
    assert!(
        idle.phases[Action::Attack as usize].released && idle.phases[Action::Use as usize].released
    );
    assert!(!idle.phases[Action::Attack as usize].held && !idle.phases[Action::Use as usize].held);
}

/// Aggregate frames that finish held never start a deferred release from earlier chatter.
#[test]
fn aggregate_bounced_press_stays_held() {
    let mut filter = ClickDebouncer::default();
    let mut input = frame(
        ActionPhase {
            pressed: true,
            released: true,
            held: true,
        },
        Default::default(),
    );
    filter.apply(&mut input, [Some(25), None], 0.);
    assert!(
        input.phases[Action::Attack as usize].pressed && input.phases[Action::Attack as usize].held
    );
    let mut held = frame(
        ActionPhase {
            held: true,
            ..Default::default()
        },
        Default::default(),
    );
    filter.apply(&mut held, [Some(25), None], 1.);
    assert_eq!(
        held.phases[Action::Attack as usize],
        ActionPhase {
            held: true,
            ..Default::default()
        }
    );
}

/// Turning a channel off flushes its owned release without touching the other channel.
#[test]
fn disabling_and_changing_intervals_do_not_leave_stuck_buttons() {
    let mut filter = ClickDebouncer::default();
    let mut input = ordered(
        [false; 2],
        &[
            (Action::Attack, true),
            (Action::Use, true),
            (Action::Attack, false),
            (Action::Use, false),
        ],
    );
    filter.apply(&mut input, [Some(100), Some(100)], 0.);
    let mut changed = ordered([false; 2], &[]);
    filter.apply(&mut changed, [Some(10), None], 0.005);
    assert!(changed.phases[Action::Attack as usize].held);
    assert!(changed.phases[Action::Use as usize].released);
    let mut settled = ordered([false; 2], &[]);
    filter.apply(&mut settled, [Some(10), None], 0.01);
    assert!(settled.phases[Action::Attack as usize].released);
    assert_eq!(settled.phases[Action::Use as usize], ActionPhase::default());
    let mut disabled = ordered(
        [false; 2],
        &[(Action::Attack, true), (Action::Attack, false)],
    );
    let before = disabled.clone();
    filter.apply(&mut disabled, [None; 2], 1.);
    assert_eq!(disabled, before);
}

/// Release prefixes preserve both buttons' event-time state in order.
#[test]
fn simultaneous_pending_releases_keep_consistent_ordered_projections() {
    let mut filter = ClickDebouncer::default();
    let mut input = ordered(
        [false; 2],
        &[
            (Action::Attack, true),
            (Action::Use, true),
            (Action::Attack, false),
            (Action::Use, false),
        ],
    );
    filter.apply(&mut input, [Some(25); 2], 0.);
    let mut settled = ordered([false; 2], &[]);
    filter.apply(&mut settled, [Some(25); 2], 0.025);
    let order = settled.ordered.unwrap();
    assert!(order.initial_buttons.held(Action::Attack) && order.initial_buttons.held(Action::Use));
    assert!(!order.events()[0].buttons.held(Action::Attack));
    assert!(order.events()[0].buttons.held(Action::Use));
    assert!(!order.events()[1].buttons.held(Action::Use));
}

/// Invalid deltas cannot expire releases, and authority transitions retire pending ownership.
#[test]
fn invalid_time_and_authority_changes_cannot_replay_releases() {
    let mut filter = ClickDebouncer::default();
    let mut input = ordered(
        [false; 2],
        &[(Action::Attack, true), (Action::Attack, false)],
    );
    filter.apply(&mut input, [Some(25), None], 0.);
    for dt in [f64::NAN, f64::INFINITY, -1.] {
        let mut idle = ordered([false; 2], &[]);
        filter.apply(&mut idle, [Some(25), None], dt);
        assert!(
            idle.phases[Action::Attack as usize].held
                && !idle.phases[Action::Attack as usize].released
        );
    }
    let mut boundary = ordered([false; 2], &[]);
    boundary.authority_released = true;
    let before = boundary.clone();
    filter.apply(&mut boundary, [Some(25), None], 1.);
    assert_eq!(boundary, before);
    let mut idle = ordered([false; 2], &[]);
    filter.apply(&mut idle, [Some(25), None], 1.);
    assert_eq!(idle.phases[Action::Attack as usize], ActionPhase::default());
    let mut input = ordered(
        [false; 2],
        &[(Action::Attack, true), (Action::Attack, false)],
    );
    filter.apply(&mut input, [Some(25), None], 0.);
    let mut next = ordered([false; 2], &[]);
    next.authority_generation =
        crate::identity::InputAuthorityGeneration::new(core::num::NonZeroU64::new(2).unwrap());
    filter.apply(&mut next, [Some(25), None], 1.);
    assert_eq!(next.phases[Action::Attack as usize], ActionPhase::default());
}
