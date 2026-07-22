use std::sync::{Arc, Mutex};

use bevy::prelude::Resource;

use super::{ActorRigRejects, ActorRigRoute};

const MAX_ACTOR_WITNESS_EMISSIONS_PER_STAGE: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActorMainWitness {
    pub local_snapshot: bool,
    pub local_visible: bool,
    pub expected_runtime_id: u64,
    pub visibility_runtime_id: u64,
    pub local_authority: &'static str,
    pub selected_count: usize,
    pub local_route: Option<ActorRigRoute>,
    pub frame_instances: usize,
    pub frame_manifest: usize,
    pub skin_bytes: usize,
    pub rejects: ActorRigRejects,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ActorPrepareWitness {
    pub input_instances: usize,
    pub input_manifest: usize,
    pub skin_bytes: usize,
    pub skin_plan: bool,
    pub valid: bool,
    pub prepared_instances: u32,
    pub maximum_vertices: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ActorQueueWitness {
    pub prepared_instances: u32,
    pub bind_group: bool,
    pub view_count: usize,
    pub queued: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ActorDrawWitness {
    pub executed: bool,
    pub instances: u32,
    pub maximum_vertices: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ActorSubmitWitness {
    pub drawn_frame: bool,
    pub exact: bool,
    pub reserved: bool,
    pub acknowledged: bool,
}

#[derive(Default)]
struct ActorRuntimeWitnessState {
    emissions: [usize; 5],
    main: Option<ActorMainWitness>,
    prepare: Option<ActorPrepareWitness>,
    queue: Option<ActorQueueWitness>,
    draw: Option<ActorDrawWitness>,
    submit: Option<ActorSubmitWitness>,
}

#[derive(Clone, Default, Resource)]
pub struct ActorRuntimeWitness {
    state: Arc<Mutex<ActorRuntimeWitnessState>>,
}

impl ActorRuntimeWitness {
    fn changed<T: Copy + Eq>(
        &self,
        select: impl FnOnce(&mut ActorRuntimeWitnessState) -> &mut Option<T>,
        stage: usize,
        observation: T,
    ) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let slot = select(&mut state);
        if *slot == Some(observation) {
            return false;
        }
        *slot = Some(observation);
        if state.emissions[stage] == MAX_ACTOR_WITNESS_EMISSIONS_PER_STAGE {
            return false;
        }
        state.emissions[stage] += 1;
        true
    }

    pub fn observe_main(&self, observation: ActorMainWitness) {
        if self.changed(|state| &mut state.main, 0, observation) && !cfg!(test) {
            eprintln!(
                "RUST_MCBE_ACTOR_WITNESS stage=main local_snapshot={} local_visible={} expected_runtime_id={} visibility_runtime_id={} local_authority={} selected_count={} local_route={:?} frame_instances={} frame_manifest={} skin_bytes={} rejects={:?}",
                observation.local_snapshot,
                observation.local_visible,
                observation.expected_runtime_id,
                observation.visibility_runtime_id,
                observation.local_authority,
                observation.selected_count,
                observation.local_route,
                observation.frame_instances,
                observation.frame_manifest,
                observation.skin_bytes,
                observation.rejects,
            );
        }
    }

    pub(crate) fn observe_prepare(&self, observation: ActorPrepareWitness) {
        if self.changed(|state| &mut state.prepare, 1, observation) && !cfg!(test) {
            eprintln!(
                "RUST_MCBE_ACTOR_WITNESS stage=prepare input_instances={} input_manifest={} skin_bytes={} skin_plan={} valid={} prepared_instances={} maximum_vertices={}",
                observation.input_instances,
                observation.input_manifest,
                observation.skin_bytes,
                observation.skin_plan,
                observation.valid,
                observation.prepared_instances,
                observation.maximum_vertices,
            );
        }
    }

    pub(crate) fn observe_queue(&self, observation: ActorQueueWitness) {
        if self.changed(|state| &mut state.queue, 2, observation) && !cfg!(test) {
            eprintln!(
                "RUST_MCBE_ACTOR_WITNESS stage=queue prepared_instances={} bind_group={} view_count={} queued={}",
                observation.prepared_instances,
                observation.bind_group,
                observation.view_count,
                observation.queued,
            );
        }
    }

    pub(crate) fn observe_draw(&self, observation: ActorDrawWitness) {
        if self.changed(|state| &mut state.draw, 3, observation) && !cfg!(test) {
            eprintln!(
                "RUST_MCBE_ACTOR_WITNESS stage=draw executed={} instances={} maximum_vertices={}",
                observation.executed, observation.instances, observation.maximum_vertices,
            );
        }
    }

    pub(crate) fn observe_submit(&self, observation: ActorSubmitWitness) {
        if self.changed(|state| &mut state.submit, 4, observation) && !cfg!(test) {
            eprintln!(
                "RUST_MCBE_ACTOR_WITNESS stage=submit drawn_frame={} exact={} reserved={} acknowledged={}",
                observation.drawn_frame,
                observation.exact,
                observation.reserved,
                observation.acknowledged,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn witness_is_change_driven_and_globally_bounded() {
        let witness = ActorRuntimeWitness::default();
        let observation = ActorQueueWitness {
            prepared_instances: 1,
            bind_group: true,
            view_count: 1,
            queued: true,
        };
        witness.observe_queue(observation);
        witness.observe_queue(observation);
        assert_eq!(witness.state.lock().unwrap().emissions[2], 1);

        for index in 0..MAX_ACTOR_WITNESS_EMISSIONS_PER_STAGE * 2 {
            witness.observe_draw(ActorDrawWitness {
                executed: index % 2 == 0,
                instances: index as u32,
                maximum_vertices: 216,
            });
        }
        assert_eq!(
            witness.state.lock().unwrap().emissions[3],
            MAX_ACTOR_WITNESS_EMISSIONS_PER_STAGE
        );
    }
}
