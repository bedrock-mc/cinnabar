//! Authoritative session data and synchronous committed-event application.
use crate::ingestion::MAX_ADMITTED_WORLD_EVENTS;
use crate::{
    ActorArmorSnapshot, ActorEquipmentSnapshot, ActorSnapshot, LocalPlayerFeed, PlayerProfile,
    RemoteActionSnapshot, RemoteActionStats, ResolvedServerPosition,
    actor_animation::{ActorAnimationStats, ActorRigSnapshot},
    actor_store::ActorStore,
    server_position::resolve_server_position,
};
use assets::{NetworkIdMode, ResolvedBiomeTints, RuntimeAssets, RuntimeEntityAssets};
use protocol::{
    BiomeDefinitionEvent, DimensionRange, MovePlayerEvent, PrimitiveShapesEvent, WorldBootstrap,
};
use std::{
    collections::BTreeSet,
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use world::{ChunkKey, ChunkStore};
mod access;
mod actors;
mod biomes;
mod block_events;
mod block_identities;
mod commits;
mod contracts;
mod dimension_ranges;
mod dimension_transfer;
#[cfg(test)]
mod local_movement_flags_tests;
#[cfg(test)]
mod local_skin_selection_tests;
mod map_data;
mod movement_attribute;
mod particles;
#[cfg(test)]
mod pickup_tests;
#[cfg(test)]
mod primitive_shape_tests;
mod queues;
mod sign_edit;
#[cfg(test)]
mod synchronized_audio_tests;
mod terrain;
#[cfg(test)]
mod tests;
pub use biomes::BiomeCommitReport;
pub use block_events::BlockEventCue;
pub use contracts::{
    CommittedAudioEvent, CommittedCameraEvent, CommittedControlEvent, CommittedParticleEvent,
    CommittedUiEvent, PublisherViewGeometry, ViewCohort,
};
pub use map_data::MapImage;
pub use movement_attribute::AIR_DRAG_MODIFIER_ATTRIBUTE;
pub use sign_edit::SignEditRequest;

static NEXT_BIOME_TINT_STREAM_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_ACTOR_SESSION_ID: AtomicU64 = AtomicU64::new(1);
pub const COMMITTED_CONTROL_CAPACITY: usize = MAX_ADMITTED_WORLD_EVENTS;
pub const COMMITTED_UI_CAPACITY: usize = MAX_ADMITTED_WORLD_EVENTS;
pub const COMMITTED_AUDIO_CAPACITY: usize = MAX_ADMITTED_WORLD_EVENTS;
pub const COMMITTED_CAMERA_CAPACITY: usize = MAX_ADMITTED_WORLD_EVENTS;
pub const COMMITTED_PARTICLE_CAPACITY: usize = 512;

/// The one session owner of packed terrain, actors, identities and committed consumer queues.
/// Terrain scheduling borrows this owner synchronously; it keeps no shadow world or frontier.
pub struct WorldAuthority {
    terrain: ChunkStore,
    actors: ActorStore,
    block_events: block_events::BlockEvents,
    map_images: map_data::MapImages,
    pending_sign_edit: Option<SignEditRequest>,
    audio_nondefault_camera_observed: bool,
    network_id_mode: NetworkIdMode,
    air_block_id: u32,
    runtime_assets: Arc<RuntimeAssets>,
    custom_block_ids: std::ops::Range<u32>,
    custom_block_identities: Arc<std::collections::HashMap<u32, u32>>,
    id_remap: Arc<assets::SequentialIdRemap>,
    decode_diagnostics: Arc<crate::ingestion::DecodeDiagnostics>,
    biome_definitions: Arc<[BiomeDefinitionEvent]>,
    resolved_biome_tints: Arc<ResolvedBiomeTints>,
    biome_tint_stream_id: u64,
    biome_tint_revision: u64,
    current_dimension: i32,
    dimension_ranges: std::collections::BTreeMap<i32, (Arc<str>, DimensionRange)>,
    frozen_dimension_ranges: BTreeSet<i32>,
    dimension_range_skips: u64,
    form_dimension_epoch: u64,
    local_player_runtime_id: u64,
    local_player_unique_id: i64,
    local_movement_speed: Option<f64>,
    resolved_server_position: ResolvedServerPosition,
    latest_movement_correction_tick: Option<u64>,
    actor_session_id: u64,
    committed_controls: VecDeque<CommittedControlEvent>,
    committed_ui: VecDeque<CommittedUiEvent>,
    committed_audio: VecDeque<CommittedAudioEvent>,
    committed_camera: VecDeque<CommittedCameraEvent>,
    committed_primitive_shapes: VecDeque<PrimitiveShapesEvent>,
    committed_particles: VecDeque<CommittedParticleEvent>,
}

impl WorldAuthority {
    /// Creates fresh session identities and authoritative state from a validated bootstrap.
    pub fn new(
        bootstrap: WorldBootstrap,
        runtime_assets: Arc<RuntimeAssets>,
        entity_assets: Option<Arc<RuntimeEntityAssets>>,
        current_position: [f32; 3],
        existing_anchor: Option<[i32; 2]>,
    ) -> Self {
        let resolved_server_position =
            resolve_server_position(bootstrap.player_position, current_position, existing_anchor);
        let resolved_biome_tints = Arc::new(
            runtime_assets
                .biome_assets()
                .resolve_live(&[])
                .expect("validated runtime biome assets resolve without live definitions"),
        );
        let biome_tint_stream_id = NEXT_BIOME_TINT_STREAM_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .expect("biome tint stream identity space exhausted");
        let actor_session_id = NEXT_ACTOR_SESSION_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .expect("actor session identity space exhausted");
        let network_id_mode = if bootstrap.block_network_ids_are_hashes {
            NetworkIdMode::Hashed
        } else {
            NetworkIdMode::Sequential
        };
        let air_block_id = runtime_assets
            .air_network_id(network_id_mode)
            .unwrap_or(bootstrap.air_network_id);
        let mut actors = entity_assets.map_or_else(
            || ActorStore::new(actor_session_id, bootstrap.dimension),
            |assets| {
                ActorStore::new_with_entity_assets(actor_session_id, bootstrap.dimension, assets)
            },
        );
        actors.exclude_remote_state_for(bootstrap.local_player_runtime_id);
        Self {
            terrain: ChunkStore::new(),
            actors,
            block_events: block_events::BlockEvents::default(),
            map_images: map_data::MapImages::default(),
            pending_sign_edit: None,
            audio_nondefault_camera_observed: false,
            network_id_mode,
            air_block_id,
            runtime_assets,
            custom_block_ids: 0..0,
            custom_block_identities: Arc::default(),
            id_remap: Arc::default(),
            decode_diagnostics: Arc::default(),
            biome_definitions: Arc::from([]),
            resolved_biome_tints,
            biome_tint_stream_id,
            biome_tint_revision: 0,
            current_dimension: bootstrap.dimension,
            dimension_ranges: std::collections::BTreeMap::new(),
            frozen_dimension_ranges: BTreeSet::new(),
            dimension_range_skips: 0,
            form_dimension_epoch: 0,
            local_player_runtime_id: bootstrap.local_player_runtime_id,
            local_player_unique_id: bootstrap.local_player_unique_id,
            local_movement_speed: None,
            resolved_server_position,
            latest_movement_correction_tick: None,
            actor_session_id,
            committed_controls: VecDeque::new(),
            committed_ui: VecDeque::new(),
            committed_audio: VecDeque::new(),
            committed_camera: VecDeque::new(),
            committed_primitive_shapes: VecDeque::new(),
            committed_particles: VecDeque::new(),
        }
    }
    /// Borrows the committed packed terrain for collision and mesh snapshots.
    pub const fn terrain(&self) -> &ChunkStore {
        &self.terrain
    }
    /// Applies an actor move before any local-player retention work.
    pub fn apply_player_move(&mut self, sequence: u64, movement: MovePlayerEvent) {
        let _ = self.actors.apply_player_move(
            self.actor_session_id,
            sequence,
            self.current_dimension,
            movement,
        );
    }
    /// Resets authoritative actor state at the committed dimension transition.
    pub fn reset_dimension(&mut self, sequence: u64, dimension: i32) {
        let previous_mount = self.actors.ridden_unique_id(self.local_player_unique_id);
        let _ = self
            .actors
            .reset_dimension(self.actor_session_id, sequence, dimension);
        self.current_dimension = dimension;
        self.form_dimension_epoch = sequence;
        self.local_movement_speed = None;
        self.publish_local_mount_change(sequence, previous_mount);
    }
    /// Returns the mount retained for an actor's current lifetime.
    pub fn ridden_unique_id(&self, unique_id: i64) -> Option<i64> {
        self.actors.ridden_unique_id(unique_id)
    }
}
