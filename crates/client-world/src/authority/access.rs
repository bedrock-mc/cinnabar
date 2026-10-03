use super::*;

impl WorldAuthority {
    /// Reports whether this session has committed a nondefault camera event.
    pub const fn audio_nondefault_camera_observed(&self) -> bool {
        self.audio_nondefault_camera_observed
    }
    /// Reads the committed network id mode.
    pub const fn network_id_mode(&self) -> NetworkIdMode {
        self.network_id_mode
    }
    /// Returns the air identity selected once from this session's admitted registry.
    pub const fn air_block_id(&self) -> u32 {
        self.air_block_id
    }
    /// Reads the committed biome tint stream id.
    pub const fn biome_tint_stream_id(&self) -> u64 {
        self.biome_tint_stream_id
    }
    /// Reads the committed biome tint revision.
    pub const fn biome_tint_revision(&self) -> u64 {
        self.biome_tint_revision
    }
    /// Reads the committed current dimension.
    pub const fn current_dimension(&self) -> i32 {
        self.current_dimension
    }
    /// Reads the committed form dimension epoch.
    pub const fn form_dimension_epoch(&self) -> u64 {
        self.form_dimension_epoch
    }
    /// Reads the committed local player runtime id.
    pub const fn local_player_runtime_id(&self) -> u64 {
        self.local_player_runtime_id
    }
    /// Reads the committed local movement speed.
    pub const fn local_movement_speed(&self) -> Option<f64> {
        self.local_movement_speed
    }
    /// Reads the committed resolved server position.
    pub const fn resolved_server_position(&self) -> ResolvedServerPosition {
        self.resolved_server_position
    }
    /// Reads the committed actor session id.
    pub const fn actor_session_id(&self) -> u64 {
        self.actor_session_id
    }
    /// Borrows the session runtime assets.
    pub fn runtime_assets(&self) -> &Arc<RuntimeAssets> {
        &self.runtime_assets
    }
    /// Borrows the session biome definitions.
    pub fn biome_definitions(&self) -> &Arc<[BiomeDefinitionEvent]> {
        &self.biome_definitions
    }
    /// Borrows the session resolved biome tints.
    pub fn resolved_biome_tints(&self) -> &Arc<ResolvedBiomeTints> {
        &self.resolved_biome_tints
    }
    /// Resolves and retains a server position using this session's existing anchor.
    pub fn resolve_position(&mut self, position: [f32; 3]) -> ResolvedServerPosition {
        let resolved = resolve_server_position(
            position,
            self.resolved_server_position.position,
            self.resolved_server_position.surface_anchor,
        );
        self.resolved_server_position = resolved;
        resolved
    }
    /// Accepts correction ticks in their original nondecreasing order.
    pub fn accept_movement_correction_tick(&mut self, tick: u64) -> bool {
        if self
            .latest_movement_correction_tick
            .is_some_and(|latest| tick < latest)
        {
            return false;
        }
        self.latest_movement_correction_tick = Some(tick);
        true
    }
    /// Replaces the session's resource view without changing its network identity.
    pub fn replace_runtime_assets(&mut self, assets: Arc<RuntimeAssets>) {
        self.runtime_assets = assets;
    }
    /// Retains server-defined sequential block identities for palette decoding.
    pub fn set_custom_block_ids(&mut self, ids: std::ops::Range<u32>) {
        self.custom_block_ids = ids;
    }
    /// Retains the single wire-to-internal sequential palette mapping.
    pub fn set_sequential_id_remap(&mut self, remap: assets::SequentialIdRemap) {
        eprintln!(
            "SESSION_BLOCK_PALETTE session={} mode={:?} air={:#010x} visual_count={} block_registry_sha256={} custom_internal_ids={:?} sequential_id_remapped={}",
            self.actor_session_id,
            self.network_id_mode,
            self.air_block_id,
            self.runtime_assets.visual_count(),
            crate::ingestion::block_registry_sha256(&self.runtime_assets),
            self.custom_block_ids,
            !remap.is_identity(),
        );
        self.id_remap = Arc::new(remap);
    }
    /// Captures the session registry view used by one admitted decode job.
    pub fn decode_ids(&self, dimension: i32) -> crate::ingestion::DecodeIds {
        crate::ingestion::DecodeIds {
            assets: Arc::clone(&self.runtime_assets),
            custom_blocks: self.custom_block_ids.clone(),
            remap: Arc::clone(&self.id_remap),
            diagnostics: Arc::clone(&self.decode_diagnostics),
            session_id: self.actor_session_id,
            mode: self.network_id_mode,
            air: self.air_block_id,
            biome_tints: Arc::clone(&self.resolved_biome_tints),
            default_biome: crate::ingestion::default_biome_id(dimension),
        }
    }
    /// Counts retained audio events for bounded-work diagnostics.
    pub fn committed_audio_count(&self) -> usize {
        self.committed_audio.len()
    }
    /// Counts retained camera events for bounded-work diagnostics.
    pub fn committed_camera_count(&self) -> usize {
        self.committed_camera.len()
    }
}
