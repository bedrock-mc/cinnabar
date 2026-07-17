use std::time::Duration;

use assets::{CollisionConfidence, NetworkIdMode, RegistryRecord};
use bevy::prelude::Resource;
use protocol::{PLAYER_NETWORK_OFFSET, STANDING_PLAYER_EYE_HEIGHT};
use sim::{
    Aabb, CollisionRegistry, CollisionWorld, MovementInput, PlayerState, PredictionHistory,
    RegistryError, SimulationError, Simulator, TICKS_PER_SECOND, Vec3,
};

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
}

impl PhysicsCollisionRegistries {
    pub fn from_records(records: &[RegistryRecord]) -> Result<Self, RegistryError> {
        let mut sequential = CollisionRegistry::new();
        let mut hashed = CollisionRegistry::new();
        let mut available_record_count = 0;
        for record in records {
            if record.collision_seed.confidence == CollisionConfidence::None {
                continue;
            }
            let boxes = record
                .collision_seed
                .boxes
                .iter()
                .copied()
                .map(collision_box_to_aabb)
                .collect::<Vec<_>>();
            sequential.register(record.sequential_id, boxes.iter().copied())?;
            hashed.register(record.network_hash, boxes)?;
            available_record_count += 1;
        }
        Ok(Self {
            sequential,
            hashed,
            available_record_count,
            sequential_count: available_record_count,
            hashed_count: available_record_count,
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

#[derive(Debug, Default)]
pub struct LocalPhysicsFrame {
    pub completed_ticks: usize,
    pub dropped_ticks: u64,
    pub blocked: Option<SimulationError>,
}

/// Locally predicted fixed-tick player state and render interpolation.
///
/// This resource never owns a network sender or changes [`super::MovementTicker`]
/// authority. It is therefore impossible for a local/free-camera prediction to
/// become a `PlayerAuthInput` merely by advancing this controller.
#[derive(Resource, Debug)]
pub struct LocalPhysicsController {
    simulator: Simulator,
    history: PredictionHistory,
    state: Option<PlayerState>,
    previous_position: Vec3,
    accumulated_seconds: f64,
    previous_jump_held: bool,
    jump_edge_pending: bool,
    dropped_tick_count: u64,
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
            previous_jump_held: false,
            jump_edge_pending: false,
            dropped_tick_count: 0,
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
        self.previous_jump_held = false;
        self.jump_edge_pending = false;
        self.history = PredictionHistory::new(LOCAL_PHYSICS_HISTORY_CAPACITY)
            .expect("local physics history capacity is non-zero");
    }

    /// Replaces prediction state from a server network-position anchor.
    ///
    /// Bedrock player movement positions carry the protocol network offset;
    /// collision simulation uses the feet origin. Non-finite anchors disable
    /// local prediction instead of allowing invalid state to reach collision.
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
        self.previous_jump_held = false;
        self.jump_edge_pending = false;
        self.dropped_tick_count = 0;
        self.history = PredictionHistory::new(LOCAL_PHYSICS_HISTORY_CAPACITY)
            .expect("local physics history capacity is non-zero");
    }

    pub fn advance(
        &mut self,
        elapsed: Duration,
        mut input: MovementInput,
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

        self.accumulated_seconds += elapsed.as_secs_f64();
        let due = ((self.accumulated_seconds + f64::EPSILON) / LOCAL_PHYSICS_TICK_SECONDS)
            .floor()
            .clamp(0.0, u64::MAX as f64) as u64;
        self.accumulated_seconds -= due as f64 * LOCAL_PHYSICS_TICK_SECONDS;
        let allowed = due.min(MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64) as usize;
        let mut frame = LocalPhysicsFrame {
            dropped_ticks: due.saturating_sub(allowed as u64),
            ..LocalPhysicsFrame::default()
        };

        for tick_index in 0..allowed {
            let before = state.position;
            match self.history.predict(state, input, &self.simulator, world) {
                Ok(_) => {
                    self.previous_position = before;
                    frame.completed_ticks += 1;
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

    #[must_use]
    pub const fn dropped_tick_count(&self) -> u64 {
        self.dropped_tick_count
    }
}
