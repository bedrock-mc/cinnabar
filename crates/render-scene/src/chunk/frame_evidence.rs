use crate::chunk::*;
use crate::{ModelWitnessFrameAck, PresentedFrameAck};
pub fn build_presented_frame_ack(
    probe: CompletedFrameProbe,
    evidence: FrameCompletionEvidence,
) -> Option<PresentedFrameAck> {
    let present_returned_at = evidence.present_returned_at?;
    let gpu_completed_at = evidence.submitted_work_done_at?;
    if probe.expectation.render_ready_at > present_returned_at
        || present_returned_at > gpu_completed_at
    {
        return None;
    }
    let model_witness = probe.model_witness.map(|model| ModelWitnessFrameAck {
        revision: model.revision,
        request_hash: model.request_hash,
        frame_sequence: probe.frame_sequence,
        view_generation: probe.expectation.view_generation,
        present_returned_at,
        gpu_completed_at,
        total_model_ref_count: model.total_model_ref_count,
        manifest: model.manifest,
        missing_key_count: model.missing_key_count,
        stale_generation_count: model.stale_generation_count,
        wrong_stream_count: model.wrong_stream_count,
        zero_model_ref_count: model.zero_model_ref_count,
        draw_mismatch_count: model.draw_mismatch_count,
    });
    Some(PresentedFrameAck {
        cohort: probe.expectation.cohort,
        frame_sequence: probe.frame_sequence,
        allocation_manifest: probe.allocation_manifest,
        visible_allocation_manifest: probe.visible_allocation_manifest,
        drawn_manifest: probe.drawn_manifest,
        view_generation: probe.expectation.view_generation,
        render_ready_at: probe.expectation.render_ready_at,
        present_returned_at,
        gpu_completed_at,
        missing_target_instances: probe.missing_target_instances,
        unexpected_target_instances: probe.unexpected_target_instances,
        source_instances: probe.source_instances,
        foreign_instances: probe.foreign_instances,
        stale_generation_instances: probe.stale_generation_instances,
        orphan_allocations: probe.orphan_allocations,
        transparent_sort_generation: probe.transparent_sort_generation,
        model_witness,
    })
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ChunkStreamMask(u8);

impl ChunkStreamMask {
    pub const CUBE: Self = Self(1 << 0);
    pub const MODEL: Self = Self(1 << 1);
    pub const LIQUID: Self = Self(1 << 2);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for ChunkStreamMask {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}
pub const MAX_TRANSPARENT_WITNESS_KEYS: usize = 64;

pub const MAX_MODEL_WITNESS_KEYS: usize = 64;
