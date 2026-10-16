use super::tests::{Floor, movement_frame};
use super::*;
use client_interaction::movement::MovementTicker;
use gameplay::movement::{
    LocalMovementEffectTimeline, LocalMovementSpeedAuthority, MovementSource, PhysicsSampleContext,
    RideKind,
};
use protocol::ClientInputLock;
use semantic_input::ActionPhase;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
enum PendingRequest {
    JumpEdge,
    ModJump,
    SneakEdge,
    SneakToggle,
    SprintToggle,
    BoatDismount,
    OtherDismount,
    LookTail,
}

/// Seeds and consumes real request owners around the production commit handler.
struct Harness {
    facts: LocalPlayerFacts,
    physics: LocalPhysicsController,
    locals: LocomotionState,
    look: LookSmoother,
    ticker: MovementTicker,
    effects: LocalMovementEffectTimeline,
    speed: LocalMovementSpeedAuthority,
}

impl Harness {
    /// Starts grounded prediction without consuming any request.
    fn new() -> Self {
        let position = [0.0, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position(position, 100, true);
        let mut ticker = MovementTicker::default();
        ticker.reset(7, 100, position);
        ticker.set_source(MovementSource::Physics);
        Self {
            facts: LocalPlayerFacts::new(7),
            physics,
            locals: Default::default(),
            look: Default::default(),
            ticker,
            effects: Default::default(),
            speed: Default::default(),
        }
    }

    /// Retains button edges, control latches, queue-full dismounts or cinematic motion.
    fn seed(&mut self, request: PendingRequest) {
        match request {
            PendingRequest::JumpEdge | PendingRequest::SneakEdge => {
                let mut context = PhysicsSampleContext::default();
                let phase = ActionPhase {
                    pressed: true,
                    held: false,
                    released: true,
                };
                match request {
                    PendingRequest::JumpEdge => context.input.jump = phase,
                    _ => context.input.sneak = phase,
                }
                assert!(
                    self.physics
                        .advance_with_context(
                            Duration::from_millis(10),
                            sim::MovementInput::default(),
                            context,
                            &Floor
                        )
                        .samples
                        .is_empty()
                );
            }
            PendingRequest::ModJump => {
                self.physics.set_jump_pulse_scope(Some((7, 0)));
                self.physics.request_jump_pulse();
            }
            PendingRequest::SneakToggle | PendingRequest::SprintToggle => {
                let mut frame = movement_frame();
                frame.toggle_sneak = true;
                frame.toggle_sprint = true;
                frame.movement = [0.0, 1.0];
                match request {
                    PendingRequest::SneakToggle => frame.sneak.pressed = true,
                    _ => frame.sprint.pressed = true,
                }
                assert!(self.locals.advance(
                    frame,
                    &mut self.physics,
                    &mut self.ticker,
                    &mut self.effects,
                    &mut self.speed,
                    &Floor
                ));
                assert_eq!(self.ticker.completed_tick(), 100);
            }
            PendingRequest::BoatDismount | PendingRequest::OtherDismount => {
                let mut frame = movement_frame();
                frame.facts.mount_runtime_id = Some(9);
                match request {
                    PendingRequest::BoatDismount => {
                        frame.facts.ride = Some(RideKind::Boat);
                        frame.jump.pressed = true;
                    }
                    _ => {
                        frame.facts.ride = Some(RideKind::Other);
                        frame.sneak.pressed = true;
                    }
                }
                self.locals.observe_dismount(&frame, 7);
                let mut attempted = false;
                self.locals.flush_dismount([0.0; 3], |_| {
                    attempted = true;
                    Err(protocol::BatchSendError::Full)
                });
                assert!(attempted);
            }
            PendingRequest::LookTail => {
                self.look.filter(bevy::prelude::Vec2::new(80.0, 20.0), 0.02);
            }
        }
    }

    /// Applies lock and unlock without a prediction or camera frame between them.
    fn commit_pair(&mut self, locks: ClientInputLocks) {
        for locks in [locks, ClientInputLocks::default()] {
            apply_committed_input_locks(
                7,
                locks,
                &mut self.facts,
                &mut self.physics,
                &mut self.locals,
                &mut self.look,
            );
        }
    }

    /// Consumes the retained request, asserting that release edges remain deliverable.
    fn survives(&mut self, request: PendingRequest) -> bool {
        match request {
            PendingRequest::JumpEdge | PendingRequest::ModJump | PendingRequest::SneakEdge => {
                let frame = self.physics.advance(
                    Duration::from_millis(50),
                    sim::MovementInput::default(),
                    &Floor,
                );
                assert_eq!(frame.samples.len(), 1);
                let sample = &frame.samples[0];
                match request {
                    PendingRequest::JumpEdge => {
                        assert!(sample.input.jump.released);
                        sample.processed.jump_initiated
                    }
                    PendingRequest::SneakEdge => {
                        assert!(sample.input.sneak.released);
                        sample.input.sneak.pressed
                    }
                    _ => sample.processed.jump_initiated,
                }
            }
            PendingRequest::SneakToggle | PendingRequest::SprintToggle => {
                let mut frame = movement_frame();
                frame.delta = Duration::from_millis(40);
                frame.toggle_sneak = true;
                frame.toggle_sprint = true;
                frame.movement = [0.0, 1.0];
                assert!(self.locals.advance(
                    frame,
                    &mut self.physics,
                    &mut self.ticker,
                    &mut self.effects,
                    &mut self.speed,
                    &Floor
                ));
                let sample = self.physics.sample_at(101).unwrap();
                match request {
                    PendingRequest::SneakToggle => sample.sneaking,
                    _ => sample.input.sprint_down,
                }
            }
            PendingRequest::BoatDismount | PendingRequest::OtherDismount => {
                let mut sent = false;
                self.locals.flush_dismount([0.0; 3], |_| {
                    sent = true;
                    Ok(())
                });
                sent
            }
            PendingRequest::LookTail => {
                self.look.filter(bevy::prelude::Vec2::ZERO, 0.02) != bevy::prelude::Vec2::ZERO
            }
        }
    }
}

/// Specifies revocation independently, with exhaustive matches on lock categories.
fn revokes(lock: ClientInputLock, request: PendingRequest) -> bool {
    use PendingRequest::*;
    match lock {
        ClientInputLock::Camera => matches!(request, LookTail),
        ClientInputLock::Movement => !matches!(request, LookTail),
        ClientInputLock::LateralMovement | ClientInputLock::MoveForward => {
            matches!(request, SprintToggle)
        }
        ClientInputLock::Sneak => matches!(request, SneakEdge | SneakToggle | OtherDismount),
        ClientInputLock::Jump => matches!(request, JumpEdge | ModJump | BoatDismount),
        ClientInputLock::Dismount => matches!(request, BoatDismount | OtherDismount),
        ClientInputLock::Mount
        | ClientInputLock::MoveBackward
        | ClientInputLock::MoveLeft
        | ClientInputLock::MoveRight => false,
    }
}

#[test]
fn server_input_locks_revoke_every_pending_request_category() {
    use PendingRequest::*;
    for request in [
        JumpEdge,
        ModJump,
        SneakEdge,
        SneakToggle,
        SprintToggle,
        BoatDismount,
        OtherDismount,
        LookTail,
    ] {
        let mut unlocked = Harness::new();
        unlocked.seed(request);
        unlocked.commit_pair(ClientInputLocks::default());
        assert!(unlocked.survives(request), "unlocked {request:?}");
        for &lock in ClientInputLock::ALL {
            let mut harness = Harness::new();
            harness.seed(request);
            harness.commit_pair(ClientInputLocks::from_bits(lock as u32));
            assert_eq!(
                harness.survives(request),
                !revokes(lock, request),
                "{lock:?} × {request:?}"
            );
        }
    }
}
