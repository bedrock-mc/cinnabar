use super::*;

/// Ordinary inputs stay at their motion endpoints while the swell query samples the frame.
#[derive(Clone, Debug, Default)]
pub(in crate::actor_animation) struct SwellMotion {
    pub variables: MolangVariables,
    pub context: ActorTickContext,
    pub input: ActorTickInput,
    pub anim_tick: u64,
    pub life_tick: u64,
    pub clips: Vec<tick::WeightedClip>,
    pub clocks: super::super::clock::ClipClocks,
}

pub(super) struct SwellEndpoint<'a> {
    pub evaluator: evaluation::Evaluator<'a>,
    pub variables: MolangVariables,
    pub clips: Vec<tick::WeightedClip>,
}

impl SwellMotion {
    pub(super) fn sample<'a>(
        &'a self,
        base: evaluation::Evaluator<'a>,
        state: &ActorRigState,
        amount: f32,
        budget: &mut EvalBudget<'_>,
    ) -> Result<SwellEndpoint<'a>, EvalError> {
        let evaluator = evaluation::Evaluator {
            input: &self.input,
            context: &self.context,
            anim_tick: self.anim_tick,
            life_tick: self.life_tick,
            swell_amount: Some(amount),
            ..base
        };
        let mut variables = self.variables.clone();
        if let Some(script) = evaluator.assets.rig_bindings()[state.rig_binding].pre_animation {
            evaluator.run(script as usize, &mut variables, 0.0, budget)?;
        }
        tick::set_item_rotation_factor(&evaluator.layout.engine, &mut variables);
        let clips = if state
            .swell_sampling
            .as_ref()
            .is_some_and(|s| s.samples_clips())
        {
            clips::sample(
                &evaluator,
                &mut variables,
                state,
                &self.clips,
                &self.clocks,
                state.swell_sampling.as_deref(),
                budget,
            )?
        } else {
            self.clips.clone()
        };
        Ok(SwellEndpoint {
            evaluator,
            variables,
            clips,
        })
    }
}
