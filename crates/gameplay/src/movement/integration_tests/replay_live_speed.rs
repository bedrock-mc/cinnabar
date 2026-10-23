use super::*;
use crate::committed_control::{CommittedGameplayState, ControlObservation};
use client_world::{CommittedControlEvent, ResolvedServerPosition};

/// The gameplay entry points that publish a replayed speed to live movement.
#[derive(Clone, Copy, Debug)]
enum ReplayTrigger {
    Correction,
    Motion,
    Flags,
    Frame,
}

/// Applies one packet to the same owners used by live movement frames.
fn apply_control(
    control: CommittedControlEvent,
    ticker: &mut MovementTicker,
    physics: &mut LocalPhysicsController,
    effects: &mut super::super::LocalMovementEffectTimeline,
    speed: &mut super::super::LocalMovementSpeedAuthority,
) -> Vec<ControlObservation> {
    let mut observations = Vec::new();
    CommittedGameplayState {
        movement: ticker,
        physics,
        effects,
        speed,
        session_generation: 7,
        dimension: 0,
        dimension_transfer_active: false,
    }
    .apply(control, &VersionedFloor(1), |event| {
        observations.push(event)
    });
    observations
}

/// A delayed correction must preserve speed accepted since the last completed tick.
fn check_pending_speed(update: CommittedControlEvent, expected: f32, trigger: ReplayTrigger) {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::super::LocomotionState::default();
    let mut effects = super::super::LocalMovementEffectTimeline::default();
    let mut speed = super::super::LocalMovementSpeedAuthority::default();
    speed.begin_session(7, 0);
    assert!(speed.apply(7, 1, 0, attribute(f64::from(0.1_f32), 0.1, None)));
    for _ in 0..4 {
        assert!(locals.advance(
            speed_frame(Default::default()),
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
    }
    let latest = ticker.completed_tick();
    let anchor = physics.sample_at(latest - 2).unwrap().clone();
    apply_control(update, &mut ticker, &mut physics, &mut effects, &mut speed);
    assert_eq!(speed.current(), Some(f64::from(expected)));

    if matches!(trigger, ReplayTrigger::Frame) {
        reconcile_timeline_rewind(&mut ticker, &mut physics, anchor.tick, &VersionedFloor(1))
            .unwrap();
        let mut frame = speed_frame(Default::default());
        frame.delta = Duration::ZERO;
        assert!(locals.advance(
            frame,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
    } else {
        let mut position = anchor.position;
        position[0] += 0.25;
        let control = match trigger {
            ReplayTrigger::Correction => CommittedControlEvent::PlayerMovementCorrection {
                sequence: 3,
                correction: protocol::PlayerMovementCorrectionEvent {
                    position,
                    delta: anchor.velocity,
                    pitch: 0.0,
                    yaw: 0.0,
                    subject: protocol::MovementCorrectionSubject::Player,
                    on_ground: anchor.grounded_after_tick,
                    tick: anchor.tick,
                },
                resolved: ResolvedServerPosition {
                    position,
                    surface_anchor: None,
                },
            },
            ReplayTrigger::Motion => CommittedControlEvent::LocalActorMotion {
                sequence: 3,
                event: protocol::ActorMotionEvent {
                    actor_runtime_id: 42,
                    motion: [0.2, 0.0, 0.0],
                    tick: anchor.tick,
                },
            },
            ReplayTrigger::Flags => CommittedControlEvent::LocalMovementFlags {
                sequence: 3,
                tick: anchor.tick,
                flags: flags(|flags| flags.sprinting = Some(false)),
            },
            ReplayTrigger::Frame => unreachable!(),
        };
        let observations =
            apply_control(control, &mut ticker, &mut physics, &mut effects, &mut speed);
        if matches!(trigger, ReplayTrigger::Correction) {
            assert!(observations.iter().any(|event| matches!(
                event,
                ControlObservation::Correction {
                    outcome: PhysicsCorrectionOutcome::Replayed {
                        replayed_ticks: 2,
                        ..
                    },
                    ..
                }
            )));
        }
    }
    assert_eq!(ticker.completed_tick(), latest);
    assert_eq!(speed.current(), Some(f64::from(expected)), "{trigger:?}");

    let mut expected_physics = physics.clone();
    let expected_tick = expected_physics.advance_with_context(
        Duration::from_millis(50),
        MovementInput {
            movement_speed: Some(f64::from(expected)),
            ..physics_movement_input([0.0, 1.0], 180.0, true, false, false, false, None)
        },
        PhysicsSampleContext {
            movement_speed_attribute: speed.attribute(),
            ..Default::default()
        },
        &VersionedFloor(1),
    );
    assert_eq!(expected_tick.completed_ticks, 1);
    assert!(locals.advance(
        speed_frame(Default::default()),
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    assert_eq!(physics.state(), expected_physics.state());
    assert_eq!(speed.current(), Some(f64::from(expected)));

    // Once recorded, the update must also survive another rewind through its tick.
    reconcile_timeline_rewind(&mut ticker, &mut physics, latest - 1, &VersionedFloor(1)).unwrap();
    let mut frame = speed_frame(Default::default());
    frame.delta = Duration::ZERO;
    assert!(locals.advance(
        frame,
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    assert_eq!(physics.state(), expected_physics.state());
    assert_eq!(speed.current(), Some(f64::from(expected)));
}

#[test]
fn pending_speed_attribute_survives_correction_and_frame_replay_adoption() {
    for trigger in [
        ReplayTrigger::Correction,
        ReplayTrigger::Motion,
        ReplayTrigger::Flags,
        ReplayTrigger::Frame,
    ] {
        check_pending_speed(
            CommittedControlEvent::LocalMovementSpeed {
                sequence: 2,
                dimension: 0,
                movement: Some(attribute(f64::from(0.2_f32), 0.2, None)),
                underwater: None,
                lava: None,
                air_drag_modifier: None,
                tick: 0,
            },
            0.2,
            trigger,
        );
    }
}

#[test]
fn pending_speed_effect_survives_correction_and_frame_replay_adoption() {
    for trigger in [
        ReplayTrigger::Correction,
        ReplayTrigger::Motion,
        ReplayTrigger::Flags,
        ReplayTrigger::Frame,
    ] {
        check_pending_speed(
            CommittedControlEvent::LocalMovementEffect {
                sequence: 2,
                event: protocol::ActorEffectEvent {
                    dimension: 0,
                    actor_runtime_id: 42,
                    action: protocol::ActorEffectAction::Add,
                    effect_id: protocol_ids::Effect::Speed.wire_value(),
                    amplifier: 1,
                    particles: false,
                    ambient: false,
                    duration_ticks: 600,
                    tick: 104,
                },
            },
            0.1_f32 * 1.4,
            trigger,
        );
    }
}

/// A first speed effect followed by live sprint metadata keeps its speed across replay.
#[test]
fn first_speed_effect_before_live_sprint_metadata_matches_replay() {
    let (mut physics, mut ticker) = walked_physics(2);
    let anchor = ticker.completed_tick();
    let mut locals = super::super::LocomotionState::default();
    let mut effects = super::super::LocalMovementEffectTimeline::default();
    let mut speed = super::super::LocalMovementSpeedAuthority::default();
    speed.begin_session(7, 0);
    apply_control(
        CommittedControlEvent::LocalMovementEffect {
            sequence: 1,
            event: protocol::ActorEffectEvent {
                dimension: 0,
                actor_runtime_id: 42,
                action: protocol::ActorEffectAction::Add,
                effect_id: protocol_ids::Effect::Speed.wire_value(),
                amplifier: 0,
                particles: false,
                ambient: false,
                duration_ticks: 600,
                tick: 0,
            },
        },
        &mut ticker,
        &mut physics,
        &mut effects,
        &mut speed,
    );
    apply_control(
        CommittedControlEvent::LocalMovementFlags {
            sequence: 2,
            tick: 0,
            flags: flags(|flags| flags.sprinting = Some(true)),
        },
        &mut ticker,
        &mut physics,
        &mut effects,
        &mut speed,
    );
    assert!(locals.advance(
        speed_frame(semantic_input::ActionPhase {
            held: true,
            ..Default::default()
        }),
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    let live_state = physics.state().cloned();
    let live_speed = speed.current();
    assert_eq!(physics.latest_sneak_sprint(), Some((false, true)));
    assert_eq!(
        reconcile_timeline_rewind(&mut ticker, &mut physics, anchor, &VersionedFloor(1)).unwrap(),
        PhysicsCorrectionOutcome::Replayed {
            corrected_tick: anchor,
            replayed_ticks: 1,
        }
    );
    let mut frame = speed_frame(Default::default());
    frame.delta = Duration::ZERO;
    assert!(locals.advance(
        frame,
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    assert_eq!(physics.state(), live_state.as_ref());
    assert_eq!(speed.current(), live_speed);
    assert_eq!(live_speed, Some(f64::from(0.1_f32 * 1.2)));
}
