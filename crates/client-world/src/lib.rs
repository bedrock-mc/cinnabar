mod action;
mod actor_store;
mod block_entity_visuals;
#[cfg(any(target_arch = "wasm32", test))]
pub mod browser_jobs;
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
pub use actor_store::{
    ActorCollisionBounds, ActorDamageState, ActorFluidProbe, ActorHitBoxes, ActorPickup, ActorPose,
    ActorShadowCaster, ActorSnapshot, ActorStatus, ActorStatusNotice, BlockEntityCandidate,
    BlockEntityKind, BlockEntityView, CrystalBeamView, DEATH_DURATION_TICKS, DEFAULT_PLAYER_HEALTH,
    DragonDeathView, DroppedItemView, FIRE_FADE_TICKS, HURT_DURATION_TICKS, HURT_OVERLAY_ALPHA,
    LightningBoltView, LocalItemUse, LocalPlayerFeed, LocalVehicleFeed, MAX_DROPPED_ITEM_COPIES,
    MAX_STATUS_NOTICES, MovementFlagUpdate, PICKUP_DURATION_TICKS, PlayerProfile, PoseYOrigin,
    PropertyDefault, ROCKET_LAUNCH_EVENT, RideSeat, RopeKind, RopeView,
    SLEEPING_PLAYER_NETWORK_OFFSET, SeatViewLock, dropped_item_copy_count, seat_world_offset,
    tnt_presentation,
};
pub use block_entity_visuals::{
    BackingBlockIdentity, BlockEntityVisualDiagnostics, BlockEntityVisualRoute,
    adjudicate_block_entity_visual,
};
pub use item::{
    ActorArmorPiece, ActorArmorSnapshot, ActorEquipmentSnapshot, CanonicalItemRegistryRecord,
    CanonicalItemStack, EffectiveItemUse, EquipmentNotice, EquipmentOutcome, MAX_EQUIPMENT_NOTICES,
    MAX_ITEM_REGISTRY_RECORDS, MAX_PENDING_ITEM_RESOLUTIONS, ResolvedUseCooldown,
    UseCooldownCategory, resolve_item_durability, resolve_item_use,
};
pub use server_position::{ResolvedServerPosition, SAFE_SERVER_HEIGHT};

mod authority;
pub use authority::{
    ActorCombatEvidence, ActorCombatKind, BiomeCommitReport, PreparedBlockIdentities,
};
pub use authority::{
    BlockEventCue, COMMITTED_AUDIO_CAPACITY, COMMITTED_CAMERA_CAPACITY, COMMITTED_CONTROL_CAPACITY,
    COMMITTED_PARTICLE_CAPACITY, COMMITTED_UI_CAPACITY, CommittedAudioEvent, CommittedCameraEvent,
    CommittedControlEvent, CommittedParticleEvent, CommittedUiEvent, MapImage,
    MovementSpeedAttribute, PublisherViewGeometry, SignEditRequest, ViewCohort, WorldAuthority,
};
pub use protocol::ItemAttackTiming;

pub use actor_store::{DisplayActorStore, MAX_DISPLAY_ACTORS};

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;
