//! Deterministic Bedrock movement simulation.

mod aabb;
mod conformance;
mod destroy;
mod fluid;
mod math;
mod prediction;
mod simulator;
mod world;

pub use aabb::{Aabb, PLAYER_HEIGHT, PLAYER_WIDTH, depenetrate_player};
pub use conformance::{
    ConformanceError, LegacyTickResult, LegacyTraceRecord, ScenarioAudit, ScenarioEvidence,
    ScenarioScript, ScenarioStep, ScenarioWorld, TraceRecord, audit_scenario_trace_jsonl,
    verify_legacy_trace_jsonl, verify_scenario_trace_jsonl, verify_trace_jsonl,
};
pub use destroy::{BlockDestroyInfo, DestroyConditions, HeldTool, destroy_progress_per_tick};
pub use fluid::sample_actor_liquids;
pub use math::{Vec3, minecraft_cos, minecraft_sin, normalize_direction, view_direction};
pub use prediction::{MotionOverlay, PredictionError, PredictionHistory, ReplayResult};
pub use simulator::{
    AxisCollisions, ControlledTickResult, DEFAULT_MOVEMENT_SPEED, JUMP_DELAY_TICKS,
    MAX_BLOCK_SAMPLES_PER_TICK, MAX_SAFE_LIQUID_VELOCITY, MovementEffects, MovementEnvironment,
    MovementInput, MovementMode, NORMAL_GRAVITY, PlayerState, ProcessedControls, RiptideLaunch,
    SPRINT_SPEED_MULTIPLIER, SimulationError, Simulator, TickResult, VerticalPhysics,
    descends_through_block, mover_collision_boxes_with_provenance, pose_fits,
    sample_actor_liquid_submersion, sample_liquid_submersion, sample_water_head,
};

pub use world::{
    BLOCK_USE_SUPPORT_DEPTH, BLOCK_USE_SUPPORT_MAX_Y, BlockHit, BlockPhysicsFacts,
    BlockPhysicsFlags, BlockPhysicsSample, CameraBlockHit, CameraShapeCache, CollisionIdSpace,
    CollisionQuery, CollisionRegistry, CollisionRegistryIdentity, CollisionSnapshot,
    CollisionWorld, DEFAULT_SURFACE_FRICTION, DoorFacing, DoorState, FlowBlockFacts,
    LenientCollisionBoxes, LenientSkipCounts, MAX_COLLISION_IDENTITY_CHUNKS,
    MAX_COLLISION_QUERY_EXTENT, MoverColliders, PaletteWorld, ProvenancedCollider, RegistryError,
    SurfaceResponse, TERRAIN_EMPTY_SHAPE, TERRAIN_NO_LAYER, TerrainColumn, TerrainGeometry,
    TerrainSample, TerrainSampleError, TerrainSampler, TerrainShape, TerrainShapeBinding,
    TerrainShapeOverride, TerrainState, WorldCollisionIdentity, WorldQueryError,
};
