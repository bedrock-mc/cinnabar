use std::{collections::VecDeque, time::Duration};

use protocol::{PLAYER_NETWORK_OFFSET, PlayerInputMode};
use sim::{
    CollisionWorld, MovementInput, PlayerState, PredictionHistory, SimulationError, Simulator,
    TICKS_PER_SECOND, Vec3, WorldCollisionIdentity,
};
use thiserror::Error;

mod aim_pose;
mod controller_frame;
mod correction;
mod dimension_wait;
mod prediction_corrections;
mod sprint_retention;
use controller_frame::ControllerFrame;
mod eye;
mod fixed_ticks;
mod motion_ticks;
mod timeline;
mod visual_correction;

pub use motion_ticks::PhysicsMotionSample;
pub use timeline::ServerControlFlags;

use super::anchor_probe::BeforeTick;
use super::locomotion::{ModeIntent, ModeObservation, ModeTracker};
use super::state::{ProcessedMovementState, ReplayJumpArcFold};

const LOCAL_PHYSICS_TICK_SECONDS: f64 = 1.0 / TICKS_PER_SECOND as f64;
/// Retained prediction window until StartGame supplies `RewindHistorySize`.
const LOCAL_PHYSICS_HISTORY_CAPACITY: usize = 32;
/// Vanilla's ceiling on StartGame `RewindHistorySize`.
const MAX_REWIND_HISTORY_SIZE: u16 = 1000;

/// Maximum fixed simulation ticks allowed in one render frame.
///
/// Longer stalls discard excess whole ticks instead of creating an unbounded
/// catch-up spike. Outbound movement remains independently disabled.
pub const MAX_LOCAL_PHYSICS_TICKS_PER_FRAME: usize = 8;

pub trait MovementEffectSource {
    fn snapshot(&self) -> sim::MovementEffects;
    fn commit_successful_tick(&mut self);
}

struct NoMovementEffects;

impl MovementEffectSource for NoMovementEffects {
    fn snapshot(&self) -> sim::MovementEffects {
        sim::MovementEffects::default()
    }

    fn commit_successful_tick(&mut self) {}
}

pub fn is_transient_collision_unavailability(error: &SimulationError) -> bool {
    matches!(
        error,
        SimulationError::World(
            sim::WorldQueryError::UnloadedChunk(_) | sim::WorldQueryError::UnknownRuntimeId { .. }
        )
    )
}

/// Converts app right/forward axes into bedsim's left-positive strafe input.
///
/// Actor sprint requests require forward movement; physical packet buttons
/// remain independent in the accompanying sample context.
#[must_use]
pub fn physics_movement_input(
    right_forward: [f32; 2],
    yaw_degrees: f32,
    active: bool,
    jumping: bool,
    sneaking: bool,
    sprint_request: bool,
    item_use_movement_modifier: Option<f64>,
) -> MovementInput {
    if !active {
        return MovementInput {
            yaw_degrees: f64::from(yaw_degrees),
            ..MovementInput::default()
        };
    }
    let sprinting = sprint_request && right_forward[1] > 0.0;
    MovementInput {
        strafe: -f64::from(right_forward[0]),
        forward: f64::from(right_forward[1]),
        yaw_degrees: f64::from(yaw_degrees),
        jumping,
        jump_pressed: false,
        sprinting,
        sneaking,
        move_vector_is_raw: true,
        using_consumable: false,
        item_use_movement_modifier,
        movement_speed: None,
        effects: sim::MovementEffects::default(),
        ..MovementInput::default()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PhysicsSampleContext {
    pub pitch: f32,
    pub head_yaw: f32,
    pub camera_orientation: [f32; 3],
    pub input_mode: PlayerInputMode,
    /// Pre-normalization controlling-device movement sample.
    pub raw_move_vector: [f32; 2],
    /// Analog-axis sample of the controlling device.
    pub analogue_move_vector: [f32; 2],
    pub mode_intent: ModeIntent,
    pub input: super::TickInput,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsMovementSample {
    pub tick: u64,
    pub position: [f32; 3],
    /// This tick's resolved displacement, used by local movement evidence.
    pub movement: [f32; 3],
    /// End-of-tick velocity sent as PlayerAuthInput.PosDelta.
    pub velocity: [f32; 3],
    pub move_vector: [f32; 2],
    /// Pre-normalization device sample used for the digital raw-input fallback.
    pub raw_move_vector: [f32; 2],
    /// Analog-axis sample carried to PlayerAuthInput analog input.
    pub analogue_move_vector: [f32; 2],
    pub pitch: f32,
    pub yaw: f32,
    pub head_yaw: f32,
    pub camera_orientation: [f32; 3],
    pub jumping: bool,
    pub sneaking: bool,
    pub input: super::TickInput,
    pub sprinting: bool,
    pub input_mode: PlayerInputMode,
    pub grounded_before_tick: bool,
    pub grounded_after_tick: bool,
    pub horizontal_collision: bool,
    pub vertical_collision: bool,
    pub jump_repeated: bool,
    /// Processed movement states this tick (VPA-011): what the simulator
    /// acted on, as opposed to which buttons are held. The outbound
    /// `PlayerAuthInput` processed flag families derive from this snapshot.
    pub processed: ProcessedMovementState,
    pub world_identity: WorldCollisionIdentity,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PhysicsCorrectionConfirmation {
    pub position: [f32; 3],
    pub world_identity: WorldCollisionIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicsCorrectionMode {
    ReplayIfRetained,
    Snap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicsCorrectionOutcome {
    Replayed {
        corrected_tick: u64,
        replayed_ticks: usize,
    },
    Snapped {
        tick: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(super) enum PhysicsCorrectionError {
    #[error("correction anchor is not finite")]
    InvalidAnchor,
    #[error("correction tick {tick} is not retained")]
    NotRetained { tick: u64 },
    #[error("correction replay failed")]
    ReplayFailed,
    #[error("correction replay tick {tick} changed immutable collision identity")]
    WorldIdentityMismatch { tick: u64 },
}

#[derive(Debug, Clone)]
pub(super) struct PhysicsCorrectionPlan {
    pub(super) outcome: PhysicsCorrectionOutcome,
    pub(super) corrected_tick: u64,
    pub(super) final_tick: u64,
    pub(super) final_position: [f32; 3],
    pub(super) anchor_input: super::encoding::HeldInput,
    pub(super) replayed_samples: Vec<PhysicsMovementSample>,
}

#[derive(Debug, Default)]
pub struct LocalPhysicsFrame {
    pub due_ticks: u64,
    pub completed_ticks: usize,
    pub dropped_ticks: u64,
    pub blocked_tick_index: Option<usize>,
    pub blocked: Option<SimulationError>,
    pub samples: Vec<PhysicsMovementSample>,
}

/// Locally predicted fixed-tick player state and render interpolation.
///
/// This resource never owns a network sender or changes [`super::MovementTicker`]
/// authority. It is therefore impossible for a local/free-camera prediction to
/// become a `PlayerAuthInput` merely by advancing this controller.
#[derive(Debug, Clone)]
pub struct LocalPhysicsController {
    simulator: Simulator,
    history: PredictionHistory,
    state: Option<PlayerState>,
    previous_position: Vec3,
    eye_offset: eye::LocalEyeOffset,
    visual_correction: visual_correction::VisualCorrection,
    accumulated_seconds: f64,
    discard_next_elapsed: bool,
    previous_jump_held: bool,
    jump_edge_pending: bool,
    input_edges: super::input_state::PendingInputEdges,
    fly_toggle_pending: bool,
    /// Open processed-jump-arc fold state carried across ticks. Reset with the
    /// rest of prediction state; rebuilt across correction replays.
    processed_jump_arc_active: bool,
    dropped_tick_count: u64,
    last_world_identity: Option<WorldCollisionIdentity>,
    sample_history: VecDeque<PhysicsMovementSample>,
    controller_history: VecDeque<ControllerFrame>,
    motion_ticks: VecDeque<motion_ticks::CompletedMotionTick>,
    motion_anchor: Option<motion_ticks::CompletedMotionTick>,
    /// Server velocity replacements, retained while a replay can still reach them.
    server_motions: VecDeque<sim::MotionOverlay>,
    deferred_corrections: prediction_corrections::DeferredPredictionCorrections,
    history_capacity: usize,
    /// Server sprint/sneak states awaiting adoption by the control latches.
    server_control_flags: Option<ServerControlFlags>,
    /// Bounded spawn-anchor depenetration state (provisional recovery
    /// policy): the pending probe, per-epoch failure budget, and any frozen
    /// embedded-anchor hold.
    anchor_state: super::anchor_probe::AnchorProbeState,
    /// Locomotion mode selector and the previous tick's sampled environment it reads.
    modes: ModeTracker,
    last_environment: sim::MovementEnvironment,
    dimension_waiting: bool,
    pub(super) prediction_sync: super::prediction_sync::PredictionSyncCountdown,
}

impl Default for LocalPhysicsController {
    fn default() -> Self {
        Self {
            simulator: Simulator::default(),
            history: PredictionHistory::new(LOCAL_PHYSICS_HISTORY_CAPACITY)
                .expect("local physics history capacity is non-zero"),
            state: None,
            previous_position: Vec3::ZERO,
            eye_offset: eye::LocalEyeOffset::default(),
            visual_correction: visual_correction::VisualCorrection::default(),
            accumulated_seconds: 0.0,
            discard_next_elapsed: false,
            previous_jump_held: false,
            jump_edge_pending: false,
            input_edges: Default::default(),
            fly_toggle_pending: false,
            processed_jump_arc_active: false,
            dropped_tick_count: 0,
            last_world_identity: None,
            sample_history: VecDeque::with_capacity(LOCAL_PHYSICS_HISTORY_CAPACITY),
            controller_history: VecDeque::with_capacity(LOCAL_PHYSICS_HISTORY_CAPACITY),
            motion_ticks: VecDeque::with_capacity(MAX_LOCAL_PHYSICS_TICKS_PER_FRAME),
            motion_anchor: None,
            server_motions: VecDeque::new(),
            deferred_corrections: Default::default(),
            history_capacity: LOCAL_PHYSICS_HISTORY_CAPACITY,
            server_control_flags: None,
            anchor_state: super::anchor_probe::AnchorProbeState::new(),
            modes: ModeTracker::default(),
            last_environment: sim::MovementEnvironment::default(),
            dimension_waiting: false,
            prediction_sync: Default::default(),
        }
    }
}

impl LocalPhysicsController {
    /// Returns the prediction state retained for one authoritative tick.
    pub(super) fn retained_state(&self, tick: u64) -> Option<&PlayerState> {
        self.history.state_at(tick)
    }

    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.state.is_some()
    }

    /// Sizes the retained window from StartGame `RewindHistorySize` as vanilla's
    /// entity initializer does: its low 16 bits, zero as one, capped at 1000.
    /// Takes effect at the next reanchor.
    pub fn set_rewind_history_size(&mut self, size: i32) {
        self.history_capacity = usize::from((size as u16).clamp(1, MAX_REWIND_HISTORY_SIZE));
    }

    #[must_use]
    pub const fn history_capacity(&self) -> usize {
        self.history_capacity
    }

    pub fn deactivate(&mut self) {
        self.prediction_sync.clear();
        self.state = None;
        self.eye_offset = eye::LocalEyeOffset::default();
        self.visual_correction = visual_correction::VisualCorrection::default();
        self.accumulated_seconds = 0.0;
        self.discard_next_elapsed = false;
        self.previous_jump_held = false;
        self.jump_edge_pending = false;
        self.input_edges = Default::default();
        self.fly_toggle_pending = false;
        self.processed_jump_arc_active = false;
        self.last_world_identity = None;
        self.sample_history.clear();
        self.controller_history.clear();
        self.motion_ticks.clear();
        self.motion_anchor = self
            .state
            .as_ref()
            .map(motion_ticks::CompletedMotionTick::anchor);
        self.server_motions.clear();
        self.deferred_corrections = Default::default();
        self.server_control_flags = None;
        self.modes.reset();
        self.last_environment = sim::MovementEnvironment::default();
        self.dimension_waiting = false;
        self.anchor_state.reset();
        self.history = PredictionHistory::new(self.history_capacity)
            .expect("local physics history capacity is non-zero");
    }

    /// Replaces prediction state from a server network-position anchor.
    ///
    /// Bedrock player movement positions carry the protocol network offset;
    /// collision simulation uses the feet origin. Non-finite anchors disable
    /// local prediction instead of allowing invalid state to reach collision.
    ///
    /// This is a hard reset used by StartGame, session/dimension replacement,
    /// teleports, and un-replayable corrections. It deliberately clears the
    /// retained axis collisions along with the rest of the history: no prior
    /// motion survives the reset, so nothing can justify the discrete
    /// ladder-climb branch until a fresh tick re-derives them.
    pub fn reanchor_network_position(
        &mut self,
        network_position: [f32; 3],
        tick: u64,
        on_ground: bool,
    ) {
        if !network_position.into_iter().all(f32::is_finite) {
            self.deactivate();
            return;
        }
        let feet = Vec3::new(
            f64::from(network_position[0]),
            f64::from(network_position[1] - PLAYER_NETWORK_OFFSET),
            f64::from(network_position[2]),
        );
        let mut state = PlayerState::new(feet);
        state.tick = tick;
        state.on_ground = on_ground;
        self.state = Some(state);
        self.previous_position = feet;
        self.eye_offset = eye::LocalEyeOffset::default();
        self.visual_correction = visual_correction::VisualCorrection::default();
        self.accumulated_seconds = 0.0;
        self.discard_next_elapsed = false;
        self.previous_jump_held = false;
        self.jump_edge_pending = false;
        self.input_edges = Default::default();
        self.fly_toggle_pending = false;
        self.processed_jump_arc_active = false;
        self.dropped_tick_count = 0;
        self.last_world_identity = None;
        self.sample_history.clear();
        self.controller_history.clear();
        self.motion_ticks.clear();
        self.motion_anchor = self
            .state
            .as_ref()
            .map(motion_ticks::CompletedMotionTick::anchor);
        self.server_motions.clear();
        self.deferred_corrections = Default::default();
        self.modes.reset();
        self.last_environment = sim::MovementEnvironment::default();
        self.dimension_waiting = false;
        // Every hard anchor starts a fresh bounded probe epoch: the new
        // position is probed before its first simulated tick, and any prior
        // failure budget or frozen embedded-anchor hold is replaced.
        self.anchor_state.note_hard_anchor();
        self.history = PredictionHistory::new(self.history_capacity)
            .expect("local physics history capacity is non-zero");
    }

    /// Reanchors prediction while discarding the render-frame delta that
    /// elapsed before the new server anchor was installed.
    ///
    /// Runtime network reconciliation runs before physics in a frame. Applying
    /// that frame's entire delta to a newly installed state would incorrectly
    /// simulate startup, transfer, or correction time after the anchor and can
    /// produce a false fixed-tick overflow. Only the immediately following
    /// advance is discarded; subsequent overload remains observable.
    pub fn reanchor_network_position_before_advance(
        &mut self,
        network_position: [f32; 3],
        tick: u64,
        on_ground: bool,
    ) {
        self.reanchor_network_position(network_position, tick, on_ground);
        self.discard_next_elapsed = self.is_active();
    }

    pub fn advance(
        &mut self,
        elapsed: Duration,
        input: MovementInput,
        world: &impl CollisionWorld,
    ) -> LocalPhysicsFrame {
        self.advance_with_context(elapsed, input, PhysicsSampleContext::default(), world)
    }

    pub fn advance_with_context(
        &mut self,
        elapsed: Duration,
        input: MovementInput,
        context: PhysicsSampleContext,
        world: &impl CollisionWorld,
    ) -> LocalPhysicsFrame {
        self.advance_with_context_and_effects(
            elapsed,
            input,
            context,
            world,
            &mut NoMovementEffects,
        )
    }

    pub fn advance_with_context_and_effects(
        &mut self,
        elapsed: Duration,
        mut input: MovementInput,
        context: PhysicsSampleContext,
        world: &impl CollisionWorld,
        effects: &mut impl MovementEffectSource,
    ) -> LocalPhysicsFrame {
        self.dimension_waiting = false;
        let Some(state) = self.state.as_mut() else {
            return LocalPhysicsFrame::default();
        };
        self.input_edges.observe(context.input);
        if context.input.jump.pressed || (input.jumping && !self.previous_jump_held) {
            self.jump_edge_pending = true;
        }
        self.previous_jump_held = input.jumping;
        input.jump_pressed = self.jump_edge_pending;
        self.fly_toggle_pending ^= context.mode_intent.fly_toggle;

        let mut frame = fixed_ticks::frame(
            elapsed,
            &mut self.accumulated_seconds,
            &mut self.discard_next_elapsed,
        );
        let allowed = frame
            .due_ticks
            .min(MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64) as usize;

        let sprint_request = input.sprinting;
        let requested_movement_speed = input.movement_speed;
        let sneak_request = input.sneaking;
        input.pitch_degrees = f64::from(context.pitch);
        input.fly_speed = context.mode_intent.fly_speed;
        input.vertical_fly_speed = context.mode_intent.vertical_fly_speed;
        input.creative_flight = context.mode_intent.creative_flight;
        input.depth_strider = context.mode_intent.depth_strider;
        input.soul_speed = context.mode_intent.soul_speed;
        input.swift_sneak = context.mode_intent.swift_sneak;
        for tick_index in 0..allowed {
            // Before the first simulated tick of a freshly anchored epoch,
            // probe the anchor out of any solid overlap (provisional
            // recovery policy; see `anchor_probe`).
            if tick_index == 0 && !input.immobile && self.modes.mode() == sim::MovementMode::Riding
            {
                // A rider's position is its seat, not a body to depenetrate.
                self.anchor_state.reset();
            } else if tick_index == 0 && !input.immobile {
                match self.anchor_state.before_tick(
                    world,
                    state.position,
                    self.modes.contact_height(),
                ) {
                    BeforeTick::Adjust(clear_feet) => state.position = clear_feet,
                    BeforeTick::Proceed => {}
                }
            }
            // Bedrock auto-jump semantics treat a held jump as a fresh request
            // once the player is grounded again. Preserve the render-frame edge
            // latch for taps shorter than one fixed tick, but never inject the
            // repeated edge while airborne or during the jump-delay window.
            let grounded_before_tick = state.on_ground;
            let jump_repeated = !input.immobile
                && input.jumping
                && grounded_before_tick
                && state.jump_delay == 0
                && !self.jump_edge_pending;
            input.jump_pressed = self.jump_edge_pending || jump_repeated;
            input.effects = effects.snapshot();
            input.sprinting = sprint_request;
            input.movement_speed = requested_movement_speed;
            input.liquid_contact_height = Some(self.modes.contact_height());
            input.liquid_flow_enabled = Some(self.modes.mode() != sim::MovementMode::Flying);
            let mut forced_sneak = false;
            let mut mode_error = None;
            let previous_modes = self.modes;
            let [move_sideways, move_forward] = ModeObservation::input_vector(input);
            match self.modes.select(
                context.mode_intent,
                self.fly_toggle_pending,
                ModeObservation {
                    feet: state.position,
                    on_ground: state.on_ground,
                    velocity_y: state.velocity.y,
                    in_water: self.last_environment.in_water,
                    in_lava: self.last_environment.in_lava,
                    sprinting: sprint_request,
                    sprint_blinded: input.effects.blindness,
                    sprint_down: context.input.sprint_down,
                    input_mode: context.input_mode,
                    requested_movement: state.requested_movement,
                    move_sideways,
                    move_forward,
                    sneaking: sneak_request,
                    pitch: context.pitch,
                    yaw: input.yaw_degrees as f32,
                    liquid_attach_height: self.eye_offset.height(1.0),
                    jumping: input.jumping,
                    jump_edge: self.jump_edge_pending,
                },
                world,
            ) {
                Ok(choice) => {
                    input.mode = choice.mode;
                    input.sprinting = choice.sprinting;
                    input.sneaking = sneak_request || choice.forced_sneak;
                    forced_sneak = choice.forced_sneak;
                }
                Err(_) if input.immobile => {
                    // Missing terrain cannot turn an authoritative freeze into a blocked tick.
                    self.modes = previous_modes;
                    input.mode = self.modes.mode();
                    input.sneaking = sneak_request;
                }
                Err(error) => mode_error = Some(error),
            }
            self.modes.record_controls(input, context.input.sneak_down);
            // A rider's position is its seat on the mount, not a simulated result.
            let mut ride_delta = None;
            if input.mode == sim::MovementMode::Riding
                && let Some(seat) = context
                    .mode_intent
                    .ride_seat
                    .filter(|seat| seat.iter().all(|axis| axis.is_finite()))
            {
                let seat = Vec3::new(f64::from(seat[0]), f64::from(seat[1]), f64::from(seat[2]));
                ride_delta = Some([
                    (seat.x - state.position.x) as f32,
                    (seat.y - state.position.y) as f32,
                    (seat.z - state.position.z) as f32,
                ]);
                state.position = seat;
            }
            // A queued server impulse replaces this tick's starting velocity,
            // mirroring how Bedrock applies knockback as an absolute velocity.
            // The overlay is retained after application so a correction
            // rewind covering its tick re-applies it deterministically;
            // capacity bounds evict the oldest entries.
            let next_tick = state.tick.saturating_add(1);
            for overlay in self
                .server_motions
                .iter()
                .filter(|overlay| overlay.tick == next_tick)
            {
                state.velocity = overlay.velocity;
            }
            let before = state.position;
            // Native attach 7 uses the prior tick's pose-adjusted eye anchor.
            // Retain the height with this input so correction replay samples
            // the same material cell instead of the rendered interpolation.
            input.liquid_attach_height = Some(f64::from(self.eye_offset.height(1.0)));
            let entry_velocity = [
                state.velocity.x as f32,
                state.velocity.y as f32,
                state.velocity.z as f32,
            ];
            let predicted = match mode_error {
                Some(error) => Err(sim::PredictionError::Simulation(SimulationError::World(
                    error,
                ))),
                None => self
                    .history
                    .predict_with_controls(state, input, &self.simulator, world),
            };
            match predicted {
                Ok(output) => {
                    let result = output.tick_result;
                    self.last_environment = result.environment;
                    while self.controller_history.len() >= self.history_capacity {
                        self.controller_history.pop_front();
                    }
                    self.eye_offset.tick(input.mode, input.sneaking);
                    self.controller_history.push_back(ControllerFrame {
                        tick: state.tick,
                        entry_velocity,
                        eye_height: self.eye_offset.height(1.0),
                        intent: context.mode_intent,
                        jump_edge: self.jump_edge_pending,
                        fly_toggle: self.fly_toggle_pending,
                        requested_sneak: sneak_request,
                        requested_sprint: sprint_request,
                        sprint_down: context.input.sprint_down,
                        input_mode: context.input_mode,
                        mode_override: None,
                        sneak_override: None,
                        sprint_override: None,
                        forced_sneak,
                        grounded_before_tick,
                        jump_repeated,
                        ride_delta,
                        input,
                        modes: self.modes,
                        environment: result.environment,
                    });
                    effects.commit_successful_tick();
                    self.previous_position = before;
                    self.visual_correction.tick();
                    let world_identity = result.world_identity;
                    self.last_world_identity = Some(world_identity.clone());
                    frame.completed_ticks += 1;
                    self.prediction_sync.tick();
                    // The simulator owns initiation; held inputs alone cannot prove a jump.
                    let mut processed = ProcessedMovementState::next(
                        self.processed_jump_arc_active,
                        output.jump_initiated,
                        state.on_ground,
                        input.sneaking,
                        input.sprinting,
                    );
                    processed.mode = input.mode;
                    processed.ride = context.mode_intent.ride;
                    processed.forced_sneak = forced_sneak;
                    if input.immobile || input.mode == sim::MovementMode::Riding {
                        // Frozen travel and mount-owned jumping cannot continue a local jump arc.
                        processed.jump_initiated = false;
                        processed.jump_arc_active = false;
                    }
                    self.processed_jump_arc_active = processed.jump_arc_active;
                    frame.samples.push(PhysicsMovementSample {
                        tick: state.tick,
                        position: [
                            state.position.x as f32,
                            state.position.y as f32 + PLAYER_NETWORK_OFFSET,
                            state.position.z as f32,
                        ],
                        movement: [
                            result.movement.x as f32,
                            result.movement.y as f32,
                            result.movement.z as f32,
                        ],
                        velocity: [
                            result.velocity.x as f32,
                            result.velocity.y as f32,
                            result.velocity.z as f32,
                        ],
                        move_vector: [
                            -output.controls.move_vector[0] as f32,
                            output.controls.move_vector[1] as f32,
                        ],
                        raw_move_vector: context.raw_move_vector,
                        analogue_move_vector: context.analogue_move_vector,
                        pitch: context.pitch,
                        yaw: input.yaw_degrees as f32,
                        head_yaw: context.head_yaw,
                        camera_orientation: context.camera_orientation,
                        jumping: input.jumping,
                        sneaking: input.sneaking,
                        input: self.input_edges.sample(context.input),
                        sprinting: input.sprinting,
                        input_mode: context.input_mode,
                        grounded_before_tick,
                        grounded_after_tick: state.on_ground,
                        horizontal_collision: result.collisions.x || result.collisions.z,
                        vertical_collision: result.collisions.y,
                        jump_repeated,
                        processed,
                        world_identity,
                    });
                    if let (Some(delta), Some(sample)) = (ride_delta, frame.samples.last_mut()) {
                        sample.movement = delta;
                    }
                    while self.sample_history.len() >= self.history_capacity {
                        self.sample_history.pop_front();
                    }
                    self.sample_history.push_back(
                        frame
                            .samples
                            .last()
                            .expect("completed tick appended a movement sample")
                            .clone(),
                    );
                    motion_ticks::retain(
                        &mut self.motion_ticks,
                        &mut self.motion_anchor,
                        self.sample_history
                            .back()
                            .expect("completed sample retained"),
                        self.controller_history
                            .back()
                            .expect("completed controller retained"),
                    );
                    self.jump_edge_pending = false;
                    self.input_edges = Default::default();
                    self.fly_toggle_pending = false;
                    input.jump_pressed = false;
                }
                Err(error) => {
                    self.modes = previous_modes;
                    let transient_collision_blocked = matches!(
                        &error,
                        sim::PredictionError::Simulation(error)
                            if is_transient_collision_unavailability(error)
                    );
                    if transient_collision_blocked {
                        // Prediction is transactional: the failed tick did
                        // not mutate state or history. Discard all elapsed
                        // time in this blocked frame instead of retaining a
                        // retry backlog that could become a false overflow
                        // or a large catch-up burst when collision data
                        // returns. New elapsed time starts a fresh tick.
                        self.previous_position = state.position;
                        self.accumulated_seconds = 0.0;
                        frame.dropped_ticks = 0;
                    } else {
                        // Keep a failed non-transient tick pending. The
                        // existing overflow count remains authoritative for
                        // genuine overload with usable collision data.
                        let unconsumed_ticks = allowed - tick_index;
                        self.accumulated_seconds +=
                            unconsumed_ticks as f64 * LOCAL_PHYSICS_TICK_SECONDS;
                    }
                    frame.blocked_tick_index = Some(tick_index);
                    frame.blocked = Some(match error {
                        sim::PredictionError::Simulation(error) => error,
                        sim::PredictionError::ZeroCapacity
                        | sim::PredictionError::StateHistoryDiverged { .. }
                        | sim::PredictionError::CorrectionNotRetained { .. } => {
                            unreachable!(
                                "local prediction uses a fresh non-zero sequential history"
                            )
                        }
                    });
                    break;
                }
            }
        }
        self.dropped_tick_count = self.dropped_tick_count.saturating_add(frame.dropped_ticks);
        frame
    }

    #[must_use]
    pub fn render_eye_position(&self) -> Option<[f32; 3]> {
        let mut feet = self.render_feet_position()?;
        feet[1] += self.eye_offset.height(self.tick_alpha());
        Some(feet)
    }

    /// How far the frame sits between the last two completed ticks, `0..=1`.
    #[must_use]
    pub fn tick_alpha(&self) -> f32 {
        (self.accumulated_seconds / LOCAL_PHYSICS_TICK_SECONDS).clamp(0.0, 1.0) as f32
    }

    /// The interpolated actor origin, independent of the camera's stance offset.
    #[must_use]
    pub fn render_feet_position(&self) -> Option<[f32; 3]> {
        let state = self.state.as_ref()?;
        let alpha = (self.accumulated_seconds / LOCAL_PHYSICS_TICK_SECONDS).clamp(0.0, 1.0);
        let feet = self.previous_position
            + (state.position - self.previous_position) * alpha
            + self.visual_correction.offset(alpha as f32);
        Some([feet.x as f32, feet.y as f32, feet.z as f32])
    }

    #[must_use]
    pub const fn state(&self) -> Option<&PlayerState> {
        self.state.as_ref()
    }

    /// Processed `(sneaking, sprinting)` of the latest completed tick.
    #[must_use]
    pub fn latest_sneak_sprint(&self) -> Option<(bool, bool)> {
        let sample = self.sample_history.back()?;
        Some((sample.processed.sneaking, sample.processed.sprinting))
    }

    #[must_use]
    /// Whether the last completed simulation tick intersected water.
    pub const fn in_water(&self) -> bool {
        self.last_environment.in_water
    }

    pub const fn mode(&self) -> sim::MovementMode {
        self.modes.mode()
    }

    /// The retained completed-tick sample for `tick`, for diagnostics.
    pub fn sample_at(&self, tick: u64) -> Option<&PhysicsMovementSample> {
        self.sample_history
            .iter()
            .find(|sample| sample.tick == tick)
    }

    #[must_use]
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    #[must_use]
    pub const fn dropped_tick_count(&self) -> u64 {
        self.dropped_tick_count
    }

    #[must_use]
    pub fn network_position(&self) -> Option<[f32; 3]> {
        let state = self.state.as_ref()?;
        Some([
            state.position.x as f32,
            state.position.y as f32 + PLAYER_NETWORK_OFFSET,
            state.position.z as f32,
        ])
    }

    #[must_use]
    pub const fn last_world_identity(&self) -> Option<&WorldCollisionIdentity> {
        self.last_world_identity.as_ref()
    }
}
