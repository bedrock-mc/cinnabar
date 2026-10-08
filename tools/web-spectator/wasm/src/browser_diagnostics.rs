//! Bounded observations from the native render gates; no readiness decisions.
use render::{ActorPresentedFrameAck, PresentedFrameAck};
use render_model::{UiRenderStatsSnapshot, VisibilityDiagnosticSnapshot};
use serde::Serialize;

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Diagnostics {
    pub main_updates: u64,
    pub pending_chunks: usize,
    pub expected_chunks: usize,
    pub gpu_acknowledgements: u64,
    pub rejected_acknowledgements: u64,
    pub last_acknowledgement: Option<ChunkAcknowledgement>,
    pub actor_gpu_acknowledgements: u64,
    pub actor_exact_acknowledgements: u64,
    pub last_actor_instances: usize,
    pub nametag_records: usize,
    pub visibility: Visibility,
    pub hud: Hud,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChunkAcknowledgement {
    exact: bool,
    allocation_chunks: usize,
    visible_chunks: usize,
    drawn_chunks: usize,
    missing: usize,
    unexpected: usize,
    source: usize,
    foreign: usize,
    stale: usize,
    orphan: usize,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Visibility {
    pose_generation: u64,
    view_generation: u64,
    frustum_chunks: Option<u64>,
    submitted_chunks: Option<u64>,
    gpu_completed_chunks: Option<u64>,
    draw_mode: String,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Hud {
    accepted_revision: Option<u64>,
    uploaded_vertices: u32,
    draw_calls: u32,
    rejections: u64,
}

impl Diagnostics {
    pub fn observe_chunk(&mut self, acknowledgement: &PresentedFrameAck) {
        let exact = acknowledgement.is_exact();
        self.gpu_acknowledgements = self.gpu_acknowledgements.saturating_add(1);
        if !exact || acknowledgement.drawn_manifest.is_empty() {
            self.rejected_acknowledgements = self.rejected_acknowledgements.saturating_add(1);
        }
        self.last_acknowledgement = Some(ChunkAcknowledgement {
            exact,
            allocation_chunks: acknowledgement.allocation_manifest.len(),
            visible_chunks: acknowledgement.visible_allocation_manifest.len(),
            drawn_chunks: acknowledgement.drawn_manifest.len(),
            missing: acknowledgement.missing_target_instances,
            unexpected: acknowledgement.unexpected_target_instances,
            source: acknowledgement.source_instances,
            foreign: acknowledgement.foreign_instances,
            stale: acknowledgement.stale_generation_instances,
            orphan: acknowledgement.orphan_allocations,
        });
    }

    pub fn observe_actor(&mut self, acknowledgement: &ActorPresentedFrameAck) {
        self.actor_gpu_acknowledgements = self.actor_gpu_acknowledgements.saturating_add(1);
        if acknowledgement.is_exact() {
            self.actor_exact_acknowledgements = self.actor_exact_acknowledgements.saturating_add(1);
        }
        self.last_actor_instances = acknowledgement.manifest.len();
    }

    pub fn observe_visibility(&mut self, snapshot: VisibilityDiagnosticSnapshot) {
        self.visibility = Visibility {
            pose_generation: snapshot.pose_generation,
            view_generation: snapshot.view_generation,
            frustum_chunks: snapshot.frustum_visible_opaque.map(|digest| digest.count),
            submitted_chunks: snapshot.submitted_opaque.map(|digest| digest.count),
            gpu_completed_chunks: snapshot.gpu_completed_opaque.map(|digest| digest.count),
            draw_mode: format!("{:?}", snapshot.draw_mode),
        };
    }

    pub fn observe_hud(&mut self, snapshot: UiRenderStatsSnapshot) {
        self.hud = Hud {
            accepted_revision: snapshot.accepted_revision,
            uploaded_vertices: snapshot.uploaded_vertices,
            draw_calls: snapshot.draw_calls,
            rejections: snapshot.rejection_count,
        };
    }
}
