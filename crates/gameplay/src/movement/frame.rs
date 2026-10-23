//! Local movement frame coordination before ordered interaction admission.
use super::control_modes::{ControlModes, ControlObservation};
use super::local_facts::LocalMovementFacts;
use super::physics_authority_fault_for_frame;
use super::{
    LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController, ModeIntent,
    MovementTicker, PhysicsSampleContext, physics_movement_input,
};
use protocol::ClientInputLock;
use semantic_input::ActionPhase;
use std::time::Duration;
use tracing::debug;

/// Vanilla actor yaw: degrees wrapped to `[-180, 180)` with f32 `fmod`.
#[must_use]
pub fn wire_yaw(degrees: f32) -> f32 {
    let rem = (degrees + 180.0) % 360.0;
    let wrapped = if rem < 0.0 { rem + 360.0 } else { rem };
    wrapped - 180.0
}

/// Vanilla head yaw keeps the unwrapped camera angle, which lies in `(-270, 90]`.
#[must_use]
pub fn wire_head_yaw(yaw: f32) -> f32 {
    if yaw > 90.0 { yaw - 360.0 } else { yaw }
}

/// Vanilla clamps each render frame's elapsed time to this and loses the rest,
/// so a stall runs at most two ticks instead of catching up.
const MAX_FRAME_ELAPSED: Duration = simulation_clock::MAX_FRAME_ELAPSED;

/// Whether a render frame of `delta` will simulate at least one fixed tick.
pub fn frame_simulates_tick(
    physics: &LocalPhysicsController,
    ticker: &MovementTicker,
    delta: Duration,
) -> bool {
    physics.is_active()
        && ticker.can_advance_physics_frame()
        && physics.ticks_due(delta.min(MAX_FRAME_ELAPSED)) > 0
}

/// Immutable input and modifier facts sampled at the existing physics phase.
pub struct PhysicsFrameInput {
    pub delta: Duration,
    pub now: Duration,
    pub active: bool,
    /// Cancels retained presses when the semantic router releases input ownership.
    pub input_authority_released: bool,
    /// Mod-held controls bypass physical toggle latches.
    pub resolved_movement: bool,
    pub movement: [f32; 2],
    pub raw_movement: [f32; 2],
    pub analogue_movement: [f32; 2],
    pub movement_buttons: semantic_input::MovementButtons,
    pub yaw: f32,
    pub pitch: f32,
    pub camera_orientation: [f32; 3],
    pub input_mode: protocol::PlayerInputMode,
    pub jump: ActionPhase,
    pub sprint: ActionPhase,
    pub sneak: ActionPhase,
    pub toggle_sprint: bool,
    pub always_sprint: bool,
    pub toggle_sneak: bool,
    pub facts: LocalMovementFacts,
    pub item_use_modifier: Option<f64>,
    pub hold: Option<PhysicsFrameHold>,
}

/// Loading freezes movement; spawn search also suppresses wire input.
#[derive(Clone, Copy)]
pub struct PhysicsFrameHold {
    pub registry: sim::CollisionRegistryIdentity,
    pub withhold_input: bool,
}

/// Persistent movement-mode input latches and collision-blocker diagnostics.
#[derive(Default)]
pub struct LocomotionState {
    controls: ControlModes,
    /// Last active device; suspended input keeps reporting it.
    input_mode: protocol::PlayerInputMode,
    previous_blocker: Option<String>,
    /// A deferred press belongs to one session and one vehicle.
    pending_dismount: Option<(u64, u64, ClientInputLock)>,
}

impl LocomotionState {
    /// Revokes retained mode requests and dismounts, including their triggering category.
    pub fn revoke_input_requests(&mut self, locks: protocol::ClientInputLocks) {
        self.controls.revoke_input_requests(locks);
        if self.pending_dismount.is_some_and(|(_, _, trigger)| {
            locks.blocks(ClientInputLock::Dismount) || locks.blocks(trigger)
        }) {
            self.pending_dismount = None;
        }
    }

    /// Admits a requested dismount ahead of movement, retaining it only on a full queue.
    pub fn flush_dismount(
        &mut self,
        position: [f32; 3],
        mut send: impl FnMut(protocol::Packet) -> Result<(), protocol::BatchSendError>,
    ) {
        let Some((_, mount, _)) = self.pending_dismount else {
            return;
        };
        let Ok(packet) = protocol::stop_riding_packet(mount, position) else {
            self.pending_dismount = None;
            return;
        };
        if send(packet) != Err(protocol::BatchSendError::Full) {
            self.pending_dismount = None;
        }
    }
    /// Clears input latches when local prediction is inactive.
    pub fn reset(&mut self) {
        self.controls.reset();
        self.pending_dismount = None;
    }

    /// Runs each fixed tick and admits its sample before any interaction producers run.
    pub fn advance(
        &mut self,
        frame: PhysicsFrameInput,
        physics: &mut LocalPhysicsController,
        movement_ticker: &mut MovementTicker,
        movement_effects: &mut LocalMovementEffectTimeline,
        movement_speed: &mut LocalMovementSpeedAuthority,
        world: &impl sim::CollisionWorld,
    ) -> bool {
        movement_ticker.settle_riptide_input(physics);
        movement_effects.begin_frame();
        self.observe_dismount(&frame, movement_ticker.session_generation);
        let PhysicsFrameInput {
            now,
            active,
            movement,
            raw_movement,
            analogue_movement,
            yaw,
            input_mode,
            jump,
            sprint,
            sneak,
            facts,
            ..
        } = frame;
        if active {
            self.input_mode = input_mode;
        }
        let input_mode = self.input_mode;
        let movement = facts.input_locks.movement(movement);
        let sprint_allowed = !facts.input_locks.blocks(ClientInputLock::MoveForward);
        let processed_sneak = ActionPhase {
            pressed: sneak.pressed && !facts.input_locks.blocks(ClientInputLock::Sneak),
            held: sneak.held && !facts.input_locks.blocks(ClientInputLock::Sneak),
            released: sneak.released,
        };
        let head_yaw = wire_head_yaw(yaw);
        let delta = frame.delta.min(MAX_FRAME_ELAPSED);
        let withhold_input = frame.hold.is_some_and(|hold| hold.withhold_input);
        let requested_speed;
        let frame = if let Some(hold) = frame.hold {
            self.reset();
            movement_speed.set_sprinting(false);
            requested_speed = movement_speed.prediction_speed();
            physics.advance_dimension_wait(
                delta,
                yaw,
                PhysicsSampleContext {
                    pitch: frame.pitch,
                    head_yaw,
                    camera_orientation: frame.camera_orientation,
                    input_mode,
                    ..PhysicsSampleContext::default()
                },
                hold.registry,
                movement_effects,
            )
        } else {
            if !active {
                self.controls.suspend(input_mode.persists_sneak());
            }
            let server = physics.take_server_control_flags();
            if let Some(server) = server {
                movement_speed.adopt_server_sprinting(server.sprinting);
                self.controls
                    .adopt_server_flags(server.sprinting, server.sneaking);
            }
            let retain_sprint = physics
                .retains_swim_sprint(world)
                .unwrap_or_else(|_| physics.mode() == sim::MovementMode::Swimming);
            let controlled = self.controls.update(ControlObservation {
                now,
                resolved_movement: frame.resolved_movement,
                forward: movement[1],
                sideways: movement[0],
                touch_input: input_mode == protocol::PlayerInputMode::Touch,
                sprint_pressed: sprint.pressed && sprint_allowed,
                sprint_held: sprint.held && sprint_allowed,
                sneak_pressed: processed_sneak.pressed,
                sneak_held: processed_sneak.held,
                toggle_sprint: frame.toggle_sprint,
                always_sprint: active && frame.always_sprint && sprint_allowed,
                toggle_sneak: frame.toggle_sneak,
                sprint_blocked: facts.sprint_blocked,
                sprint_start_blocked: facts.sprint_start_blocked,
                flying: physics.mode() == sim::MovementMode::Flying,
                retain_sprint,
                hold_sneak: !active && input_mode.persists_sneak(),
            });
            let mut input = physics_movement_input(
                movement,
                yaw,
                active,
                jump.held,
                controlled.sneaking && !facts.input_locks.blocks(ClientInputLock::Sneak),
                controlled.sprint_request,
                frame.item_use_modifier,
            );
            if retain_sprint {
                input.sprinting = controlled.sprint_request;
            }
            if !active {
                input.sneaking =
                    controlled.sneaking && !facts.input_locks.blocks(ClientInputLock::Sneak);
            }
            input.immobile = facts.immobile;
            input.vertical_physics = facts.vertical_physics;
            movement_speed.set_sprinting(input.sprinting);
            input.movement_speed = movement_speed.prediction_speed();
            let liquid = movement_speed.liquid();
            input.underwater_movement_speed = liquid.underwater;
            input.lava_movement_speed = liquid.lava;
            requested_speed = input.movement_speed;
            physics.advance_with_context_and_effects(
                delta,
                input,
                PhysicsSampleContext {
                    pitch: frame.pitch,
                    head_yaw,
                    camera_orientation: frame.camera_orientation,
                    input_mode,
                    raw_move_vector: raw_movement,
                    analogue_move_vector: analogue_movement,
                    mode_intent: ModeIntent {
                        spectator: facts.spectator,
                        no_clip: facts.no_clip,
                        ride: facts.ride,
                        ride_seat: facts.ride_seat,
                        can_fly: facts.can_fly,
                        flight: facts.flight,
                        ability_sequence: facts.ability_sequence,
                        fly_speed: facts.fly_speed,
                        vertical_fly_speed: facts.vertical_fly_speed,
                        creative_flight: facts.creative_flight,
                        elytra_ready: facts.elytra_ready,
                        can_stand_on_snow: facts.can_stand_on_snow,
                        depth_strider: facts.depth_strider,
                        soul_speed: facts.soul_speed,
                        swift_sneak: facts.swift_sneak,
                        swim_hunger_blocked: facts.swim_hunger_blocked,
                        sprint_blocked: facts.sprint_blocked,
                        sprint_start_blocked: facts.sprint_start_blocked,
                        stop_sprinting: controlled.stop_sprinting,
                    },
                    release_use_modifier: movement_ticker.release_use_modifier(),
                    input_locks: facts.input_locks,
                    input: super::TickInput {
                        movement_buttons: filtered_movement_buttons(
                            frame.movement_buttons,
                            facts.input_locks,
                        ),
                        jump,
                        sneak: processed_sneak,
                        sprint_down: controlled.sprint_down,
                        sneak_down: input.sneaking,
                        server_sprinting: server.and_then(|flags| flags.sprinting),
                        server_sneaking: server.and_then(|flags| flags.sneaking),
                        server_mode: None,
                        server_spinning: None,
                    },
                },
                world,
                movement_effects,
            )
        };
        if let Some(sample) = frame.samples.last() {
            self.controls
                .adopt_tick_sprinting(sample.processed.sprinting);
            movement_speed.set_sprinting(sample.processed.sprinting);
        }
        super::control_trace::trace_physics_frame(
            movement_ticker.session_generation,
            now,
            requested_speed,
            movement_speed.current(),
            &frame,
        );
        let blocker = frame.blocked.as_ref().map(ToString::to_string);
        if frame.dropped_ticks != 0 {
            // Time starvation keeps the retained samples contiguous and monotonic,
            // so the outbound stream remains a valid 20 Hz sequence and the server
            // can still reconcile it. Record the stall instead of permanently
            // revoking authority over a load symptom; a live Venity session died
            // exactly here when join-time streaming stalls dropped seven of
            // fifteen due ticks.
            debug!(
                due = frame.due_ticks,
                dropped = frame.dropped_ticks,
                "local physics dropped excess catch-up ticks"
            );
        }
        let authority_fault = physics_authority_fault_for_frame(&frame);
        if blocker != self.previous_blocker {
            if authority_fault.is_none()
                && let Some(blocker) = blocker.as_deref()
            {
                debug!(%blocker, "local physics is waiting for collision data or a bounded query");
            }
            self.previous_blocker = blocker;
        }
        if let Some(fault) = authority_fault
            && movement_ticker.physics_is_authorized()
        {
            movement_ticker.record_physics_fault(fault);
            physics.deactivate();
            return false;
        }
        for sample in frame.samples {
            let admitted = if withhold_input {
                movement_ticker.withhold_respawn_input(sample)
            } else {
                movement_ticker.enqueue_completed_physics_at(sample, Some(now))
            };
            if let Err(fault) = admitted {
                debug!(?fault, "local Physics movement authority failed closed");
                physics.deactivate();
                return false;
            }
        }
        true
    }

    /// Retains riding input even when transport admission pauses physics, preserving server links.
    pub fn observe_dismount(&mut self, frame: &PhysicsFrameInput, session: u64) {
        // A boat maps both jump and sneak to dismount; other mounts keep jump for themselves.
        // A press counts through whichever pressed button its locks still allow.
        let boat = frame.facts.ride == Some(super::RideKind::Boat);
        let allowed = |lock| !frame.facts.input_locks.blocks(lock);
        let trigger = [
            (boat && frame.jump.pressed, ClientInputLock::Jump),
            (frame.sneak.pressed, ClientInputLock::Sneak),
        ]
        .into_iter()
        .find(|(pressed, lock)| *pressed && allowed(*lock))
        .map(|(_, lock)| lock);
        let mount = frame
            .facts
            .ride
            .and(frame.facts.mount_runtime_id)
            .filter(|mount| *mount != 0)
            .filter(|_| frame.active && !frame.input_authority_released && frame.hold.is_none())
            .map(|mount| (session, mount));
        // A retained press survives later frames on the same mount, whichever button made it.
        if self
            .pending_dismount
            .map(|(session, mount, _)| (session, mount))
            != mount
        {
            self.pending_dismount = None;
        }
        if let (Some(trigger), Some((session, mount))) = (trigger, mount)
            && allowed(ClientInputLock::Dismount)
        {
            self.pending_dismount = Some((session, mount, trigger));
        }
    }
}

/// Filters digital directions independently of raw and analogue device vectors.
fn filtered_movement_buttons(
    buttons: semantic_input::MovementButtons,
    locks: protocol::ClientInputLocks,
) -> semantic_input::MovementButtons {
    semantic_input::MovementButtons {
        forward: buttons.forward && !locks.blocks(ClientInputLock::MoveForward),
        backward: buttons.backward && !locks.blocks(ClientInputLock::MoveBackward),
        left: buttons.left && !locks.blocks(ClientInputLock::MoveLeft),
        right: buttons.right && !locks.blocks(ClientInputLock::MoveRight),
    }
}
