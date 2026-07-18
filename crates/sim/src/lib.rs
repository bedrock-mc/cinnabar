//! Deterministic Bedrock movement simulation.

mod aabb;
mod conformance;
mod math;
mod prediction;
mod simulator;
mod world;

pub use aabb::{Aabb, PLAYER_HEIGHT, PLAYER_HORIZONTAL_EPSILON, PLAYER_WIDTH};
pub use conformance::{
    ConformanceError, ScenarioExpectedError, ScenarioTraceRecord, ScenarioWorld, TraceRecord,
    verify_scenario_trace_jsonl, verify_trace_jsonl,
};
pub use math::Vec3;
pub use prediction::{PredictionError, PredictionHistory, ReplayResult};
pub use simulator::{
    AxisCollisions, MAX_BLOCK_SAMPLES_PER_TICK, MovementEnvironment, MovementInput, PlayerState,
    SimulationError, Simulator, TICKS_PER_SECOND, TickResult,
};
pub use world::{
    BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionIdSpace, CollisionQuery,
    CollisionRegistry, CollisionRegistryIdentity, CollisionWorld, MAX_COLLISION_IDENTITY_CHUNKS,
    MAX_COLLISION_QUERY_EXTENT, PaletteWorld, RegistryError, SurfaceResponse,
    WorldCollisionIdentity, WorldQueryError,
};
