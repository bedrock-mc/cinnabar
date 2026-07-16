use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use bevy::prelude::Resource;
use client_world::CommittedControlEvent;
use render::{PresentedFrameAck, TargetRenderExpectation};
use world::SubChunkKey;

use self::{
    mutation::{MutationTracker, target_mutation_armed_marker},
    remesh::FullViewRemeshTracker,
    teleport::FullViewTeleportTracker,
    world_ready::{GalleryAnchorEmitter, WorldReadySettler},
};
use crate::metrics::TransparentSortMetricsSnapshot;

pub(crate) mod markers;
pub(crate) mod model_witness;
pub(crate) mod mutation;
pub(crate) mod proofs;
pub(crate) mod remesh;
pub(crate) mod teleport;
pub(crate) mod transparent_witness;
pub(crate) mod world_ready;

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
    pub(crate) gallery_anchor: GalleryAnchorEmitter,
    pub(crate) world_ready_settler: WorldReadySettler,
    pub(crate) full_view_teleport: FullViewTeleportTracker,
    pub(crate) full_view_remesh: FullViewRemeshTracker,
    pub(crate) world_ready: bool,
    pub(crate) require_transparent_presentation: bool,
    pub(crate) finished: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcceptanceExitDecision {
    Continue,
    WaitForTransparentPresentation,
    Complete,
    Fatal,
    TransparentPresentationTimedOut,
}

impl AcceptanceExitDecision {
    pub(crate) const fn is_error(self) -> bool {
        matches!(self, Self::Fatal | Self::TransparentPresentationTimedOut)
    }
}

impl AcceptanceRun {
    pub(crate) fn new(
        seconds: Option<u64>,
        metrics_out: Option<PathBuf>,
        full_view_teleport_gate: bool,
        require_transparent_presentation: bool,
    ) -> Self {
        Self {
            duration: seconds.map(Duration::from_secs),
            deadline: None,
            metrics_out,
            mutation_surface_anchor: None,
            source_mutation_coordinate: None,
            mutation: None,
            gallery_anchor: GalleryAnchorEmitter::default(),
            world_ready_settler: WorldReadySettler::default(),
            full_view_teleport: FullViewTeleportTracker::new(full_view_teleport_gate),
            full_view_remesh: FullViewRemeshTracker::default(),
            world_ready: false,
            require_transparent_presentation,
            finished: false,
        }
    }

    pub(crate) fn exit_decision(
        &self,
        now: Instant,
        fatal: bool,
        transparent: TransparentSortMetricsSnapshot,
    ) -> AcceptanceExitDecision {
        if fatal {
            return AcceptanceExitDecision::Fatal;
        }
        let Some(deadline) = self.deadline else {
            return AcceptanceExitDecision::Continue;
        };
        if now < deadline {
            return AcceptanceExitDecision::Continue;
        }
        if !self.require_transparent_presentation {
            return AcceptanceExitDecision::Complete;
        }
        if transparent.ref_count > 0
            && transparent.committed_generation != 0
            && transparent.committed_generation == transparent.encoded_generation
            && transparent.committed_generation == transparent.presented_generation
        {
            return AcceptanceExitDecision::Complete;
        }
        let grace_deadline = deadline
            .checked_add(TRANSPARENT_PRESENTATION_EXIT_GRACE)
            .expect("transparent presentation grace deadline overflowed");
        if now < grace_deadline {
            AcceptanceExitDecision::WaitForTransparentPresentation
        } else {
            AcceptanceExitDecision::TransparentPresentationTimedOut
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.duration.is_some()
    }

    pub(crate) fn begin_world_ready(
        &mut self,
        ready_at: Instant,
        position: [f32; 3],
        local_player_runtime_id: u64,
    ) {
        self.deadline = self.duration.map(|duration| ready_at + duration);
        self.world_ready = true;
        self.full_view_teleport
            .begin_world_ready(position, local_player_runtime_id);
    }

    pub(crate) fn set_mutation_surface_anchor(&mut self, anchor: [i32; 2]) {
        self.mutation_surface_anchor = Some(anchor);
    }

    pub(crate) fn mutation_surface_anchor(&self) -> Option<[i32; 2]> {
        self.mutation_surface_anchor
    }

    pub(crate) fn set_mutation_coordinate(&mut self, coordinate: [i32; 3]) {
        self.mutation_surface_anchor = None;
        self.source_mutation_coordinate = Some(coordinate);
        self.full_view_teleport
            .set_source_mutation_coordinate(coordinate);
        if !self.full_view_teleport.enabled {
            self.mutation = Some(MutationTracker::new(coordinate));
        }
    }

    pub(crate) fn source_mutation_coordinate(&self) -> Option<[i32; 3]> {
        self.source_mutation_coordinate
    }

    pub(crate) fn retarget_mutation(&mut self, coordinate: [i32; 3], armed_at: Instant) -> bool {
        if self.full_view_teleport.completed_target_mutation != Some(coordinate)
            || self.full_view_remesh.completed.is_none()
            || self
                .mutation
                .as_ref()
                .is_some_and(|mutation| mutation.coordinate() == coordinate)
        {
            return false;
        }
        self.mutation_surface_anchor = None;
        self.mutation = Some(MutationTracker::armed(coordinate, armed_at));
        true
    }

    pub(crate) fn target_mutation_marker(&self) -> Option<String> {
        let source = self.source_mutation_coordinate()?;
        let target = self.mutation.as_ref()?.coordinate();
        if self.full_view_teleport.completed_target_mutation != Some(target) {
            return None;
        }
        let view_generation = self.full_view_remesh.completed.as_ref()?.view_generation;
        Some(target_mutation_armed_marker(
            source,
            target,
            view_generation,
        ))
    }

    pub(crate) fn observe_mutation(&mut self, event: &protocol::WorldEvent, observed_at: Instant) {
        if let Some(mutation) = &mut self.mutation {
            mutation.observe(event, observed_at);
        }
    }

    pub(crate) fn observe_full_view_teleport_ingress(
        &mut self,
        event: &protocol::WorldEvent,
        sequence: u64,
        observed_at: Instant,
        current_dimension: i32,
        frame_count: u64,
    ) -> bool {
        self.world_ready
            && self.full_view_teleport.observe_ingress(
                event,
                sequence,
                observed_at,
                current_dimension,
                frame_count,
            )
    }

    pub(crate) fn observe_committed_full_view_control(
        &mut self,
        control: &CommittedControlEvent,
    ) -> bool {
        self.world_ready && self.full_view_teleport.observe_committed_control(control)
    }

    pub(crate) fn acknowledge_mutation(
        &mut self,
        key: SubChunkKey,
        generation: u64,
        dirty_since: Instant,
        applied_at: Instant,
    ) -> Option<Duration> {
        let requires_presented_frame = self.full_view_teleport.enabled;
        self.mutation.as_mut().and_then(|mutation| {
            mutation.acknowledge_upload(
                key,
                generation,
                dirty_since,
                applied_at,
                requires_presented_frame,
            )
        })
    }

    pub(crate) fn mutation_coordinate(&self) -> Option<[i32; 3]> {
        self.mutation
            .as_ref()
            .map(MutationTracker::coordinate)
            .or(self.source_mutation_coordinate)
    }

    pub(crate) fn visible_mutation_count(&self) -> u64 {
        self.mutation
            .as_ref()
            .map_or(0, MutationTracker::visible_count)
    }

    pub(crate) fn reconcile_mutation_presented_expectation(
        &mut self,
        proposed: TargetRenderExpectation,
        now: Instant,
    ) -> Option<TargetRenderExpectation> {
        let minimum_view_generation = self.full_view_remesh.completed.as_ref()?.view_generation;
        self.mutation.as_mut()?.reconcile_presented_expectation(
            proposed,
            minimum_view_generation,
            now,
        )
    }

    pub(crate) fn observe_presented_mutation(
        &mut self,
        acknowledgement: PresentedFrameAck,
    ) -> Option<Duration> {
        self.mutation
            .as_mut()?
            .observe_presented_frame(acknowledgement)
    }
}
