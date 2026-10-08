//! Original authored inputs retained for replacing a local tick's late evaluation.
use super::*;

#[derive(Debug)]
pub(super) struct Replay {
    pub(super) tick: u64,
    pub(super) geometry: usize,
    pub(super) variables: MolangVariables,
    pub(super) controllers: Vec<ControllerState>,
    pub(super) server_animations: Vec<server_animation::Controller>,
    pub(super) clocks: clock::ClipClocks,
    pub(super) initialized: bool,
    pub(super) reset: bool,
    pub(super) epoch: u64,
    pub(super) elapsed: Option<u32>,
}

impl Replay {
    /// Moves replaced authored buffers into the local tick's baseline without copying a rig.
    pub(super) fn commit(
        state: &mut ActorRigState,
        evaluated: &mut EvaluatedState,
        context: &ActorTickContext,
        tick: u64,
        refresh: bool,
        retain: bool,
    ) {
        let variables = std::mem::replace(
            &mut state.variables,
            std::mem::take(&mut evaluated.variables),
        );
        let controllers = std::mem::replace(
            &mut state.controllers,
            std::mem::take(&mut evaluated.controllers),
        );
        let server_animations = std::mem::replace(
            &mut state.server_animations,
            std::mem::take(&mut evaluated.server_animations),
        );
        let clocks = std::mem::replace(
            &mut state.clip_clocks,
            std::mem::take(&mut evaluated.clip_clocks),
        );
        if !retain {
            state.replay = None;
            return;
        }
        if refresh && let Some(replay) = state.replay.as_mut().filter(|replay| replay.tick == tick)
        {
            if replay.geometry != state.geometry_binding {
                replay.geometry = state.geometry_binding;
                replay.controllers = controllers;
                replay.reset = state.reset_pending;
                replay.epoch = state.animation_epoch;
            }
            return;
        }
        state.replay = Some(Self {
            tick,
            geometry: state.geometry_binding,
            variables,
            controllers,
            server_animations,
            clocks,
            initialized: state.initialized,
            reset: state.reset_pending,
            epoch: state.animation_epoch,
            elapsed: context.animation_elapsed_ticks,
        });
    }
}

impl ActorRigState {
    /// Returns the original authored input only while replacing its exact completed tick.
    pub(super) fn replay_at(&self, tick: u64) -> Option<&Replay> {
        self.replay.as_ref().filter(|replay| replay.tick == tick)
    }
}
