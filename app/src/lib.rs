pub mod args;
pub mod asset_startup;
pub mod camera;
mod environment;
pub mod metrics;
pub mod movement;

use std::{
    collections::{BTreeSet, HashSet, VecDeque},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use acceptance::model_witness::{ModelWitnessFileSource, poll_model_witness_request};
use acceptance::transparent_witness::{
    TransparentWitnessFileSource, poll_transparent_witness_request,
};
use anyhow::{Context, Result, bail};
use asset_startup::{LoadedAssetKind, load_runtime_assets, select_asset_path_from_environment};
use assets::{DIAGNOSTIC_MATERIAL, RuntimeAssets};
use bevy::{
    anti_alias::{AntiAliasPlugin, fxaa::FxaaPlugin},
    app::{AppExit, TerminalCtrlCHandlerPlugin},
    diagnostic::{DiagnosticPath, DiagnosticsStore},
    ecs::system::SystemParam,
    prelude::*,
    render::diagnostic::RenderDiagnosticsPlugin,
    time::Real,
    window::{CursorOptions, PresentMode, PrimaryWindow, WindowCloseRequested, WindowPlugin},
    winit::{UpdateMode, WinitSettings},
};
use camera::{FlyCamera, FlyCameraPlugin, FlyCameraUpdateSet, input_is_active, movement_axes};
use client_world::SAFE_SERVER_HEIGHT;
use client_world::{ActorSnapshot, PlayerProfile};
use client_world::{
    CommittedControlEvent, ViewCohort, ViewCohortStatus, WorldMeshChange, WorldStream,
};
use environment::{
    WeatherState, WorldClock, apply_environment_control, replace_session, update_atmosphere_frame,
};
use meshing::CameraMedium;
use metrics::{
    DiagnosticQuadTracker, ExactFullViewProof, GpuPassMeasurement, MetricsCollector,
    ModelWorkloadMetricsSnapshot, PipelineMetricsSnapshot, TeleportProof,
    TransparentSortMetricsSnapshot, deterministic_manifest_hash, pair_gpu_pass_sample,
};
use movement::{
    MovementInputSample, MovementSendError, MovementSource, MovementTicker,
    flush_player_auth_inputs,
};
use render::{
    ActorRenderFrame, ActorRenderPlugin, ActorRenderScene, ActorRenderSource, ActorSkinPixels,
    AtmosphereFrame, AtmospherePlugin, AtmosphereTextureAssets, ChunkBiomeTints,
    ChunkRenderApplySet, ChunkRenderInstance, ChunkRenderPlugin, ChunkRenderQueue,
    ChunkTextureAssets, ChunkUploadAcknowledgements, ChunkUploadPriority, ChunkUploadToken,
    ModelWitnessEvidence, ModelWitnessManifestRecord, ModelWorkloadMetrics, PresentedFrameAck,
    PresentedFrameGate, RenderViewCohort, TargetRenderExpectation, TransparentSortMetrics,
    TransparentWitnessEvidence, VisibilityDiagnostics, VisibilityDiagnosticsInput,
    VisibilityKeyDelta, VisibilityKeyDigest,
};
use runtime::network::{NetworkConfig, NetworkHandle, spawn_network};
use sha2::{Digest, Sha256};
use world::SubChunkKey;

const MESH_JOB_BUDGET_PER_FRAME: usize = 128;
const GPU_UPLOAD_BUDGET_PER_FRAME: usize = 128;
const NETWORK_INGRESS_BUDGET_PER_FRAME: usize = 32;
const OUTBOUND_SEND_BUDGET_PER_FRAME: usize = 16;
const TITLE_REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const VISIBILITY_DIAGNOSTIC_INTERVAL: Duration = Duration::from_secs(1);
const OPAQUE_3D_GPU_DIAGNOSTIC: DiagnosticPath =
    DiagnosticPath::const_new("render/main_opaque_pass_3d/elapsed_gpu");
const TRANSPARENT_3D_GPU_DIAGNOSTIC: DiagnosticPath =
    DiagnosticPath::const_new("render/main_transparent_pass_3d/elapsed_gpu");
const WORLD_READY_QUIET_INTERVAL: Duration = Duration::from_secs(2);
const TRANSPARENT_PRESENTATION_EXIT_GRACE: Duration = Duration::from_secs(2);
const SHUTDOWN_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(2);
const TELEPORT_COHORT_PROGRESS_INTERVAL: Duration = Duration::from_secs(1);
const PHASE0_REQUESTED_RADIUS_CHUNKS: i32 = 16;
const MUTATION_X_OFFSET_BLOCKS: i32 = 4;
const LEAF_FOREST_FAR_OFFSET_CHUNKS: i32 = 65;
const LEAF_FOREST_FAR_OFFSET_BLOCKS: i32 = LEAF_FOREST_FAR_OFFSET_CHUNKS * 16;
const LEAF_FOREST_MUTATION_Z_OFFSET_BLOCKS: i32 = 12;
const FULL_VIEW_TELEPORT_MIN_CHUNK_DELTA: u64 = (PHASE0_REQUESTED_RADIUS_CHUNKS as u64) * 2 + 1;
const _: () = assert!(runtime::network::WORLD_EVENT_CAPACITY >= NETWORK_INGRESS_BUDGET_PER_FRAME);
const _: () = assert!(NETWORK_INGRESS_BUDGET_PER_FRAME == client_world::MAX_ADMITTED_HEAVY_EVENTS);

mod acceptance;
mod app;
mod runtime;

use acceptance::{markers::*, mutation::*, proofs::*, remesh::*, teleport::*, world_ready::*, *};
use runtime::{endpoint::*, network::*, shutdown::*, telemetry::*, visibility::*, world::*};

pub use app::run;

#[cfg(test)]
mod tests;
