//! Controller traversal and retained effects of completed transitions.
use super::*;

#[cfg(test)]
mod state_time_tests;

#[derive(Clone, Debug, Default)]
pub(in crate::actor_animation) struct ControllerJournal {
    events: Vec<ControllerEvent>,
    retain_states: bool,
}

#[derive(Clone, Debug)]
struct ControllerEvent {
    slot: usize,
    reference: usize,
    path: [usize; assets::MAX_ENTITY_CONTROLLER_NESTING],
    depth: usize,
    state: usize,
    runtime: ControllerState,
    effects: evaluation::MolangEffects,
}

impl ControllerJournal {
    pub(in crate::actor_animation) fn new(retain_states: bool) -> Self {
        Self {
            retain_states,
            ..Default::default()
        }
    }

    /// Completed transition effects survive presentation gates that skip drawing a subtree.
    pub(super) fn replay_inactive(
        &self,
        reference: usize,
        path: &[usize],
        controllers: &mut [ControllerState],
        variables: &mut MolangVariables,
    ) -> Result<(), EvalError> {
        for event in &self.events {
            if event.reference == reference
                && event.depth >= path.len()
                && &event.path[..path.len()] == path
            {
                event.effects.apply(variables)?;
                controllers[event.slot] = event.runtime;
            }
        }
        Ok(())
    }

    pub(in crate::actor_animation) fn apply(
        &self,
        variables: &mut MolangVariables,
    ) -> Result<(), EvalError> {
        for event in &self.events {
            event.effects.apply(variables)?;
        }
        Ok(())
    }
}

pub(super) struct ControllerWalk<'e, 'v, 'b, 'w> {
    pub(super) evaluator: &'e Evaluator<'e>,
    pub(super) variables: &'v mut MolangVariables,
    pub(super) controllers: &'v mut [ControllerState],
    pub(super) clip_clocks: &'v super::clock::ClipClocks,
    pub(super) clips: &'v mut Vec<WeightedClip>,
    pub(super) budget: &'b mut EvalBudget<'w>,
    pub(super) journal: &'v mut ControllerJournal,
    pub(super) replay: bool,
    pub(super) record: bool,
    pub(super) reference: usize,
    pub(super) path: [usize; assets::MAX_ENTITY_CONTROLLER_NESTING],
}

impl ControllerWalk<'_, '_, '_, '_> {
    pub(super) fn evaluate(
        &mut self,
        controller: usize,
        weight: f32,
        depth: usize,
    ) -> Result<(), EvalError> {
        if depth >= assets::MAX_ENTITY_CONTROLLER_NESTING {
            return Err(EvalError::Invalid);
        }
        let assets = self.evaluator.assets;
        let slot = self
            .controllers
            .iter()
            .position(|runtime| runtime.controller == controller)
            .ok_or(EvalError::Invalid)?;
        self.budget.charge_work()?;
        let state = self.advance(slot, depth)?;
        self.controllers[slot].active = true;
        let runtime = self.controllers[slot];
        if let Some((previous, started, began)) = runtime.blend_from {
            let definition = &assets.controllers()[controller];
            let previous = definition.first_state as usize + previous as usize;
            let source = &assets.controller_states()[previous];
            let elapsed = (self
                .evaluator
                .anim_tick
                .saturating_sub(runtime.entered_tick) as f32
                + self.frame_alpha()
                - began)
                .max(0.0)
                * ACTOR_TICK_DURATION.as_secs_f32();
            let amount = (elapsed / source.blend_transition.get()).clamp(0.0, 1.0);
            if amount < 1.0 {
                if source.blend_via_shortest_path {
                    let blend = |incoming| {
                        Some(ControllerBlend {
                            controller: slot,
                            incoming,
                            amount,
                        })
                    };
                    self.animations(previous, weight, depth, started, blend(false))?;
                    return self.animations(
                        state,
                        weight,
                        depth,
                        runtime.entered_tick,
                        blend(true),
                    );
                }
                // Other blends apply both weighted states straight onto the shared pose.
                self.animations(previous, weight * (1.0 - amount), depth, started, None)?;
                return self.animations(state, weight * amount, depth, runtime.entered_tick, None);
            }
            self.controllers[slot].blend_from = None;
        }
        self.animations(state, weight, depth, runtime.entered_tick, None)
    }

    fn frame_alpha(&self) -> f32 {
        self.evaluator
            .context
            .attachable
            .map_or(0.0, |input| input.frame_alpha)
    }

    /// Keeps outgoing and incoming clip channels together before composing the bone hierarchy.
    fn animations(
        &mut self,
        state: usize,
        weight: f32,
        depth: usize,
        started_tick: u64,
        blend: Option<ControllerBlend>,
    ) -> Result<(), EvalError> {
        let evaluator = self.evaluator.for_controller_state(started_tick);
        let first = self.evaluator.assets.controller_states()[state].first_animation as usize;
        for (index, animation) in state_animations(self.evaluator.assets, state)?
            .iter()
            .enumerate()
        {
            self.budget.charge_work()?;
            let weight = blend_weight(
                &evaluator,
                self.variables,
                animation.weight,
                weight,
                self.budget,
            )?;
            if weight == 0.0 {
                if self.replay
                    && matches!(
                        animation.target,
                        EntityControllerAnimationTarget::Controller(_)
                    )
                {
                    self.path[depth] = first + index;
                    self.journal.replay_inactive(
                        self.reference,
                        &self.path[..depth + 1],
                        self.controllers,
                        self.variables,
                    )?;
                }
                continue;
            }
            match animation.target {
                EntityControllerAnimationTarget::Clip(clip) => self.clips.push(WeightedClip {
                    clip: clip as usize,
                    weight,
                    started_tick,
                    clock: super::clock::Basis::Controller,
                    time: 0.0,
                    blend,
                }),
                EntityControllerAnimationTarget::Controller(nested) => {
                    self.path[depth] = first + index;
                    self.evaluate(nested as usize, weight, depth + 1)?
                }
            }
        }
        Ok(())
    }
}

fn state_animations(
    assets: &RuntimeEntityAssets,
    state: usize,
) -> Result<&[assets::EntityControllerAnimation], EvalError> {
    {
        let state = assets
            .controller_states()
            .get(state)
            .ok_or(EvalError::Invalid)?;
        let first = state.first_animation as usize;
        let end = first
            .checked_add(state.animation_count as usize)
            .ok_or(EvalError::Invalid)?;
        assets
            .controller_animations()
            .get(first..end)
            .ok_or(EvalError::Invalid)
    }
}

impl ControllerWalk<'_, '_, '_, '_> {
    /// Whether every and any clip of a state has played through once since it was entered.
    fn finished(&self, state: usize, entered_tick: u64) -> Result<(bool, bool), EvalError> {
        let elapsed = self.evaluator.anim_tick.saturating_sub(entered_tick) as f32
            * ACTOR_TICK_DURATION.as_secs_f32();
        let mut all = true;
        let mut any = false;
        for animation in state_animations(self.evaluator.assets, state)? {
            let EntityControllerAnimationTarget::Clip(index) = animation.target else {
                continue;
            };
            let clip = self
                .evaluator
                .assets
                .animation_clips()
                .get(index as usize)
                .ok_or(EvalError::Invalid)?;
            let done = if clip.anim_time_update.is_some() {
                self.clip_clocks
                    .get(&(
                        index as usize,
                        entered_tick,
                        super::clock::Basis::Controller,
                    ))
                    .is_some_and(|clock| clock.finished)
            } else {
                elapsed >= clip.length_seconds.get()
            };
            all &= done;
            any |= done;
        }
        Ok((all, any))
    }

    /// Takes at most the bounded number of transitions; returns the absolute state index.
    fn advance(&mut self, slot: usize, depth: usize) -> Result<usize, EvalError> {
        if self.replay {
            if let Some(event) = self.journal.events.iter().find(|event| {
                event.slot == slot
                    && event.reference == self.reference
                    && event.depth == depth
                    && event.path[..depth] == self.path[..depth]
            }) {
                event.effects.apply(self.variables)?;
                self.controllers[slot] = event.runtime;
                return Ok(event.state);
            }
            let runtime = self.controllers[slot];
            return Ok(
                self.evaluator.assets.controllers()[runtime.controller].first_state as usize
                    + runtime.state as usize,
            );
        }
        if !self.record {
            return self.advance_tick(slot);
        }
        let capture = self.variables.begin_effects();
        let state = self.advance_tick(slot);
        let effects = self.variables.finish_effects(capture);
        let state = state?;
        if self.journal.retain_states || !effects.is_empty() {
            self.journal.events.push(ControllerEvent {
                slot,
                reference: self.reference,
                path: self.path,
                depth,
                state,
                runtime: self.controllers[slot],
                effects,
            });
        }
        Ok(state)
    }

    fn advance_tick(&mut self, slot: usize) -> Result<usize, EvalError> {
        let assets = self.evaluator.assets;
        let runtime = self.controllers[slot];
        let controller = assets
            .controllers()
            .get(runtime.controller)
            .ok_or(EvalError::Invalid)?;
        let (mut current, mut entered_tick) = (runtime.state, runtime.entered_tick);
        loop {
            if current >= controller.state_count {
                return Err(EvalError::Invalid);
            }
            let state_index = controller.first_state as usize + current as usize;
            let state = assets
                .controller_states()
                .get(state_index)
                .ok_or(EvalError::Invalid)?;
            let first = state.first_transition as usize;
            let end = first
                .checked_add(state.transition_count as usize)
                .ok_or(EvalError::Invalid)?;
            let transitions = assets
                .controller_transitions()
                .get(first..end)
                .ok_or(EvalError::Invalid)?;
            let evaluator = Evaluator {
                finished: if transitions.is_empty() {
                    (false, false)
                } else {
                    self.finished(state_index, entered_tick)?
                },
                ..self.evaluator.for_controller_state(entered_tick)
            };
            let mut target = None;
            for transition in transitions {
                self.budget.charge_work()?;
                let condition = evaluator.run(
                    transition.condition as usize,
                    self.variables,
                    0.0,
                    self.budget,
                )?;
                if condition.truthy() {
                    target = Some(transition.target_state);
                    break;
                }
            }
            let Some(target) = target.filter(|_| self.budget.take_transition()) else {
                self.controllers[slot].state = current;
                self.controllers[slot].entered_tick = entered_tick;
                return Ok(state_index);
            };
            if target >= controller.state_count {
                return Err(EvalError::Invalid);
            }
            if let Some(script) = state.on_exit {
                evaluator.run(script as usize, self.variables, 0.0, self.budget)?;
            }
            let worn = self
                .evaluator
                .context
                .attachable
                .is_some_and(|input| input.worn);
            let single_clip = |index| {
                state_animations(assets, index).is_ok_and(|animations| {
                    matches!(
                        animations,
                        [assets::EntityControllerAnimation {
                            target: EntityControllerAnimationTarget::Clip(_),
                            ..
                        }]
                    )
                })
            };
            self.controllers[slot].blend_from = (worn
                && state.blend_transition.get() > 0.0
                && single_clip(state_index)
                && single_clip(controller.first_state as usize + target as usize))
            .then_some((current, entered_tick, self.frame_alpha()));
            current = target;
            entered_tick = self.evaluator.anim_tick;
            let entered = assets
                .controller_states()
                .get(controller.first_state as usize + target as usize)
                .ok_or(EvalError::Invalid)?;
            if let Some(script) = entered.on_entry {
                let evaluator = Evaluator {
                    state_time: 0.0,
                    ..evaluator
                };
                evaluator.run(script as usize, self.variables, 0.0, self.budget)?;
            }
            if worn {
                self.controllers[slot].state = current;
                self.controllers[slot].entered_tick = entered_tick;
                return Ok(controller.first_state as usize + current as usize);
            }
        }
    }
}
