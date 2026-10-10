//! Presentation sampling against frozen completed animation state.
use super::*;

pub(super) struct ClipHistory<'a> {
    pub clips: &'a [tick::WeightedClip],
    pub clocks: &'a super::super::clock::ClipClocks,
    pub controllers: &'a [ControllerState],
    pub journal: &'a tick::controller::ControllerJournal,
    pub server_effects: &'a evaluation::MolangEffects,
}

/// Samples weights and authored times once on scratch state without committing transitions or clocks.
/// Isolated swell endpoints keep ordinary motion clocks at their completed times.
pub(super) fn sample(
    evaluator: &evaluation::Evaluator<'_>,
    variables: &mut MolangVariables,
    state: &ActorRigState,
    history: ClipHistory<'_>,
    swelling: Option<&swell::SwellSampling>,
    sample_motion: bool,
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
            replay: swelling.is_some(),
            record: false,
        },
        budget,
    )?;
    if swelling.is_some() {
        server_effects.apply(variables)?;
    }
    clips.extend(
        previous
            .iter()
            .filter(|clip| clip.clock == super::super::clock::Basis::Lifetime)
            .copied(),
    );
    super::super::clock::sample(evaluator, clocks, &mut clips, budget)?;
    let mut sampled_times = BTreeMap::new();
    for weighted in &mut clips {
        let motion_time = sample_motion && motion::samples_time(evaluator.assets, weighted.clip);
        if (swelling.is_some() || motion_time)
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
        if !motion_time
            && !swelling
                .is_some_and(|sampling| sampling.samples_time(evaluator.assets, weighted.clip))
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
