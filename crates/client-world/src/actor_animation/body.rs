//! Optional world-context third-person animation for the local first-person body's shadow.

use super::*;

#[derive(Debug)]
pub(super) struct WorldBodyState {
    controllers: Vec<ControllerState>,
    clip_clocks: clock::ClipClocks,
    variables: MolangVariables,
    initialized: bool,
    previous: Vec<BoneTransform>,
    current: Vec<BoneTransform>,
    render: Vec<RenderTextureLayer>,
    skin_layers: Vec<SkinRenderLayer>,
    scale: Option<[f32; 4]>,
    completed_tick: Option<u64>,
}

impl WorldBodyState {
    fn swap_evaluator(&mut self, state: &mut ActorRigState) {
        std::mem::swap(&mut self.controllers, &mut state.controllers);
        std::mem::swap(&mut self.clip_clocks, &mut state.clip_clocks);
        std::mem::swap(&mut self.variables, &mut state.variables);
        std::mem::swap(&mut self.initialized, &mut state.initialized);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    state: &mut ActorRigState,
    actor: &ActorSnapshot,
    context: &ActorTickContext,
    tick: u64,
    budget: &mut EvalBudget<'_>,
) -> Result<(), EvalError> {
    if state
        .world_body
        .as_ref()
        .is_some_and(|body| body.completed_tick == Some(tick))
    {
        return Ok(());
    }
    let mut body = state.world_body.take().unwrap_or_else(|| WorldBodyState {
        controllers: state.controllers.clone(),
        clip_clocks: state.clip_clocks.clone(),
        variables: state.variables.clone(),
        initialized: state.initialized,
        previous: Vec::new(),
        current: Vec::new(),
        render: Vec::new(),
        skin_layers: Vec::new(),
        scale: None,
        completed_tick: None,
    });
    let context = ActorTickContext {
        is_local_first_person: false,
        is_in_ui: false,
        ..context.clone()
    };
    let mut body_budget = EvalBudget {
        actor_left: MAX_MOLANG_OPS_PER_ACTOR_TICK,
        world_left: &mut *budget.world_left,
        work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
        transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
        used: 0,
        static_draw: None,
        stack: std::mem::take(&mut budget.stack),
    };
    body.swap_evaluator(state);
    let result = evaluate_state(
        assets,
        layout,
        state,
        actor,
        &context,
        tick,
        &mut body_budget,
        true,
        None,
    );
    body.swap_evaluator(state);
    budget.used += body_budget.used;
    budget.stack = std::mem::take(&mut body_budget.stack);
    let status = match result {
        Ok(mut evaluated) => {
            let reset = body.completed_tick.is_none()
                || state.reset_pending
                || body.current.len() != evaluated.pose.len();
            if reset {
                body.previous.clone_from(&evaluated.pose);
            } else {
                body.previous = std::mem::take(&mut body.current);
            }
            body.current = evaluated.pose;
            skin_layers::carry(&body.skin_layers, &mut evaluated.skin_layers, reset, true);
            body.skin_layers = evaluated.skin_layers;
            if let Some(mut layers) = evaluated.render {
                render::carry_layer_poses(&body.render, &mut layers, reset, true);
                body.render = layers;
            }
            body.controllers = evaluated.controllers;
            body.clip_clocks = evaluated.clip_clocks;
            body.variables = evaluated.variables;
            body.scale = evaluated.scale;
            body.initialized = true;
            body.completed_tick = Some(tick);
            Ok(())
        }
        Err(error) => {
            body.previous.clone_from(&body.current);
            if !body.current.is_empty() {
                body.completed_tick = Some(tick);
            }
            Err(error)
        }
    };
    state.world_body = Some(body);
    status
}

impl ActorAnimationStore {
    pub(crate) fn set_local_body_enabled(&mut self, enabled: bool) -> bool {
        let changed = self.local_body_enabled != enabled;
        self.local_body_enabled = enabled;
        changed
    }

    pub(crate) fn world_body(&self, runtime_id: u64) -> Option<ActorRigSnapshot<'_>> {
        if !self.local_body_enabled {
            return None;
        }
        let lifetime = *self.runtime_to_lifetime.get(&runtime_id)?;
        let state = self.rigs.get(&lifetime)?;
        let mut rig = self.snapshot(lifetime, state)?;
        if state.fallback == EntityRigFallback::GeometryOnly {
            return Some(rig);
        }
        let body = state.world_body.as_ref()?;
        if body.current.is_empty() || body.completed_tick != Some(state.completed_tick) {
            return None;
        }
        rig.previous = &body.previous;
        rig.current = &body.current;
        rig.render = &body.render;
        rig.skin_layers = &body.skin_layers;
        if let Some(scale) = body.scale {
            rig.scale = scale[0];
            rig.axis_scale = [scale[1], scale[2], scale[3]];
        }
        let assets = if state.pack {
            self.pack.as_ref().map(|pack| pack.assets.as_ref())
        } else {
            self.assets.as_deref()
        };
        rig.animation_variables = ActorAnimationVariables::new(
            assets,
            &body.variables,
            state.completed_tick.saturating_sub(state.lifetime_epoch),
        );
        Some(rig)
    }
}

#[cfg(test)]
mod tests;
