//! Presentation sampling against frozen completed animation state.
use super::*;

/// Resamples weights on scratch controllers without committing transitions or advancing clip clocks.
pub(super) fn sample(
    evaluator: &evaluation::Evaluator<'_>,
    variables: &mut MolangVariables,
    state: &ActorRigState,
    previous: &[tick::WeightedClip],
    clocks: &super::super::clock::ClipClocks,
    swelling: Option<&swell::SwellSampling>,
    budget: &mut EvalBudget<'_>,
) -> Result<Vec<tick::WeightedClip>, EvalError> {
    let mut controllers = state.controllers.clone();
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
