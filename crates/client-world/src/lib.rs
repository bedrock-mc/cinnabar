mod action;
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
pub use actor_store::{
    ActorDamageState, ActorFluidProbe, ActorHitBoxes, ActorPickup, ActorPose, ActorShadowCaster,
    ActorSnapshot, ActorStatus, ActorStatusNotice, BOUNDING_BOX_HEIGHT_METADATA_KEY,
    BOUNDING_BOX_WIDTH_METADATA_KEY, BlockEntityCandidate, BlockEntityKind, BlockEntityView,
    CrystalBeamView, DEATH_DURATION_TICKS, DEFAULT_PLAYER_HEALTH, DragonDeathView, DroppedItemView,
    FIRE_FADE_TICKS, HITBOX_METADATA_KEY, HURT_DURATION_TICKS, HURT_OVERLAY_ALPHA,
    LightningBoltView, LocalItemUse, LocalPlayerFeed, MAX_DROPPED_ITEM_COPIES, MAX_STATUS_NOTICES,
    MovementFlagUpdate, PICKUP_DURATION_TICKS, PlayerProfile, PropertyDefault, RideSeat, RopeKind,
    RopeView, SCALE_METADATA_KEY, SeatDefaults, SeatRequirement, dropped_item_copy_count,
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
    AIR_DRAG_MODIFIER_ATTRIBUTE, BlockEventCue, COMMITTED_AUDIO_CAPACITY,
    COMMITTED_CAMERA_CAPACITY, COMMITTED_CONTROL_CAPACITY, COMMITTED_PARTICLE_CAPACITY,
    COMMITTED_UI_CAPACITY, CommittedAudioEvent, CommittedCameraEvent, CommittedControlEvent,
    CommittedParticleEvent, CommittedUiEvent, MapImage, MovementSpeedAttribute,
    PublisherViewGeometry, SPRINT_SPEED_MODIFIER_ID, SignEditRequest, ViewCohort, WorldAuthority,
};
pub use protocol::ItemAttackTiming;
