use super::*;
use semantic_input::{ActionSnapshot, DesktopEvent, DesktopEvents};

/// Resolves a complete arrival batch alongside its final held desktop state.
fn resolve(
    router: &mut SemanticInputRouter,
    held: Vec<u8>,
    events: &[(u8, bool)],
) -> ActionSnapshot {
    let mut ordered = DesktopEvents::default();
    for &(button, pressed) in events {
        ordered.push(DesktopEvent::Button {
            control: PhysicalControl::MouseButton(button),
            pressed,
            modifiers: ModifierChord::default(),
        });
    }
    router
        .route(DeviceFrame {
            desktop_events: Some(ordered),
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_buttons: held,
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    router.finalize().unwrap()
}

/// Reads event identity without hiding repeated equal transitions behind a set.
fn edges(snapshot: &ActionSnapshot) -> Vec<(Action, bool)> {
    snapshot
        .ordered
        .as_ref()
        .unwrap()
        .events()
        .iter()
        .map(|edge| (edge.action, edge.pressed))
        .collect()
}

#[test]
fn ordered_desktop_repeats_and_mixed_buttons_preserve_arrival_order() {
    let snapshot = resolve(
        &mut SemanticInputRouter::default(),
        vec![],
        &[
            (1, true),
            (1, false),
            (2, true),
            (2, false),
            (1, true),
            (1, false),
        ],
    );
    assert_eq!(
        edges(&snapshot),
        [
            (Action::Attack, true),
            (Action::Attack, false),
            (Action::Use, true),
            (Action::Use, false),
            (Action::Attack, true),
            (Action::Attack, false)
        ]
    );
    let attack = snapshot.phases[Action::Attack as usize];
    assert!(attack.pressed && attack.released && !attack.held);
}

#[test]
fn ordered_desktop_retains_every_click_in_a_large_complete_batch() {
    let events: Vec<_> = (0..300).flat_map(|_| [(1, true), (1, false)]).collect();
    let snapshot = resolve(&mut SemanticInputRouter::default(), vec![], &events);
    let expected: Vec<_> = events
        .iter()
        .map(|&(_, pressed)| (Action::Attack, pressed))
        .collect();
    assert_eq!(edges(&snapshot), expected);
    assert!(!snapshot.phases[Action::Attack as usize].held);
}

#[test]
fn ordered_desktop_held_state_follows_the_consumed_cursor() {
    let mut router = SemanticInputRouter::default();
    resolve(&mut router, vec![1], &[(1, true)]);
    let snapshot = resolve(
        &mut router,
        vec![1],
        &[(3, true), (3, false), (1, false), (1, true)],
    );
    let ordered = snapshot.ordered.as_ref().unwrap();
    assert_eq!(
        (0..=ordered.events().len())
            .map(|cursor| ordered.buttons_at(cursor).held(Action::Attack))
            .collect::<Vec<_>>(),
        [true, true, true, false, true]
    );
    let held = resolve(&mut router, vec![1], &[]);
    assert!(
        held.ordered
            .as_ref()
            .unwrap()
            .buttons_at(0)
            .held(Action::Attack)
    );
}

#[test]
fn ordered_desktop_consumed_control_rearms_on_its_release_within_a_batch() {
    let mut router = SemanticInputRouter::default();
    router.consume_control(PhysicalControl::MouseButton(1));
    let snapshot = resolve(&mut router, vec![1], &[(1, true), (1, false), (1, true)]);
    assert_eq!(edges(&snapshot), [(Action::Attack, true)]);
    assert!(snapshot.phases[Action::Attack as usize].held);
}

#[test]
fn ordered_desktop_scroll_keeps_quarantined_buttons_suppressed_until_release() {
    for next in [false, true] {
        let mut router = SemanticInputRouter::default();
        resolve(&mut router, vec![1, 2], &[(1, true), (2, true)]);
        for button in [1, 2] {
            router.consume_control(PhysicalControl::MouseButton(button));
        }
        let mut events = DesktopEvents::default();
        events.push(DesktopEvent::Scroll { next });
        router
            .route(DeviceFrame {
                desktop_events: Some(events),
                keyboard_mouse: Some(KeyboardMouseFrame {
                    activity_sequence: 1,
                    mouse_buttons: vec![1, 2],
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap();
        let snapshot = router.finalize().unwrap();
        assert_eq!(
            edges(&snapshot),
            [(
                if next {
                    Action::HotbarNext
                } else {
                    Action::HotbarPrevious
                },
                true
            )]
        );
        let ordered = snapshot.ordered.as_ref().unwrap();
        for action in [Action::Attack, Action::Use] {
            assert!(!snapshot.phases[action as usize].held);
            for cursor in 0..=ordered.events().len() {
                assert!(!ordered.buttons_at(cursor).held(action));
            }
        }
        let rearmed = resolve(
            &mut router,
            vec![1, 2],
            &[(1, false), (2, false), (1, true), (2, true)],
        );
        assert_eq!(
            edges(&rearmed),
            [(Action::Attack, true), (Action::Use, true)]
        );
    }
}

#[test]
fn ordered_desktop_uses_event_time_modifiers_and_the_shared_binding_evaluator() {
    let bindings = vec![
        ActionBinding {
            action: Action::Attack,
            context: InputContext::Gameplay,
            chord: empty_chord(PhysicalControl::MouseButton(4)),
        },
        ActionBinding {
            action: Action::Use,
            context: InputContext::Gameplay,
            chord: InputChord {
                control: PhysicalControl::MouseButton(4),
                modifiers: ModifierChord {
                    shift: true,
                    ..Default::default()
                },
            },
        },
    ];
    let settings = ControlSettings::new(bindings, 1.0, 1.0, 1.0, false, false, 0.0, 0.0).unwrap();
    let mut router = SemanticInputRouter::with_settings_and_touch_layout(
        settings,
        TouchControlLayout::default(),
    )
    .unwrap();
    let mut events = DesktopEvents::default();
    for (control, pressed, shift) in [
        (PhysicalControl::MouseButton(4), true, false),
        (PhysicalControl::MouseButton(4), false, false),
        (PhysicalControl::KeyboardUsage(0xe1), true, true),
        (PhysicalControl::MouseButton(4), true, true),
        (PhysicalControl::MouseButton(4), false, true),
        (PhysicalControl::KeyboardUsage(0xe1), false, false),
    ] {
        events.push(DesktopEvent::Button {
            control,
            pressed,
            modifiers: ModifierChord {
                shift,
                ..Default::default()
            },
        });
    }
    router
        .route(DeviceFrame {
            desktop_events: Some(events),
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        edges(&router.finalize().unwrap()),
        [
            (Action::Attack, true),
            (Action::Attack, false),
            (Action::Use, true),
            (Action::Use, false)
        ]
    );
}

#[test]
fn ordered_desktop_focus_and_context_boundaries_drop_the_entire_batch() {
    let mut router = SemanticInputRouter::default();
    resolve(&mut router, vec![1], &[(1, true)]);
    router.set_context(InputContext::UiFocused);
    let cancelled = resolve(&mut router, vec![1], &[(1, false), (1, true)]);
    assert!(edges(&cancelled).is_empty());
    assert!(cancelled.authority_released);
    router.set_context(InputContext::Gameplay);
    resolve(&mut router, vec![1], &[]);
    assert!(edges(&resolve(&mut router, vec![1], &[])).is_empty());
    resolve(&mut router, vec![], &[(1, false)]);
    assert_eq!(
        edges(&resolve(&mut router, vec![], &[(1, true), (1, false)])),
        [(Action::Attack, true), (Action::Attack, false)]
    );
}

#[test]
fn ordered_desktop_overflow_discards_the_prefix_and_releases_held_input() {
    let mut router = SemanticInputRouter::default();
    resolve(&mut router, vec![1], &[(1, true)]);
    let mut events = DesktopEvents::default();
    events.invalidate();
    router
        .route(DeviceFrame {
            desktop_events: Some(events),
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_buttons: vec![1],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    let dropped = router.finalize().unwrap();
    assert!(dropped.ordered.as_ref().unwrap().overflowed());
    assert!(edges(&dropped).is_empty());
    assert!(!dropped.phases[Action::Attack as usize].held);
    assert_eq!(
        dropped.release_reasons[Action::Attack as usize],
        Some(ReleaseReason::InputOverflow)
    );
    assert!(edges(&resolve(&mut router, vec![1], &[])).is_empty());
    resolve(&mut router, vec![], &[(1, false)]);
    assert_eq!(
        edges(&resolve(&mut router, vec![], &[(1, true), (1, false)])),
        [(Action::Attack, true), (Action::Attack, false)]
    );
}

#[test]
fn ordered_desktop_does_not_replace_the_existing_gamepad_admission_path() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            desktop_events: Some(DesktopEvents::default()),
            controllers: vec![ControllerFrame {
                device_id: 1,
                activity_sequence: 1,
                buttons: vec![7],
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().ordered.is_none());
}
