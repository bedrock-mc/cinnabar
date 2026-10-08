//! Authored clip and controller weights in their declared order.
use super::*;

pub(in crate::actor_animation) struct Input<'a> {
    pub clocks: &'a super::super::clock::ClipClocks,
    pub geometry: usize,
    pub blink: Option<usize>,
    pub journal: &'a mut super::controller::ControllerJournal,
    pub replay: bool,
    pub record: bool,
}

/// Collects authored weights; a zero transition budget preserves every controller's completed state.
pub(in crate::actor_animation) fn select(
    evaluator: &Evaluator<'_>,
    variables: &mut MolangVariables,
    controllers: &mut [ControllerState],
    input: Input<'_>,
    budget: &mut EvalBudget<'_>,
) -> Result<Vec<WeightedClip>, EvalError> {
    let Input {
        clocks: clip_clocks,
        geometry: geometry_binding,
        blink: blink_controller,
        journal,
        replay,
        record,
    } = input;
    let assets = evaluator.assets;
    let mut weighted_clips = Vec::new();
    let candidate = assets
        .rig_geometries()
        .get(geometry_binding)
        .ok_or(EvalError::Invalid)?;
    let direct_first = candidate.first_animation as usize;
    let direct_end = direct_first
        .checked_add(candidate.animation_count as usize)
        .ok_or(EvalError::Invalid)?;
    let direct = assets
        .rig_animations()
        .get(direct_first..direct_end)
        .ok_or(EvalError::Invalid)?;
    let controller_first = candidate.first_controller as usize;
    let controller_end = controller_first
        .checked_add(candidate.controller_count as usize)
        .ok_or(EvalError::Invalid)?;
    let bound = assets
        .rig_controllers()
        .get(controller_first..controller_end)
        .ok_or(EvalError::Invalid)?;
    // Clips and controllers run interleaved in the authored `animate` order.
    let (mut next_clip, mut next_controller) = (0, 0);
    while next_clip < direct.len() || next_controller < bound.len() {
        budget.charge_work()?;
        let clip_first = match (direct.get(next_clip), bound.get(next_controller)) {
            (Some(clip), Some(controller)) => clip.order <= controller.order,
            (clip, _) => clip.is_some(),
        };
        if clip_first {
            let binding = &direct[next_clip];
            next_clip += 1;
            let weight = blend_weight(evaluator, variables, binding.weight, 1.0, budget)?;
            if weight != 0.0 {
                weighted_clips.push(WeightedClip {
                    clip: binding.clip as usize,
                    weight,
                    started_tick: 0,
                    clock: super::super::clock::Basis::Direct,
                    time: 0.0,
                    blend: None,
                });
            }
        } else {
            let reference = next_controller;
            let binding = &bound[next_controller];
            next_controller += 1;
            let weight = blend_weight(evaluator, variables, binding.weight, 1.0, budget)?;
            if weight != 0.0 {
                let mut walk = ControllerWalk {
                    evaluator,
                    variables,
                    controllers,
                    clip_clocks,
                    clips: &mut weighted_clips,
                    budget,
                    journal,
                    replay,
                    record,
                    reference,
                    path: [0; assets::MAX_ENTITY_CONTROLLER_NESTING],
                };
                walk.evaluate(binding.controller as usize, weight, 0)?;
            } else if replay {
                journal.replay_inactive(reference, &[], controllers, variables)?;
            }
        }
    }
    if let Some(controller) = blink_controller
        && !bound
            .iter()
            .any(|binding| binding.controller as usize == controller)
    {
        ControllerWalk {
            evaluator,
            variables,
            controllers,
            clip_clocks,
            clips: &mut weighted_clips,
            budget,
            journal,
            replay,
            record,
            reference: bound.len(),
            path: [0; assets::MAX_ENTITY_CONTROLLER_NESTING],
        }
        .evaluate(controller, 1.0, 0)?;
    }
    Ok(weighted_clips)
}
