//! Local movement frame coordination before ordered interaction admission.
use super::control_modes::{ControlModes, ControlObservation};
use super::local_facts::LocalMovementFacts;
use super::physics_authority_fault_for_frame;
use super::{
    LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController, ModeIntent,
    MovementTicker, PhysicsSampleContext, physics_movement_input,
};
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
const MAX_FRAME_ELAPSED: Duration = Duration::from_millis(100);

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
}

impl LocomotionState {
    /// Clears input latches when local prediction is inactive.
    pub fn reset(&mut self) {
        self.controls.reset();
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
        movement_effects.begin_frame();
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
            if let Some(server) = physics.take_server_control_flags() {
                movement_speed.adopt_server_sprinting(server.sprinting);
                self.controls
                    .adopt_server_flags(server.sprinting, server.sneaking);
            }
            let retain_sprint = physics
                .retains_swim_sprint(world)
                .unwrap_or_else(|_| physics.mode() == sim::MovementMode::Swimming);
            let controlled = self.controls.update(ControlObservation {
                now,
                forward: movement[1],
                sideways: movement[0],
                touch_input: input_mode == protocol::PlayerInputMode::Touch,
                sprint_pressed: sprint.pressed,
                sprint_held: sprint.held,
                sneak_pressed: sneak.pressed,
                sneak_held: sneak.held,
                toggle_sprint: frame.toggle_sprint,
                always_sprint: active && frame.always_sprint,
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
                controlled.sneaking,
                controlled.sprint_request,
                frame.item_use_modifier,
            );
            if retain_sprint {
                input.sprinting = controlled.sprint_request;
            }
            if !active {
                input.sneaking = controlled.sneaking;
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
                        ride: facts.ride,
                        ride_seat: facts.ride_seat,
                        can_fly: facts.can_fly,
                        server_flying: facts.server_flying,
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
                    input: super::TickInput {
                        movement_buttons: frame.movement_buttons,
                        jump,
                        sneak,
                        sprint_down: controlled.sprint_down,
                        sneak_down: controlled.sneaking,
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
                debug!(%blocker, "local physics is waiting for authoritative collision data");
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
                movement_ticker.enqueue_completed_physics(sample)
            };
            if let Err(fault) = admitted {
                debug!(?fault, "local Physics movement authority failed closed");
                physics.deactivate();
                return false;
            }
        }
        true
    }
}
