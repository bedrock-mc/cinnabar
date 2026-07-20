use std::{collections::VecDeque, time::Duration};

use assets::{NetworkIdMode, RegistryRecord};
use bevy::prelude::Resource;
use protocol::{PLAYER_NETWORK_OFFSET, PlayerInputMode, STANDING_PLAYER_EYE_HEIGHT};
use sim::{
    Aabb, CollisionIdSpace, CollisionRegistry, CollisionRegistryIdentity, CollisionWorld,
    MovementInput, PlayerState, PredictionHistory, RegistryError, SimulationError, Simulator,
    TICKS_PER_SECOND, Vec3, WorldCollisionIdentity,
};
use thiserror::Error;

const COLLISION_COORDINATE_SCALE: f64 = 1.0 / 100_000_000.0;
const LOCAL_PHYSICS_TICK_SECONDS: f64 = 1.0 / TICKS_PER_SECOND as f64;
const LOCAL_PHYSICS_HISTORY_CAPACITY: usize = 32;

/// Maximum fixed simulation ticks allowed in one render frame.
///
/// Longer stalls discard excess whole ticks instead of creating an unbounded
/// catch-up spike. Outbound movement remains independently disabled.
pub const MAX_LOCAL_PHYSICS_TICKS_PER_FRAME: usize = 8;

/// Converts app right/forward axes into bedsim's left-positive strafe input.
#[must_use]
pub fn physics_movement_input(
    right_forward: [f32; 2],
    yaw_degrees: f32,
    active: bool,
    jumping: bool,
    sneaking: bool,
    sprinting: bool,
) -> MovementInput {
    if !active {
        return MovementInput::default();
    }
    MovementInput {
        strafe: -f64::from(right_forward[0]),
        forward: f64::from(right_forward[1]),
        yaw_degrees: f64::from(yaw_degrees),
        jumping,
        jump_pressed: false,
        sprinting,
        sneaking,
    }
}

/// Runtime-ID collision registries for both Bedrock palette identity modes.
///
/// The two maps are intentionally distinct: a 32-bit network hash may have
/// the same numeric value as an unrelated sequential ID.
#[derive(Resource, Debug)]
pub struct PhysicsCollisionRegistries {
    sequential: CollisionRegistry,
    hashed: CollisionRegistry,
    available_record_count: usize,
    sequential_count: usize,
    hashed_count: usize,
    preg_sha256: [u8; 32],
    breg_sha256: [u8; 32],
}

#[derive(Debug, Error)]
pub enum PhysicsCollisionRegistryError {
    #[error(transparent)]
    Asset(#[from] assets::AssetError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
}

impl PhysicsCollisionRegistries {
    pub fn from_assets(
        breg_bytes: &[u8],
        records: &[RegistryRecord],
        preg_bytes: &[u8],
    ) -> Result<Self, PhysicsCollisionRegistryError> {
        let physics = assets::read_physics_registry(preg_bytes, breg_bytes, records)?;
        let sequential_identity = CollisionRegistryIdentity {
            protocol: 1001,
            id_space: CollisionIdSpace::Sequential,
            preg_sha256: physics.sha256(),
        };
        let hashed_identity = CollisionRegistryIdentity {
            id_space: CollisionIdSpace::Hashed,
            ..sequential_identity
        };
        let mut sequential = CollisionRegistry::with_identity(sequential_identity);
        let mut hashed = CollisionRegistry::with_identity(hashed_identity);
        for record in records {
            let fact = physics
                .by_sequential_id(record.sequential_id)
                .expect("strict PREG decoder covers every supplied BREG record");
            let boxes = fact
                .boxes
                .iter()
                .copied()
                .map(collision_box_to_aabb)
                .collect::<Vec<_>>();
            let register = |registry: &mut CollisionRegistry, runtime_id, boxes: Vec<Aabb>| {
                registry.register_primitives(
                    runtime_id,
                    boxes,
                    f64::from(fact.friction_q1e8) * COLLISION_COORDINATE_SCALE,
                    f64::from(fact.horizontal_speed_q1e8) * COLLISION_COORDINATE_SCALE,
                    f64::from(fact.vertical_speed_q1e8) * COLLISION_COORDINATE_SCALE,
                    f64::from(fact.fluid_height_q1e8) * COLLISION_COORDINATE_SCALE,
                    fact.flags.bits(),
                    fact.surface_response as u8,
                )
            };
            register(&mut sequential, record.sequential_id, boxes.clone())?;
            register(&mut hashed, record.network_hash, boxes)?;
            if record.name.as_ref() == "minecraft:air" {
                sequential.set_air_runtime_id(record.sequential_id);
                hashed.set_air_runtime_id(record.network_hash);
            }
        }
        let available_record_count = physics.len();
        let preg_sha256 = physics.sha256();
        let breg_sha256 = physics.breg_sha256();
        Ok(Self {
            sequential,
            hashed,
            available_record_count,
            sequential_count: physics.len(),
            hashed_count: physics.len(),
            preg_sha256,
            breg_sha256,
        })
    }

    #[must_use]
    pub const fn registry(&self, mode: NetworkIdMode) -> &CollisionRegistry {
        match mode {
            NetworkIdMode::Sequential => &self.sequential,
            NetworkIdMode::Hashed => &self.hashed,
        }
    }

    #[must_use]
    pub const fn registered_count(&self, mode: NetworkIdMode) -> usize {
        match mode {
            NetworkIdMode::Sequential => self.sequential_count,
            NetworkIdMode::Hashed => self.hashed_count,
        }
    }

    #[must_use]
    pub const fn available_record_count(&self) -> usize {
        self.available_record_count
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.available_record_count != 0
            && self.sequential_count == self.available_record_count
            && self.hashed_count == self.available_record_count
            && self.preg_sha256 != [0; 32]
            && self.breg_sha256 != [0; 32]
    }

    #[must_use]
    pub const fn preg_sha256(&self) -> [u8; 32] {
        self.preg_sha256
    }

    #[must_use]
    pub const fn breg_sha256(&self) -> [u8; 32] {
        self.breg_sha256
    }
}

fn collision_box_to_aabb(collision: assets::CollisionBox) -> Aabb {
    let coordinate = |value: i32| f64::from(value) * COLLISION_COORDINATE_SCALE;
    Aabb::new(
        Vec3::new(
            coordinate(collision.min_x),
            coordinate(collision.min_y),
            coordinate(collision.min_z),
        ),
        Vec3::new(
            coordinate(collision.max_x),
            coordinate(collision.max_y),
            coordinate(collision.max_z),
        ),
    )
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PhysicsSampleContext {
    pub pitch: f32,
    pub head_yaw: f32,
    pub camera_orientation: [f32; 3],
    pub input_mode: PlayerInputMode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsMovementSample {
    pub tick: u64,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub move_vector: [f32; 2],
    pub pitch: f32,
    pub yaw: f32,
    pub head_yaw: f32,
    pub camera_orientation: [f32; 3],
    pub jumping: bool,
    pub sneaking: bool,
    pub sprinting: bool,
    pub input_mode: PlayerInputMode,
    pub grounded_before_tick: bool,
    pub grounded_after_tick: bool,
    pub jump_repeated: bool,
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
    pub(super) corrected_position: [f32; 3],
    pub(super) final_tick: u64,
    pub(super) final_position: [f32; 3],
    pub(super) replayed_samples: Vec<PhysicsMovementSample>,
}

#[derive(Debug, Default)]
pub struct LocalPhysicsFrame {
    pub completed_ticks: usize,
    pub dropped_ticks: u64,
    pub blocked: Option<SimulationError>,
    pub samples: Vec<PhysicsMovementSample>,
}

/// Locally predicted fixed-tick player state and render interpolation.
///
/// This resource never owns a network sender or changes [`super::MovementTicker`]
/// authority. It is therefore impossible for a local/free-camera prediction to
/// become a `PlayerAuthInput` merely by advancing this controller.
#[derive(Resource, Debug, Clone)]
pub struct LocalPhysicsController {
    simulator: Simulator,
    history: PredictionHistory,
    state: Option<PlayerState>,
    previous_position: Vec3,
    accumulated_seconds: f64,
    discard_next_elapsed: bool,
    previous_jump_held: bool,
    jump_edge_pending: bool,
    dropped_tick_count: u64,
    last_world_identity: Option<WorldCollisionIdentity>,
    sample_history: VecDeque<PhysicsMovementSample>,
}

impl Default for LocalPhysicsController {
    fn default() -> Self {
        Self {
            simulator: Simulator::default(),
            history: PredictionHistory::new(LOCAL_PHYSICS_HISTORY_CAPACITY)
                .expect("local physics history capacity is non-zero"),
            state: None,
            previous_position: Vec3::ZERO,
            accumulated_seconds: 0.0,
            discard_next_elapsed: false,
            previous_jump_held: false,
            jump_edge_pending: false,
            dropped_tick_count: 0,
            last_world_identity: None,
            sample_history: VecDeque::with_capacity(LOCAL_PHYSICS_HISTORY_CAPACITY),
        }
    }
}

impl LocalPhysicsController {
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.state.is_some()
    }

    pub fn deactivate(&mut self) {
        self.state = None;
        self.accumulated_seconds = 0.0;
        self.discard_next_elapsed = false;
        self.previous_jump_held = false;
        self.jump_edge_pending = false;
        self.last_world_identity = None;
        self.sample_history.clear();
        self.history = PredictionHistory::new(LOCAL_PHYSICS_HISTORY_CAPACITY)
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
        self.accumulated_seconds = 0.0;
        self.discard_next_elapsed = false;
        self.previous_jump_held = false;
        self.jump_edge_pending = false;
        self.dropped_tick_count = 0;
        self.last_world_identity = None;
        self.sample_history.clear();
        self.history = PredictionHistory::new(LOCAL_PHYSICS_HISTORY_CAPACITY)
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
        mut input: MovementInput,
        context: PhysicsSampleContext,
        world: &impl CollisionWorld,
    ) -> LocalPhysicsFrame {
        let Some(state) = self.state.as_mut() else {
            return LocalPhysicsFrame::default();
        };
        if input.jumping && !self.previous_jump_held {
            self.jump_edge_pending = true;
        }
        self.previous_jump_held = input.jumping;
        input.jump_pressed = self.jump_edge_pending;

        if self.discard_next_elapsed {
            self.discard_next_elapsed = false;
            return LocalPhysicsFrame::default();
        }

        self.accumulated_seconds += elapsed.as_secs_f64();
        let due = ((self.accumulated_seconds + f64::EPSILON) / LOCAL_PHYSICS_TICK_SECONDS)
            .floor()
            .clamp(0.0, u64::MAX as f64) as u64;
        self.accumulated_seconds -= due as f64 * LOCAL_PHYSICS_TICK_SECONDS;
        let allowed = due.min(MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64) as usize;
        let mut frame = LocalPhysicsFrame {
            dropped_ticks: due.saturating_sub(allowed as u64),
            samples: Vec::with_capacity(allowed),
            ..LocalPhysicsFrame::default()
        };

        for tick_index in 0..allowed {
            // Bedrock auto-jump semantics treat a held jump as a fresh request
            // once the player is grounded again. Preserve the render-frame edge
            // latch for taps shorter than one fixed tick, but never inject the
            // repeated edge while airborne or during the jump-delay window.
            let grounded_before_tick = state.on_ground;
            let jump_repeated = input.jumping
                && grounded_before_tick
                && state.jump_delay == 0
                && !self.jump_edge_pending;
            input.jump_pressed = self.jump_edge_pending || jump_repeated;
            let before = state.position;
            match self.history.predict(state, input, &self.simulator, world) {
                Ok(result) => {
                    self.previous_position = before;
                    let world_identity = result.world_identity;
                    self.last_world_identity = Some(world_identity.clone());
                    frame.completed_ticks += 1;
                    frame.samples.push(PhysicsMovementSample {
                        tick: state.tick,
                        position: [
                            state.position.x as f32,
                            state.position.y as f32 + PLAYER_NETWORK_OFFSET,
                            state.position.z as f32,
                        ],
                        velocity: [
                            state.velocity.x as f32,
                            state.velocity.y as f32,
                            state.velocity.z as f32,
                        ],
                        move_vector: [-input.strafe as f32, input.forward as f32],
                        pitch: context.pitch,
                        yaw: input.yaw_degrees as f32,
                        head_yaw: context.head_yaw,
                        camera_orientation: context.camera_orientation,
                        jumping: input.jumping,
                        sneaking: input.sneaking,
                        sprinting: input.sprinting,
                        input_mode: context.input_mode,
                        grounded_before_tick,
                        grounded_after_tick: state.on_ground,
                        jump_repeated,
                        world_identity,
                    });
                    if self.sample_history.len() == LOCAL_PHYSICS_HISTORY_CAPACITY {
                        self.sample_history.pop_front();
                    }
                    self.sample_history.push_back(
                        frame
                            .samples
                            .last()
                            .expect("completed tick appended a movement sample")
                            .clone(),
                    );
                    self.jump_edge_pending = false;
                    input.jump_pressed = false;
                }
                Err(error) => {
                    self.previous_position = state.position;
                    self.accumulated_seconds = 0.0;
                    frame.dropped_ticks = frame
                        .dropped_ticks
                        .saturating_add((allowed - tick_index) as u64);
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

    pub(super) fn apply_correction(
        &mut self,
        network_position: [f32; 3],
        tick: u64,
        on_ground: bool,
        mode: PhysicsCorrectionMode,
        world: &impl CollisionWorld,
    ) -> Result<PhysicsCorrectionPlan, PhysicsCorrectionError> {
        if !network_position.into_iter().all(f32::is_finite) {
            return Err(PhysicsCorrectionError::InvalidAnchor);
        }
        if matches!(mode, PhysicsCorrectionMode::Snap) {
            self.reanchor_network_position_before_advance(network_position, tick, on_ground);
            return Ok(PhysicsCorrectionPlan {
                outcome: PhysicsCorrectionOutcome::Snapped { tick },
                corrected_tick: tick,
                corrected_position: network_position,
                final_tick: tick,
                final_position: network_position,
                replayed_samples: Vec::new(),
            });
        }

        if self.state.is_none() {
            return Err(PhysicsCorrectionError::NotRetained { tick });
        }

        let current_tick = self
            .state
            .as_ref()
            .expect("active correction checked for local state")
            .tick;
        if tick > current_tick {
            return Err(PhysicsCorrectionError::NotRetained { tick });
        }
        if self.history.state_at(tick).is_none()
            || !self.sample_history.iter().any(|sample| sample.tick == tick)
        {
            return Err(PhysicsCorrectionError::NotRetained { tick });
        }

        let feet = Vec3::new(
            f64::from(network_position[0]),
            f64::from(network_position[1] - PLAYER_NETWORK_OFFSET),
            f64::from(network_position[2]),
        );
        let mut corrected = PlayerState::new(feet);
        corrected.tick = tick;
        corrected.on_ground = on_ground;
        // Axis collisions describe the motion that produced a position, so they
        // cannot be recomputed from a corrected anchor. They are retained only
        // when the server echoes back the exact network position this client
        // sent for that tick: the motion behind it is then confirmed too, and a
        // legitimate wall climb must not stutter on every confirming correction.
        // Any other correction repudiates that motion, so the flags are cleared
        // and the discrete climb branch stays closed. The comparison is exact
        // and happens in the sent `f32` network space rather than against the
        // `f64` feet state, because only the former is what the server actually
        // acknowledged. The resulting loss is bounded to the first replayed
        // tick: `Simulator::tick` re-derives collisions for every tick after it.
        let server_confirmed_prediction = self
            .sample_history
            .iter()
            .any(|sample| sample.tick == tick && sample.position == network_position);
        corrected.collisions = if server_confirmed_prediction {
            self.history
                .state_at(tick)
                .map_or_else(sim::AxisCollisions::default, |retained| retained.collisions)
        } else {
            sim::AxisCollisions::default()
        };
        let (replay, replayed_ticks) = self
            .history
            .rewind_and_replay_traced(
                self.state
                    .as_mut()
                    .expect("active correction checked for local state"),
                corrected,
                &self.simulator,
                world,
            )
            .map_err(|_| PhysicsCorrectionError::ReplayFailed)?;

        if replayed_ticks.len() != replay.replayed_ticks {
            return Err(PhysicsCorrectionError::ReplayFailed);
        }
        let mut replayed_samples = Vec::with_capacity(replayed_ticks.len());
        for result in replayed_ticks {
            let Some(retained) = self
                .sample_history
                .iter_mut()
                .find(|sample| sample.tick == result.tick)
            else {
                return Err(PhysicsCorrectionError::NotRetained { tick: result.tick });
            };
            if retained.world_identity != result.world_identity {
                return Err(PhysicsCorrectionError::WorldIdentityMismatch { tick: result.tick });
            }
            retained.position = [
                result.position.x as f32,
                result.position.y as f32 + PLAYER_NETWORK_OFFSET,
                result.position.z as f32,
            ];
            replayed_samples.push(retained.clone());
        }
        let corrected_world_identity = {
            let corrected_sample = self
                .sample_history
                .iter_mut()
                .find(|sample| sample.tick == tick)
                .expect("retained correction sample was checked");
            corrected_sample.position = network_position;
            corrected_sample.world_identity.clone()
        };

        let state = self
            .state
            .as_ref()
            .expect("successful replay retains local state");
        let final_tick = state.tick;
        let final_position = [
            state.position.x as f32,
            state.position.y as f32 + PLAYER_NETWORK_OFFSET,
            state.position.z as f32,
        ];
        self.previous_position = if final_tick == tick {
            feet
        } else {
            self.history
                .state_at(final_tick.saturating_sub(1))
                .map_or(feet, |previous| previous.position)
        };
        self.accumulated_seconds = 0.0;
        self.last_world_identity = replayed_samples
            .last()
            .map(|sample| sample.world_identity.clone())
            .or(Some(corrected_world_identity));

        Ok(PhysicsCorrectionPlan {
            outcome: PhysicsCorrectionOutcome::Replayed {
                corrected_tick: replay.corrected_tick,
                replayed_ticks: replay.replayed_ticks,
            },
            corrected_tick: tick,
            corrected_position: network_position,
            final_tick,
            final_position,
            replayed_samples,
        })
    }

    #[must_use]
    pub fn render_eye_position(&self) -> Option<[f32; 3]> {
        let state = self.state.as_ref()?;
        let alpha = (self.accumulated_seconds / LOCAL_PHYSICS_TICK_SECONDS).clamp(0.0, 1.0);
        let feet = self.previous_position + (state.position - self.previous_position) * alpha;
        Some([
            feet.x as f32,
            feet.y as f32 + STANDING_PLAYER_EYE_HEIGHT,
            feet.z as f32,
        ])
    }

    #[must_use]
    pub const fn state(&self) -> Option<&PlayerState> {
        self.state.as_ref()
    }

    #[must_use]
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Retained axis collisions at a specific predicted tick, for asserting the
    /// correction/replay contract without exposing history to production code.
    #[cfg(test)]
    #[must_use]
    pub(super) fn retained_collisions_at(&self, tick: u64) -> Option<sim::AxisCollisions> {
        self.history.state_at(tick).map(|state| state.collisions)
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
