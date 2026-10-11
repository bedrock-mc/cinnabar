//! Presentation sampling against frozen completed animation state.
use super::*;

pub(super) struct ClipHistory<'a> {
    pub clips: &'a [tick::WeightedClip],
    pub clocks: &'a super::super::clock::ClipClocks,
    pub controllers: &'a [ControllerState],
    pub journal: &'a tick::controller::ControllerJournal,
    pub server_effects: &'a evaluation::MolangEffects,
}

/// Resamples weights on scratch controllers without committing transitions or advancing clip clocks.
pub(super) fn sample(
    evaluator: &evaluation::Evaluator<'_>,
    variables: &mut MolangVariables,
    state: &ActorRigState,
    history: ClipHistory<'_>,
    swelling: Option<&swell::SwellSampling>,
    presentation: bool,
    budget: &mut EvalBudget<'_>,
) -> Result<Vec<tick::WeightedClip>, EvalError> {
    let ClipHistory {
        clips: previous,
        clocks,
        controllers,
        journal,
        server_effects,
    } = history;
    let mut journal = journal.clone();
    let mut controllers = controllers.to_vec();
    let mut clips = tick::selection::select(
        evaluator,
        variables,
        &mut controllers,
        tick::selection::Input {
            clocks,
            geometry: state.geometry_binding,
            blink: super::super::skin_layers::blink_controller(evaluator.assets, state),
            journal: &mut journal,
            replay: true,
            record: false,
        },
        budget,
    )?;
    server_effects.apply(variables)?;
    clips.extend(
        previous
            .iter()
            .filter(|clip| clip.clock == super::super::clock::Basis::Lifetime)
            .copied(),
    );
    super::super::clock::sample(evaluator, clocks, &mut clips, budget)?;
    let mut sampled_times = BTreeMap::new();
    for weighted in &mut clips {
        if (swelling.is_some() || presentation)
            && weighted.weight >= f32::EPSILON
            && evaluator.assets.animation_clips()[weighted.clip]
                .anim_time_update
                .is_some()
        {
            let key = (weighted.clip, weighted.started_tick, weighted.clock);
            if let Some(&time) = sampled_times.get(&key) {
                weighted.time = time;
            } else {
                super::super::clock::sample_update(evaluator, variables, weighted, clocks, budget)?;
                sampled_times.insert(key, weighted.time);
            }
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
