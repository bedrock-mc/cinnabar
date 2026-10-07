//! Publishes the captured actor batch after authoritative input has been sent.
use super::*;
use crate::presentation::actors::{ActorPresentationBatch, update_actor_rig_scene};
use render::{ActorRenderFrame, ActorRuntimeWitness};

#[derive(Resource, Default)]
pub struct PreparedActorPublication(pub(super) Option<PendingActorPublication>);

impl PreparedActorPublication {
    #[cfg(any(test, feature = "test-support"))]
    pub fn submissions(&self) -> Option<&[render::ActorRigSubmission]> {
        self.0
            .as_ref()
            .map(|pending| pending.batch.submissions.as_slice())
    }
}

pub(super) struct PendingActorPublication {
    pub(super) batch: ActorPresentationBatch,
    pub(super) partial_tick: f32,
    pub(super) witness: ActorMainWitness,
}

/// Builds GPU payloads from the same captured inputs that preceded the network send.
pub fn publish_actor_render_frame(
    mut prepared: ResMut<PreparedActorPublication>,
    mut scene: ResMut<ActorRenderScene>,
    mut frame: ResMut<ActorRenderFrame>,
    witness: Res<ActorRuntimeWitness>,
    profiler: Option<Res<render::RuntimeStageProfiler>>,
) {
    let Some(mut prepared) = prepared.0.take() else {
        return;
    };
    let _publication = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::ActorPublication));
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::ActorRigBuild));
    *frame = update_actor_rig_scene(&mut scene, prepared.partial_tick, prepared.batch).clone();
    prepared.witness.local_route = frame
        .rig
        .manifest
        .iter()
        .find(|entry| entry.identity.runtime_id == prepared.witness.expected_runtime_id)
        .map(|entry| entry.route);
    prepared.witness.frame_instances = frame.rig.instances.len();
    prepared.witness.frame_manifest = frame.rig.manifest.len();
    prepared.witness.skin_bytes = frame.skin_bytes();
    prepared.witness.rejects = frame.rig.rejects;
    witness.observe_main(prepared.witness);
}
