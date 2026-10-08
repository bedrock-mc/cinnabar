//! Presentation sampling against frozen completed animation state.
use super::*;

pub(super) struct ClipHistory<'a> {
    pub clips: &'a [tick::WeightedClip],
    pub clocks: &'a super::super::clock::ClipClocks,
    pub controllers: &'a [ControllerState],
}

/// Resamples weights on scratch controllers without committing transitions or advancing clip clocks.
pub(super) fn sample(
    evaluator: &evaluation::Evaluator<'_>,
    variables: &mut MolangVariables,
    state: &ActorRigState,
    history: ClipHistory<'_>,
    swelling: Option<&swell::SwellSampling>,
    budget: &mut EvalBudget<'_>,
) -> Result<Vec<tick::WeightedClip>, EvalError> {
    let ClipHistory {
        clips: previous,
        clocks,
        controllers,
    } = history;
    let mut controllers = controllers.to_vec();
    let mut clips = tick::selection::select(
        evaluator,
        variables,
        &mut controllers,
        clocks,
        state.geometry_binding,
        super::super::skin_layers::blink_controller(evaluator.assets, state),
        budget,
    )?;
    clips.extend(
        previous
            .iter()
            .filter(|clip| clip.clock == super::super::clock::Basis::Lifetime)
            .copied(),
    );
    super::super::clock::sample(evaluator, clocks, &mut clips, budget)?;
    for weighted in &mut clips {
        if swelling.is_some_and(|sampling| sampling.samples_time(evaluator.assets, weighted.clip)) {
            super::super::clock::sample_update(evaluator, variables, weighted, clocks, budget)?;
        }
        if !swelling.is_some_and(|sampling| sampling.samples_time(evaluator.assets, weighted.clip))
            && let Some(old) = previous.iter().find(|old| {
                old.clip == weighted.clip
                    && old.started_tick == weighted.started_tick
                    && old.clock == weighted.clock
            })
        {
            weighted.time = old.time;
        }
    }
    Ok(clips)
}
