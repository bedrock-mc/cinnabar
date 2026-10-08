//! UI animation retains its own controller clocks and script variables.

use super::*;

#[derive(Debug)]
pub(super) struct UiAnimationState {
    controllers: Vec<ControllerState>,
    pub(super) server_animations: Vec<server_animation::Controller>,
    clip_clocks: clock::ClipClocks,
    variables: MolangVariables,
    initialized: bool,
    replay: Option<replay::Replay>,
}

impl UiAnimationState {
    /// Lends the HUD's script state to the shared evaluator, preserving the world state.
    fn swap_with(&mut self, state: &mut ActorRigState) {
        std::mem::swap(&mut self.controllers, &mut state.controllers);
        std::mem::swap(&mut self.server_animations, &mut state.server_animations);
        std::mem::swap(&mut self.clip_clocks, &mut state.clip_clocks);
        std::mem::swap(&mut self.variables, &mut state.variables);
        std::mem::swap(&mut self.initialized, &mut state.initialized);
        std::mem::swap(&mut self.replay, &mut state.replay);
    }
}

/// Evaluates the UI animation component and retains it independently of world rendering.
/// Vanilla selects that separate component per actor; the HUD forces
/// third person before drawing the same actor in the paper doll.
#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    state: &mut ActorRigState,
    actor: &ActorSnapshot,
    context: &ActorTickContext,
    tick: u64,
    budget: &mut EvalBudget<'_>,
    advance_clocks: bool,
) {
    let existing = state.ui_animation.take();
    let original = (!advance_clocks && existing.is_none())
        .then(|| state.replay_at(tick))
        .flatten();
    let replay_context = original.map(|replay| ActorTickContext {
        animation_elapsed_ticks: replay.elapsed,
        ..context.clone()
    });
    let initialize_replay = replay_context.is_some();
    let mut ui = existing.unwrap_or_else(|| UiAnimationState {
        controllers: state.controllers.clone(),
        server_animations: state.server_animations.clone(),
        clip_clocks: original
            .map_or(&state.clip_clocks, |replay| &replay.clocks)
            .clone(),
        variables: original
            .map_or(&state.variables, |replay| &replay.variables)
            .clone(),
        initialized: original.map_or(state.initialized, |replay| replay.initialized),
        replay: None,
    });
    let evaluation_context = replay_context.as_ref().unwrap_or(context);
    ui.swap_with(state);
    let result = evaluate_state(
        assets,
        layout,
        state,
        actor,
        evaluation_context,
        tick,
        budget,
        advance_clocks || initialize_replay,
        None,
    );
    let pose = match result {
        Ok(mut evaluated) => {
            replay::Replay::commit(
                state,
                &mut evaluated,
                evaluation_context,
                tick,
                !advance_clocks,
                true,
            );
            state.initialized = true;
            evaluated.pose
        }
        Err(_) => Vec::new(),
    };
    ui.swap_with(state);
    state.ui_pose = Some(pose);
    state.ui_animation = Some(ui);
}
