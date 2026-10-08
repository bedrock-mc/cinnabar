//! Swing-weight sampling against frozen completed animation state.
use super::*;

/// Resamples weights on scratch controllers without committing transitions or advancing clip clocks.
pub(super) fn sample(
    evaluator: &evaluation::Evaluator<'_>,
    variables: &mut MolangVariables,
    state: &ActorRigState,
    previous: &[tick::WeightedClip],
    budget: &mut EvalBudget<'_>,
) -> Result<Vec<tick::WeightedClip>, EvalError> {
    let mut controllers = state.controllers.clone();
    let mut clips = tick::selection::select(
        evaluator,
        variables,
        &mut controllers,
        &state.clip_clocks,
        state.geometry_binding,
        super::super::skin_layers::blink_controller(evaluator.assets, state),
        budget,
    )?;
    super::super::clock::sample(evaluator, &state.clip_clocks, &mut clips, budget)?;
    for weighted in &mut clips {
        if let Some(old) = previous
            .iter()
            .find(|old| old.clip == weighted.clip && old.started_tick == weighted.started_tick)
        {
            weighted.time = old.time;
        }
    }
    Ok(clips)
}
