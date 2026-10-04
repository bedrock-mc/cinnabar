//! Optional evidence and control adapters over committed runtime observations.
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
use diagnostics::bounded_file;
pub use diagnostics::markers;
pub use diagnostics::metrics;
pub mod model_witness;
pub mod mutation;
pub mod phase2_evidence;
mod phase3;
pub mod phase3_evidence;
pub mod proofs;
pub mod remesh;
pub mod teleport;
pub mod transparent_witness;
pub mod world_observation;
pub mod world_ready;

mod run;
pub use exit::AcceptanceExitDecision;
pub use phase3::Phase3TerminalDrainDecision;

pub use diagnostics::PHASE0_REQUESTED_RADIUS_CHUNKS;
pub const TRANSPARENT_PRESENTATION_EXIT_GRACE: Duration = Duration::from_secs(2);

#[derive(Resource)]
pub struct AcceptanceRun {
    pub duration: Option<Duration>,
    pub deadline: Option<Instant>,
    pub metrics_out: Option<PathBuf>,
    pub mutation_surface_anchor: Option<[i32; 2]>,
    pub source_mutation_coordinate: Option<[i32; 3]>,
    pub mutation: Option<MutationTracker>,
    pub mutation_cohort: Option<ViewCohortStatus>,
    pub gallery_anchor: GalleryAnchorEmitter,
    pub world_ready_settler: WorldReadySettler,
    pub full_view_teleport: FullViewTeleportTracker,
    pub full_view_remesh: FullViewRemeshTracker,
    pub world_ready: bool,
    pub require_transparent_presentation: bool,
    pub shutdown_requested: bool,
    pub finished: bool,
}

pub mod audio_wire;
pub mod committed_control;

mod plugin;
pub use plugin::AcceptancePlugin;

pub mod finish;

pub mod witness_markers;
