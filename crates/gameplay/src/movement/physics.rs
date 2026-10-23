use std::{collections::VecDeque, time::Duration};

use protocol::{PLAYER_NETWORK_OFFSET, PlayerInputMode};
use thiserror::Error;
use {
    sim::{
        CollisionWorld, MovementInput, PlayerState, PredictionHistory, SimulationError, Simulator,
        Vec3, WorldCollisionIdentity,
    },
    simulation_clock::TICKS_PER_SECOND,
};

mod actor_move;
mod advance;
mod aim_pose;
mod controller_frame;
mod correction;
mod dimension_wait;
mod prediction_corrections;
mod riptide;
mod speed;
mod sprint_retention;
use controller_frame::ControllerFrame;
mod eye;
mod fixed_ticks;
mod input_requests;
mod motion_ticks;
mod posture;
mod timeline;
mod visual_correction;

#[cfg(test)]
mod liquid_probe_tests;

pub use motion_ticks::PhysicsMotionSample;
pub use timeline::ServerControlFlags;

use super::anchor_probe::AnchorProbeState;
use super::locomotion::{ModeIntent, ModeObservation, ModeTracker};
use super::state::{ProcessedMovementState, ReplayJumpArcFold};

const LOCAL_PHYSICS_TICK_SECONDS: f64 = 1.0 / TICKS_PER_SECOND as f64;
/// Retained prediction window until StartGame supplies `RewindHistorySize`.
const LOCAL_PHYSICS_HISTORY_CAPACITY: usize = 32;
/// Vanilla's ceiling on StartGame `RewindHistorySize`.
const MAX_REWIND_HISTORY_SIZE: u16 = 1000;

/// Vanilla's per-frame tick cap; excess whole ticks are discarded.
pub const MAX_LOCAL_PHYSICS_TICKS_PER_FRAME: usize = simulation_clock::MAX_TICKS_PER_FRAME as usize;

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

/// Keeps missing terrain and bounded-query limits retryable without revoking authority.
pub fn is_retryable_collision_block(error: &SimulationError) -> bool {
    matches!(
        error,
        SimulationError::World(
            sim::WorldQueryError::UnloadedChunk(_)
                | sim::WorldQueryError::UnknownRuntimeId { .. }
                | sim::WorldQueryError::QueryExtentExceeded
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
        item_use_movement_modifier: item_use_movement_modifier.map(item_use_factor),
        movement_speed: None,
        effects: sim::MovementEffects::default(),
        ..MovementInput::default()
    }
}

/// Vanilla slows the move vector by the square of the used item's modifier in
/// f32, skipping modifiers within float epsilon of one.
fn item_use_factor(modifier: f64) -> f64 {
    let modifier = modifier as f32;
    if (modifier - 1.0).abs() > f32::EPSILON {
        f64::from(modifier * modifier)
    } else {
        1.0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PhysicsSampleContext {
    /// Accepted attribute replacement awaiting a completed tick, even if its value is unchanged.
    pub movement_speed_attribute: Option<client_world::MovementSpeedAttribute>,
    /// A captured held-use modifier applied only through its release-facing tick.
    pub release_use_modifier: Option<(u64, f64)>,
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
    /// Current permissions gate new input; committed controls revoke retained requests.
    pub input_locks: protocol::ClientInputLocks,
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
    /// The corrected tick's own end state; replayed samples start after it.
    pub(super) corrected_sample: Option<PhysicsMovementSample>,
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

/// An admitted launch and the completed facing tick that must precede it.
#[derive(Debug, Clone, Copy)]
pub(in crate::movement) struct RiptideRequest {
    launch: sim::RiptideLaunch,
    pub(in crate::movement) after_tick: u64,
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
    /// A snapped server teleport suppresses travel on the next completed tick.
    skip_travel_pending: bool,
    previous_jump_held: bool,
    jump_edge_pending: bool,
    input_edges: super::input_state::PendingInputEdges,
    jump_pulse_pending: bool,
    /// Synthetic jump state from the previous completed tick.
    jump_pulse_held: bool,
    jump_pulse_scope: Option<(u64, i32)>,
    knockback_sequence: u64,
    /// Open processed-jump-arc fold state carried across ticks. Reset with the
    /// rest of prediction state; rebuilt across correction replays.
    processed_jump_arc_active: bool,
    dropped_tick_count: u64,
    last_world_identity: Option<WorldCollisionIdentity>,
    sample_history: VecDeque<PhysicsMovementSample>,
    controller_history: VecDeque<ControllerFrame>,
    replayed_speed: Option<super::speed_authority::EffectiveMovementSpeed>,
    /// Earliest timeline edit not yet incorporated by a successful replay.
    rewind_pending: Option<u64>,
    motion_ticks: VecDeque<motion_ticks::CompletedMotionTick>,
    motion_anchor: Option<motion_ticks::CompletedMotionTick>,
    /// Server velocity replacements, retained while a replay can still reach them.
    server_motions: VecDeque<sim::MotionOverlay>,
    actor_position_pending: Option<actor_move::ActorPositionUpdate>,
    deferred_corrections: prediction_corrections::DeferredPredictionCorrections,
    history_capacity: usize,
    /// Server sprint/sneak states awaiting adoption by the control latches.
    server_control_flags: Option<ServerControlFlags>,
    /// Accepted server posture newer than the latest completed sample.
    live_posture: ServerControlFlags,
    /// One admitted release waiting for a successful movement tick.
    riptide_pending: Option<RiptideRequest>,
    boosts: super::boosts::BoostTimeline,
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
            skip_travel_pending: false,
            previous_jump_held: false,
            jump_edge_pending: false,
            input_edges: Default::default(),
            jump_pulse_pending: false,
            jump_pulse_held: false,
            jump_pulse_scope: None,
            knockback_sequence: 0,
            processed_jump_arc_active: false,
            dropped_tick_count: 0,
            last_world_identity: None,
            sample_history: VecDeque::with_capacity(LOCAL_PHYSICS_HISTORY_CAPACITY),
            controller_history: VecDeque::with_capacity(LOCAL_PHYSICS_HISTORY_CAPACITY),
            replayed_speed: None,
            rewind_pending: None,
            motion_ticks: VecDeque::with_capacity(MAX_LOCAL_PHYSICS_TICKS_PER_FRAME),
            motion_anchor: None,
            server_motions: VecDeque::new(),
            actor_position_pending: None,
            deferred_corrections: Default::default(),
            history_capacity: LOCAL_PHYSICS_HISTORY_CAPACITY,
            server_control_flags: None,
            live_posture: Default::default(),
            riptide_pending: None,
            boosts: Default::default(),
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
        self.skip_travel_pending = false;
        self.previous_jump_held = false;
        self.jump_edge_pending = false;
        self.input_edges = Default::default();
        self.jump_pulse_pending = false;
        self.jump_pulse_held = false;
        self.jump_pulse_scope = None;
        self.processed_jump_arc_active = false;
        self.last_world_identity = None;
        self.sample_history.clear();
        self.replayed_speed = None;
        self.rewind_pending = None;
        self.controller_history.clear();
        self.motion_ticks.clear();
        self.motion_anchor = self
            .state
            .as_ref()
            .map(motion_ticks::CompletedMotionTick::anchor);
        self.server_motions.clear();
        self.actor_position_pending = None;
        self.deferred_corrections = Default::default();
        self.server_control_flags = None;
        self.live_posture = Default::default();
        self.riptide_pending = None;
        self.boosts.clear();
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
        self.skip_travel_pending = false;
        self.previous_jump_held = false;
        self.jump_edge_pending = false;
        self.input_edges = Default::default();
        self.jump_pulse_pending = false;
        self.jump_pulse_held = false;
        self.jump_pulse_scope = None;
        self.processed_jump_arc_active = false;
        self.dropped_tick_count = 0;
        self.last_world_identity = None;
        self.sample_history.clear();
        self.replayed_speed = None;
        self.rewind_pending = None;
        self.controller_history.clear();
        self.motion_ticks.clear();
        self.motion_anchor = self
            .state
            .as_ref()
            .map(motion_ticks::CompletedMotionTick::anchor);
        self.server_motions.clear();
        self.actor_position_pending = None;
        self.deferred_corrections = Default::default();
        self.modes.reset();
        self.live_posture = Default::default();
        self.riptide_pending = None;
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

    /// Fixed ticks a frame of `elapsed` would complete, without advancing the clock.
    pub fn ticks_due(&self, elapsed: Duration) -> u64 {
        if self.state.is_none() || self.discard_next_elapsed {
            return 0;
        }
        fixed_ticks::whole_ticks(self.accumulated_seconds + elapsed.as_secs_f64())
    }

    /// Advances only when a finite, simulable server motion enters live prediction.
    pub const fn knockback_sequence(&self) -> u64 {
        self.knockback_sequence
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

    /// Consecutive completed gliding ticks, counting the tick the glide started.
    pub const fn fall_fly_ticks(&self) -> u32 {
        self.modes.fall_fly_ticks()
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
