use super::*;

/// Ordinary inputs stay at their motion endpoints while the swell query samples the frame.
#[derive(Clone, Debug, Default)]
pub(in crate::actor_animation) struct SwellMotion {
    pub variables: MolangVariables,
    pub sampling: Option<Arc<super::swell::SwellSampling>>,
    pub queries: Vec<(u32, MolangValue)>,
    pub actor: Option<Arc<ActorSnapshot>>,
    pub context: ActorTickContext,
    pub input: ActorTickInput,
    pub anim_tick: u64,
    pub life_tick: u64,
    pub clips: Vec<tick::WeightedClip>,
    pub clocks: super::super::clock::ClipClocks,
    pub controllers: Vec<ControllerState>,
    pub journal: tick::controller::ControllerJournal,
    pub server_effects: evaluation::MolangEffects,
}

pub(super) struct SwellEndpoint<'a> {
    pub evaluator: evaluation::Evaluator<'a>,
    pub variables: MolangVariables,
    pub clips: Vec<tick::WeightedClip>,
    pub scale: Option<[f32; 4]>,
}

impl SwellMotion {
    pub(super) fn sample<'a>(
        &'a self,
        base: evaluation::Evaluator<'a>,
        state: &ActorRigState,
        amount: f32,
        publish: bool,
        budget: &mut EvalBudget<'_>,
    ) -> Result<SwellEndpoint<'a>, EvalError> {
        let evaluator = evaluation::Evaluator {
            input: &self.input,
            context: &self.context,
            anim_tick: self.anim_tick,
            life_tick: self.life_tick,
            swell_amount: Some(amount),
            presentation_alpha: Some(base.context.frame_alpha),
            query_history: Some(&self.queries),
            actor: self.actor.as_deref().unwrap_or(base.actor),
            ..base
        };
        let mut variables = self.variables.clone();
        if let Some(script) = evaluator.assets.rig_bindings()[state.rig_binding].pre_animation {
            evaluator.run(script as usize, &mut variables, 0.0, budget)?;
        }
        if publish {
            variables.capture_writes();
        }
        let scale = tick::evaluate_scale(
            &evaluator,
            &evaluator.assets.rig_bindings()[state.rig_binding],
            &mut variables,
            budget,
        )?;
        tick::set_item_rotation_factor(&evaluator.layout.engine, &mut variables);
        let clips = if self.sampling.as_ref().is_some_and(|s| s.samples_clips()) {
            clips::sample(
                &evaluator,
                &mut variables,
                state,
                clips::ClipHistory {
                    clips: &self.clips,
                    clocks: &self.clocks,
                    controllers: &self.controllers,
                    journal: &self.journal,
                    server_effects: &self.server_effects,
                },
                self.sampling.as_deref(),
                budget,
            )?
        } else {
            self.journal.apply(&mut variables)?;
            self.server_effects.apply(&mut variables)?;
            self.clips.clone()
        };
        Ok(SwellEndpoint {
            evaluator,
            variables,
            clips,
            scale,
        })
    }
}
