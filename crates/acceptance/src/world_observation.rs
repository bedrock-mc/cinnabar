//! The read-only world inputs and explicit commands used by evidence.
use crate::world_ready::{SubChunkTimeoutProgress, WorldReadyWork};
use chunk_pipeline::{
    ForcedRemeshManifest, ForcedRemeshManifestState, ViewCohortStatus, WorldStreamStats,
};
use std::{collections::BTreeSet, time::Instant};
use world::{ChunkKey, SubChunkKey};

/// One frame's world, transport and visibility observations.
pub struct WorldReadyObservation {
    pub stats: WorldStreamStats,
    pub missing_mapping_count: u64,
    pub timeout_progress: SubChunkTimeoutProgress,
    pub work: WorldReadyWork,
    pub committed_cohort: Option<ViewCohortStatus>,
    pub required_columns: BTreeSet<ChunkKey>,
    pub target_cohort: Option<ViewCohortStatus>,
    pub loaded_columns: usize,
    pub rendered_sub_chunks: usize,
    pub visible_sub_chunks: usize,
    pub mutation_target_rendered: bool,
    pub mutation_target_visible: bool,
    pub mutation_target_clean: bool,
    pub position: [f32; 3],
    pub local_player_runtime_id: u64,
    pub readiness_produced: u64,
    pub readiness_consumed: u64,
}

/// Commands the runtime accepts without exposing ownership of its world resource.
pub trait WorldReadyCommands {
    /// Starts a remesh only if the frozen publication manifest is still current.
    fn remesh_published_manifest(
        &mut self,
        published: &[(SubChunkKey, u64)],
        now: Instant,
    ) -> Option<ForcedRemeshManifest>;
    /// Observes the completion state of the requested remesh.
    fn forced_remesh_manifest_state(
        &self,
        manifest: &ForcedRemeshManifest,
    ) -> ForcedRemeshManifestState;
    /// Starts the ordinary timed metrics interval.
    fn begin_timed_session(&mut self);
}
