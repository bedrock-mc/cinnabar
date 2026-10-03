mod action;
mod actor_animation;
mod actor_store;
mod block_entity_visuals;
pub mod game_mode_capabilities;
pub mod ingestion;
mod item;
mod local_player_facts;
pub mod server_position;

pub use local_player_facts::{LocalPlayerFacts, LocalPlayerStat};

pub use action::{
    ActorEventIdentity, ActorSourceTick, MAX_ACTION_EVENTS_PER_TICK, MAX_ACTIONS_PER_ACTOR,
    RemoteActionFallback, RemoteActionSnapshot, RemoteActionStats,
};
pub use actor_animation::{
    ACTOR_SWING_TICKS, ACTOR_TICK_DURATION, ActorAnimationStats, ActorAnimationVariables,
    ActorAnimationView, ActorLifetimeId, ActorRigSnapshot, AttachableAnimationInput,
    AttachableRigSnapshot, AttachablesRuntime, BoneTransform, EntityRigId, HandPhase,
    ItemAnimationState, MAX_ACTOR_ACTION_HISTORY, MAX_CONTROLLER_TRANSITIONS_PER_TICK,
    MAX_MOLANG_OPS_PER_ACTOR_TICK, MAX_MOLANG_OPS_PER_RENDER_FRAME, MAX_MOLANG_OPS_PER_WORLD_TICK,
    MAX_RUNTIME_BONES_PER_RIG, MODEL_PART_ORIGIN_Y, RenderTextureLayer, SkinRenderLayer,
};
pub use actor_store::{
    ActorPickup, ActorPose, ActorSnapshot, ActorStatus, ActorStatusNotice, BlockEntityKind,
    BlockEntityView, DEATH_DURATION_TICKS, DroppedItemView, HURT_DURATION_TICKS,
    HURT_OVERLAY_ALPHA, LightningBoltView, LocalItemUse, LocalPlayerFeed, MAX_DROPPED_ITEM_COPIES,
    MAX_STATUS_NOTICES, MovementFlagUpdate, PICKUP_DURATION_TICKS, PlayerProfile, PropertyDefault,
    RideSeat, RopeKind, RopeView, SeatDefaults, SeatRequirement, dropped_item_copy_count,
    tnt_presentation,
};
pub use block_entity_visuals::{
    BackingBlockIdentity, BlockEntityVisualDiagnostics, BlockEntityVisualRoute,
    adjudicate_block_entity_visual,
};
pub use item::{
    ActorArmorPiece, ActorArmorSnapshot, ActorEquipmentSnapshot, CanonicalItemRegistryRecord,
    CanonicalItemStack, EquipmentNotice, EquipmentOutcome, MAX_EQUIPMENT_NOTICES,
    MAX_ITEM_REGISTRY_RECORDS, MAX_PENDING_ITEM_RESOLUTIONS,
};
pub use server_position::{ResolvedServerPosition, SAFE_SERVER_HEIGHT};

mod authority;
pub use authority::BiomeCommitReport;
pub use authority::{
    BlockEventCue, COMMITTED_AUDIO_CAPACITY, COMMITTED_CAMERA_CAPACITY, COMMITTED_CONTROL_CAPACITY,
    COMMITTED_PARTICLE_CAPACITY, COMMITTED_UI_CAPACITY, CommittedAudioEvent, CommittedCameraEvent,
    CommittedControlEvent, CommittedParticleEvent, CommittedUiEvent, MapImage,
    PublisherViewGeometry, SignEditRequest, ViewCohort, WorldAuthority,
};
