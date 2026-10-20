use super::*;

/// Ordinary inputs stay at their tick endpoints while selected motion queries sample the frame.
#[derive(Clone, Debug, Default)]
pub(crate) struct MotionEndpointState {
    pub variables: MolangVariables,
    pub scratch: evaluation::variables::frame::FrameVariables,
    pub sampling: Option<Arc<super::motion_sampling::MotionSampling>>,
    pub queries: Vec<(u32, MolangValue)>,
    pub actor: Option<Arc<dyn AnimatedActor>>,
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

pub(super) struct MotionEndpoint<'a> {
    pub evaluator: evaluation::Evaluator<'a>,
    pub variables: evaluation::variables::frame::FrameVariableLease<'a>,
    pub clips: Cow<'a, [tick::WeightedClip]>,
    pub scale: Option<[f32; 4]>,
}

impl MotionEndpointState {
    /// Replays an endpoint with frame rotation, swelling and swing, retaining its other inputs.
    /// A `camera` context replaces the tick's camera and resamples camera-weighted clips.
    pub(super) fn sample<'a>(
        &'a self,
        base: evaluation::Evaluator<'a>,
        state: &ActorRigState,
        (amount, swing): (f32, Option<f32>),
        camera: Option<&'a ActorTickContext>,
        publish: bool,
        budget: &mut EvalBudget<'_>,
    ) -> Result<MotionEndpoint<'a>, EvalError> {
        let evaluator = evaluation::Evaluator {
            context: camera.unwrap_or(&self.context),
            anim_tick: self.anim_tick,
            life_tick: self.life_tick,
            swell_amount: Some(amount),
            presentation_alpha: Some(base.context.frame_alpha),
            query_history: Some(&self.queries),
            actor: match self.actor.as_deref() {
                Some(actor) => actor,
                None => base.actor,
            },
            ..base
        };
        let mut variables = self.scratch.lease(&self.variables);
        if let Some(swing) = swing {
            variables.set(evaluator.layout.engine.attack_time, swing);
        }
        // Replay held-expression effects before this endpoint's downstream scripts.
        tick::scale::evaluate_held_item(
            &evaluator,
            &evaluator.assets.rig_bindings()[state.rig_binding],
            &mut variables,
            budget,
        )?;
        if let Some(script) = evaluator.assets.rig_bindings()[state.rig_binding].pre_animation {
            evaluator.run(script as usize, &mut variables, 0.0, budget)?;
        }
        if state.complete_spear_variables {
            evaluator.layout.engine.spear.apply(
                &mut variables,
                evaluator.actor,
                evaluator.context,
                evaluator.input,
                swing.unwrap_or(evaluator.input.attack_time),
            );
        }
        if publish {
            variables.capture_writes();
        }
        let scale = tick::scale::evaluate_model(
            &evaluator,
            &evaluator.assets.rig_bindings()[state.rig_binding],
            &mut variables,
            budget,
        )?;
        tick::set_item_rotation_factor(&evaluator.layout.engine, &mut variables);
        let clips = if camera.is_some() || self.sampling.as_ref().is_some_and(|s| s.samples_clips())
        {
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
                false,
                budget,
            )?
            .into()
        } else {
            self.journal.apply(&mut variables)?;
            self.server_effects.apply(&mut variables)?;
            Cow::Borrowed(self.clips.as_slice())
        };
        Ok(MotionEndpoint {
            evaluator,
            variables,
            clips,
            scale,
        })
    }
}
