//! UI animation retains its own controller clocks and script variables.

use super::*;

#[derive(Debug)]
pub(super) struct UiAnimationState {
    controllers: Vec<ControllerState>,
    clip_clocks: clock::ClipClocks,
    variables: MolangVariables,
    initialized: bool,
}

impl UiAnimationState {
    /// Lends the HUD's script state to the shared evaluator, preserving the world state.
    fn swap_with(&mut self, state: &mut ActorRigState) {
        std::mem::swap(&mut self.controllers, &mut state.controllers);
        std::mem::swap(&mut self.clip_clocks, &mut state.clip_clocks);
        std::mem::swap(&mut self.variables, &mut state.variables);
        std::mem::swap(&mut self.initialized, &mut state.initialized);
    }
}

/// Evaluates the UI animation component and retains it independently of world rendering.
/// Vanilla selects that separate component in Actor; the HUD forces
/// third person before drawing the same actor in HudPlayerRenderer.
#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    state: &mut ActorRigState,
    actor: &ActorSnapshot,
    context: &ActorTickContext,
    tick: u64,
    budget: &mut EvalBudget<'_>,
) {
    let mut ui = state
        .ui_animation
        .take()
        .unwrap_or_else(|| UiAnimationState {
            controllers: state.controllers.clone(),
            clip_clocks: state.clip_clocks.clone(),
            variables: state.variables.clone(),
            initialized: state.initialized,
        });
    ui.swap_with(state);
    let result = evaluate_state(assets, layout, state, actor, context, tick, budget, None);
    ui.swap_with(state);
    state.ui_pose = Some(match result {
        Ok(evaluated) => {
            ui.controllers = evaluated.controllers;
            ui.clip_clocks = evaluated.clip_clocks;
            ui.variables = evaluated.variables;
            ui.initialized = true;
            evaluated.pose
        }
        Err(_) => Vec::new(),
    });
    state.ui_animation = Some(ui);
}
