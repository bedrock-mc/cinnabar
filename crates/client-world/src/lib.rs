mod action;
mod actor_animation;
mod actor_store;
mod block_entity_visuals;
mod culling;
mod item;
mod server_position;
mod stream;

pub use culling::CaveVisibilityScratch;
pub use protocol::{
    CLASSIC_SKIN_SIDE, MAX_SKIN_ANIMATION_LAYERS, MAX_STANDARD_SKIN_SIDE, expand_legacy_skin_rgba8,
};

pub use action::{
    ActorEventIdentity, ActorSourceTick, MAX_ACTION_EVENTS_PER_TICK, MAX_ACTIONS_PER_ACTOR,
    RemoteActionFallback, RemoteActionSnapshot, RemoteActionStats,
};
pub use actor_animation::{
    ACTOR_SWING_TICKS, ACTOR_TICK_DURATION, ActorAnimationStats, ActorAnimationView,
    ActorLifetimeId, ActorRigSnapshot, BoneTransform, EntityRigId, HandPhase,
    MAX_ACTOR_ACTION_HISTORY, MAX_CONTROLLER_TRANSITIONS_PER_TICK, MAX_MOLANG_OPS_PER_ACTOR_TICK,
    MAX_MOLANG_OPS_PER_RENDER_FRAME, MAX_MOLANG_OPS_PER_WORLD_TICK, MAX_RUNTIME_BONES_PER_RIG,
    RenderTextureLayer, SkinRenderLayer,
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
    BackingBlockIdentity, BlockEntityVisualRoute, adjudicate_block_entity_visual,
};
pub use item::{
    ActorArmorPiece, ActorArmorSnapshot, ActorEquipmentSnapshot, CanonicalItemRegistryRecord,
    CanonicalItemStack, EquipmentNotice, EquipmentOutcome, MAX_EQUIPMENT_NOTICES,
    MAX_ITEM_REGISTRY_RECORDS, MAX_PENDING_ITEM_RESOLUTIONS,
};
pub use server_position::{ResolvedServerPosition, SAFE_SERVER_HEIGHT};
pub use stream::{
    ActiveBlockCrack, BlockCrackSnapshot, BlockCrackStatus, BlockEventCue, BuildProfileIdentity,
    COMMITTED_AUDIO_CAPACITY, COMMITTED_CAMERA_CAPACITY, COMMITTED_CONTROL_CAPACITY,
    CohortManifestIdentity, CommittedAudioEvent, CommittedCameraEvent, CommittedControlEvent,
    CommittedParticleEvent, CommittedUiEvent, DECODE_DISPATCH_BUDGET_PER_POLL,
    DEFERRED_RETRY_CAPACITY, ForcedRemeshManifest, ForcedRemeshManifestState,
    LIGHT_DISPATCH_BUDGET_PER_POLL, MAX_ACTIVE_BLOCK_CRACKS, MAX_ADMITTED_HEAVY_EVENTS,
    MAX_ADMITTED_WORLD_EVENTS, MAX_IN_FLIGHT_DECODE_JOBS, MAX_IN_FLIGHT_LIGHT_JOBS,
    MAX_LOCAL_RESET_DISPATCH_EVIDENCE, MAX_PENDING_MESH_CHANGES, MAX_SUB_CHUNK_RETRIES, MapImage,
    OUTBOUND_REQUEST_CAPACITY, PHASE0_MAX_VIEW_RADIUS_CHUNKS, PendingSubChunkRequest,
    Phase2PresentationSnapshot, Phase2PublicationSnapshot, PresentModeIdentity,
    PublicationAllowance, PublicationPermit, PublicationPermitStage, PublicationServiceConfig,
    PublicationStageCounters, PublisherViewGeometry, RequestClass, RequestClassDepth,
    RequestQueueEvidence, SUB_CHUNK_RESPONSE_TIMEOUT, SignEditRequest, StageDurations,
    SubChunkOutcomeCounters, ViewCohort, ViewCohortStatus, WORK_RESULT_CAPACITY, WorldMeshChange,
    WorldStream, WorldStreamError, WorldStreamFatalError, WorldStreamNormalizationStats,
    WorldStreamPoll, WorldStreamStats,
};
#[cfg(feature = "publication-test-support")]
pub use stream::{PublicationFixtureIdentity, PublicationFixtureSnapshot};

/// Portable native actor and item-use classification shared with spectator presentation.
pub use render_data::{actor_is_billboard, item_use};
