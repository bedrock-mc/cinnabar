use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use bevy::prelude::Resource;
use client_world::{CommittedControlEvent, ViewCohortStatus};
use render::{PresentedFrameAck, TargetRenderExpectation};
use world::SubChunkKey;

use self::{
    mutation::{MutationTracker, target_mutation_armed_marker},
    remesh::FullViewRemeshTracker,
    teleport::FullViewTeleportTracker,
    world_ready::{GalleryAnchorEmitter, WorldReadySettler},
};
use crate::metrics::TransparentSortMetricsSnapshot;

mod exit;
pub(crate) mod markers;
pub(crate) mod model_witness;
pub(crate) mod mutation;
mod phase3;
pub(crate) mod proofs;
pub(crate) mod remesh;
pub(crate) mod teleport;
pub(crate) mod transparent_witness;
pub(crate) mod world_ready;

mod run;
pub(crate) use exit::AcceptanceExitDecision;
pub(crate) use phase3::Phase3TerminalDrainDecision;

pub(crate) const PHASE0_REQUESTED_RADIUS_CHUNKS: i32 = 16;
pub(crate) const TRANSPARENT_PRESENTATION_EXIT_GRACE: Duration = Duration::from_secs(2);

#[derive(Resource)]
pub(crate) struct AcceptanceRun {
    pub(crate) duration: Option<Duration>,
    pub(crate) deadline: Option<Instant>,
    pub(crate) metrics_out: Option<PathBuf>,
    pub(crate) mutation_surface_anchor: Option<[i32; 2]>,
    pub(crate) source_mutation_coordinate: Option<[i32; 3]>,
    pub(crate) mutation: Option<MutationTracker>,
    pub(crate) mutation_cohort: Option<ViewCohortStatus>,
    pub(crate) gallery_anchor: GalleryAnchorEmitter,
    pub(crate) world_ready_settler: WorldReadySettler,
    pub(crate) full_view_teleport: FullViewTeleportTracker,
    pub(crate) full_view_remesh: FullViewRemeshTracker,
    pub(crate) world_ready: bool,
    pub(crate) require_transparent_presentation: bool,
    pub(crate) shutdown_requested: bool,
    pub(crate) finished: bool,
}
