//! Bounded terrain scheduling and publication for an ordered client world.

mod culling;
mod stream;

// Temporary facade for stream consumers while the app is decomposed.
pub use client_world::game_mode_capabilities;
pub use client_world::{
    ACTOR_SWING_TICKS, ACTOR_TICK_DURATION, ActorAnimationStats, ActorAnimationVariables,
    ActorAnimationView, ActorArmorPiece, ActorArmorSnapshot, ActorEquipmentSnapshot,
    ActorEventIdentity, ActorLifetimeId, ActorPickup, ActorPose, ActorRigSnapshot, ActorSnapshot,
    ActorSourceTick, ActorStatus, ActorStatusNotice, AttachableAnimationInput,
    AttachableRigSnapshot, AttachablesRuntime, BackingBlockIdentity, BlockEntityKind,
    BlockEntityView, BlockEntityVisualRoute, BoneTransform, CanonicalItemRegistryRecord,
    CanonicalItemStack, DEATH_DURATION_TICKS, DroppedItemView, EntityRigId, EquipmentNotice,
    EquipmentOutcome, HURT_DURATION_TICKS, HURT_OVERLAY_ALPHA, HandPhase, ItemAnimationState,
    LightningBoltView, LocalItemUse, LocalPlayerFacts, LocalPlayerFeed, LocalPlayerStat,
    MAX_ACTION_EVENTS_PER_TICK, MAX_ACTIONS_PER_ACTOR, MAX_ACTOR_ACTION_HISTORY,
    MAX_CONTROLLER_TRANSITIONS_PER_TICK, MAX_DROPPED_ITEM_COPIES, MAX_EQUIPMENT_NOTICES,
    MAX_ITEM_REGISTRY_RECORDS, MAX_MOLANG_OPS_PER_ACTOR_TICK, MAX_MOLANG_OPS_PER_RENDER_FRAME,
    MAX_MOLANG_OPS_PER_WORLD_TICK, MAX_PENDING_ITEM_RESOLUTIONS, MAX_RUNTIME_BONES_PER_RIG,
    MAX_STATUS_NOTICES, MODEL_PART_ORIGIN_Y, MovementFlagUpdate, PICKUP_DURATION_TICKS,
    PlayerProfile, PropertyDefault, RemoteActionFallback, RemoteActionSnapshot, RemoteActionStats,
    RenderTextureLayer, ResolvedServerPosition, RideSeat, RopeKind, RopeView, SAFE_SERVER_HEIGHT,
    SeatDefaults, SeatRequirement, SkinRenderLayer, adjudicate_block_entity_visual,
    dropped_item_copy_count, tnt_presentation,
};
pub use culling::CaveVisibilityScratch;
pub use render_api::{
    CLASSIC_SKIN_SIDE, MAX_SKIN_ANIMATION_LAYERS, MAX_STANDARD_SKIN_SIDE, expand_legacy_skin_rgba8,
};

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

pub use stream::ResourceMeshSnapshot;

use client_world::server_position;
