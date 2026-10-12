//! Publishes the captured actor batch after authoritative input has been sent.
use super::*;
use crate::presentation::actors::{ActorPresentationBatch, update_actor_rig_scene};
use render_scene::{ActorMainWitness, ActorRenderFrame, ActorRuntimeWitness};

#[derive(Resource, Default)]
pub struct PreparedActorPublication(pub(super) Option<PendingActorPublication>);

impl PreparedActorPublication {
    #[cfg(any(test, feature = "test-support"))]
    pub fn submissions(&self) -> Option<&[render_model::ActorRigSubmission]> {
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

impl PendingActorPublication {
    /// Records the pre-layer body count and captured visibility for final publication.
    pub(super) fn new(
        batch: ActorPresentationBatch,
        partial_tick: f32,
        visibility: Option<crate::local_player::FrozenLocalAvatarVisibility>,
        local_visible: bool,
        local_runtime_id: u64,
        selected_count: usize,
        unrigged_actors: usize,
    ) -> Self {
        Self {
            batch,
            partial_tick,
            witness: ActorMainWitness {
                local_snapshot: visibility.is_some(),
                local_visible,
                expected_runtime_id: local_runtime_id,
                visibility_runtime_id: visibility.map_or(0, |snapshot| snapshot.runtime_id()),
                selected_count,
                local_route: None,
                frame_instances: 0,
                frame_manifest: 0,
                skin_bytes: 0,
                rejects: Default::default(),
                unrigged_actors,
            },
        }
    }
}

/// Builds GPU payloads from the same captured inputs that preceded the network send.
pub fn publish_actor_render_frame(
    mut prepared: ResMut<PreparedActorPublication>,
    mut scene: ResMut<ActorRenderScene>,
    mut frame: ResMut<ActorRenderFrame>,
    witness: Res<ActorRuntimeWitness>,
    profiler: Option<Res<render_scene::RuntimeStageProfiler>>,
) {
    let Some(mut prepared) = prepared.0.take() else {
        return;
    };
    let _publication = profiler
        .as_deref()
        .map(|profiler| profiler.time(render_scene::RuntimeStage::ActorPublication));
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render_scene::RuntimeStage::ActorRigBuild));
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
