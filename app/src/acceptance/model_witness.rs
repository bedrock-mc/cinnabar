//! Model evidence observes only the current committed cohort.
use crate::runtime::world::ClientWorld;
pub(crate) use acceptance::model_witness::{
    ModelWitnessExpectationState, ModelWitnessFileSource, ModelWitnessObservation,
    poll_model_witness_request,
};
use bevy::prelude::*;
use render::{ChunkRenderQueue, ModelWitnessEvidence, ModelWitnessRequest, PresentedFrameGate};
/// Captures world identity and lets the plugin drive the renderer's existing witness gate.
pub(crate) fn drive_model_witness(
    world: Res<ClientWorld>,
    queue: Res<ChunkRenderQueue>,
    frames: Res<PresentedFrameGate>,
    request: Res<ModelWitnessRequest>,
    evidence: Res<ModelWitnessEvidence>,
    mut state: Local<ModelWitnessExpectationState>,
) {
    acceptance::model_witness::drive_model_witness(
        ModelWitnessObservation {
            committed_cohort: world
                .stream
                .as_ref()
                .and_then(chunk_pipeline::WorldStream::committed_view_cohort),
        },
        &queue,
        &frames,
        &request,
        &evidence,
        &mut state,
    );
}
