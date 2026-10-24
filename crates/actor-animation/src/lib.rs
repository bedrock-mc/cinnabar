//! Actor rigs, Molang evaluation, controllers and poses, driven by actor authority through
//! the [`AnimatedActor`] seam. Bevy-free.
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
use simulation_clock::TICK_DURATION as ACTOR_TICK_DURATION;

pub const MAX_RUNTIME_BONES_PER_RIG: usize = assets::MAX_ENTITY_GEOMETRY_BONES;
const ANIMATION_TICK_SECONDS: f32 = ACTOR_TICK_DURATION.as_secs_f32();
pub const MAX_CONTROLLER_TRANSITIONS_PER_TICK: usize = 8;
pub const MAX_MOLANG_OPS_PER_ACTOR_TICK: usize = 4_096;
pub const MAX_MOLANG_OPS_PER_WORLD_TICK: usize = 262_144;
/// World-wide ceiling for authored render-time layer expressions; pose histories stay tick-owned.
pub const MAX_MOLANG_OPS_PER_RENDER_FRAME: usize = MAX_MOLANG_OPS_PER_WORLD_TICK;
pub const MAX_ACTOR_ACTION_HISTORY: usize = 32;
const MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK: usize = 4_096;
/// A frame sample replays pre-animation for both tick endpoints besides its own and render pass.
const FRAME_PASSES_PER_ACTOR: usize = 4;
/// One actor's frame sample fits any rig whose tick evaluation fits its tick budget.
const MAX_MOLANG_OPS_PER_ACTOR_FRAME: usize =
    FRAME_PASSES_PER_ACTOR * MAX_MOLANG_OPS_PER_ACTOR_TICK;
const MAX_RUNTIME_POSE_WORK_PER_ACTOR_FRAME: usize =
    FRAME_PASSES_PER_ACTOR * MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK;
const MAX_RUNTIME_BINDINGS_PER_RIG: usize = 4_096;

mod actor;
pub use actor::{AnimatedActor, AnimatedActors, MAX_ANIMATED_ACTORS};
pub mod consumables;
mod properties;
mod rig_plans;
pub use properties::{PropertyDefinition, PropertyKind};
mod rig;
pub use rig::{
    ActorAnimationStats, ActorLifetimeId, ActorRigSnapshot, BoneTransform, EntityRigId, HandPhase,
    ItemAnimationState, LocalSwingProgress,
};
mod state;
use state::{
    ActorRigState, ActorTickInput, AttachableRootFrame, ControllerState, EvalBudget, EvalError,
    EvaluatedState, OperandStack, RuntimeBone,
};
mod store;
pub use store::ActorAnimationStore;
use store::{PackCatalog, PoseStep, model_scale};

mod attachable;
mod boat;
mod cape;
pub use cape::CapePreparationQueue;
mod clock;
pub mod custom_emotes;
pub use custom_emotes::{CustomEmotePose, sample_custom_emote};
mod evaluation;
mod geometry;
mod horse;
mod hud;
mod java;
mod legacy_swing;
mod mount;
mod spear;
pub use java::retarget::JavaRetargetCache;
pub use java::{JavaHeldItem, JavaMotion, java_mounted_body_yaw, java_walked_distance};
mod motion;
mod particles;
mod pose;
pub use particles::{ActorParticleController, ActorParticleQueries};
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
use evaluation::variables::MolangVariables;
use evaluation::{EngineSlots, Evaluator, MolangValue, VariableLayout};
use geometry::{collect_controllers, resolve_binding, resolve_bones, skeleton};
pub use motion::{ACTOR_SWING_TICKS, SwingEffects};
use motion::{MotionInput, MotionState};
pub use pose::MODEL_PART_ORIGIN_Y;
use pose::compose_pose;
pub use render::RenderTextureLayer;
pub use skin_layers::SkinRenderLayer;
pub use tick::{ActorTickContext, StackVariant, WornArmor};
use tick::{advance_motion, evaluate_state};
pub use view::ActorAnimationView;

mod local_motion;
pub use local_motion::LocalSwingMotionSample;

#[cfg(test)]
mod test_actor;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
#[cfg(test)]
use test_actor::TestActor;

#[cfg(test)]
mod local_motion_tests;

#[cfg(test)]
mod remote_motion_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod crystal_tests;

#[cfg(test)]
mod dragon_tests;

#[cfg(test)]
#[global_allocator]
static TEST_ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;
