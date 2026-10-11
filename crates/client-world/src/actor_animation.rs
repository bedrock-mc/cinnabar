use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
};

use assets::{
    EntityAnimationInterpolation, EntityAnimationLoop, EntityAssetKind, EntityRigFallback,
    MolangOp, RuntimeEntityAssets,
};
use protocol::{ActorKind, ActorMetadataValue};

/// Simulation tick duration used by actor clocks and Molang time queries.
use world::TICK_DURATION as ACTOR_TICK_DURATION;

pub const MAX_RUNTIME_BONES_PER_RIG: usize = assets::MAX_ENTITY_GEOMETRY_BONES;
const ANIMATION_TICK_SECONDS: f32 = ACTOR_TICK_DURATION.as_secs_f32();
pub const MAX_CONTROLLER_TRANSITIONS_PER_TICK: usize = 8;
pub const MAX_MOLANG_OPS_PER_ACTOR_TICK: usize = 4_096;
pub const MAX_MOLANG_OPS_PER_WORLD_TICK: usize = 262_144;
/// World-wide ceiling for authored render-time layer expressions; pose histories stay tick-owned.
pub const MAX_MOLANG_OPS_PER_RENDER_FRAME: usize = MAX_MOLANG_OPS_PER_WORLD_TICK;
pub const MAX_ACTOR_ACTION_HISTORY: usize = 32;
const MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK: usize = 4_096;
const MAX_RUNTIME_BINDINGS_PER_RIG: usize = 4_096;

mod actor;
pub use actor::{AnimatedActor, AnimatedActors, MAX_ANIMATED_ACTORS};
mod properties;
pub use properties::{PropertyDefinition, PropertyKind};
mod rig;
pub use rig::{
    ActorAnimationStats, ActorLifetimeId, ActorRigSnapshot, BoneTransform, EntityRigId, HandPhase,
    ItemAnimationState, LocalSwingProgress,
};
mod state;
use state::{
    ActorRigState, ActorTickInput, AttachableRootFrame, ControllerState, EvalBudget, EvalError,
    EvaluatedState, RuntimeBone,
};
mod store;
pub use store::ActorAnimationStore;
use store::{PackCatalog, PoseStep, model_scale};

mod attachable;
mod clock;
pub mod custom_emotes;
mod evaluation;
mod geometry;
mod horse;
mod hud;
mod java;
mod spear;
pub use java::retarget::JavaRetargetCache;
pub use java::{JavaHeldItem, JavaMotion, java_mounted_body_yaw, java_walked_distance};
mod motion;
mod particles;
mod pose;
pub use particles::ActorParticleController;
mod query;
mod render;
mod render_frame;
pub use render_frame::{ActorRenderFrame, ActorRenderLayers};
mod replay;
mod server_animation;
pub use server_animation::ServerAnimationCompiler;
mod schedule;
mod skin;
mod skin_layers;
mod skin_preparation;
mod tick;
mod view;
pub use attachable::{
    AttachableAnimationInput, AttachableBoneParent, AttachableRigSnapshot, AttachablesRuntime,
};
pub use evaluation::ActorAnimationVariables;
use evaluation::{EngineSlots, Evaluator, MolangValue, MolangVariables, VariableLayout};
use geometry::{collect_controllers, resolve_binding, resolve_bones, skeleton};
pub use motion::ACTOR_SWING_TICKS;
use motion::{MotionInput, MotionState};
pub use pose::MODEL_PART_ORIGIN_Y;
use pose::{compose_pose, sample_clips};
pub use render::RenderTextureLayer;
pub use skin_layers::SkinRenderLayer;
pub use tick::{ActorTickContext, WornArmor};
use tick::{advance_motion, evaluate_state};
pub use view::ActorAnimationView;

mod local_motion;
pub use local_motion::LocalSwingMotionSample;

#[cfg(test)]
mod test_actor;
#[cfg(test)]
pub mod test_support;
#[cfg(test)]
use test_actor::TestActor;

#[cfg(test)]
mod local_motion_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "actor_animation/crystal_tests.rs"]
mod crystal_tests;

#[cfg(test)]
mod dragon_tests;
