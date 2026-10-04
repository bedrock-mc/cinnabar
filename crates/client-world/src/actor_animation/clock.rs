//! Clip time assignment before bone evaluation.
use super::{tick::WeightedClip, *};

#[derive(Clone, Copy, Debug)]
pub(super) struct ClipClock {
    pub(super) time: f32,
    pub(super) finished: bool,
}

pub(super) type ClipClocks = BTreeMap<(usize, u64), ClipClock>;

/// Evaluate authored clocks once, before any model samples the clips. Zero-weight clips
/// pause their clock; controller reentry discards clocks belonging to the previous state.
pub(super) fn prepare(
    evaluator: &Evaluator<'_>,
    variables: &mut MolangVariables,
    previous: Option<&ClipClocks>,
    controllers: &[ControllerState],
    clips: &mut [WeightedClip],
    budget: &mut EvalBudget<'_>,
) -> Result<ClipClocks, EvalError> {
    let mut clocks = previous.cloned().unwrap_or_default();
    clocks.retain(|(_, started), _| {
        *started == 0
            || controllers
                .iter()
                .any(|state| state.entered_tick == *started)
    });
    let mut updated = std::collections::BTreeSet::new();
    for weighted in clips {
        budget.charge_work()?;
        let clip = evaluator
            .assets
            .animation_clips()
            .get(weighted.clip)
            .ok_or(EvalError::Invalid)?;
        let key = (weighted.clip, weighted.started_tick);
        if weighted.weight < f32::EPSILON {
            continue;
        }
        let raw_time = if let Some(expression) = clip.anim_time_update {
            if !updated.insert(key) {
                weighted.time = clocks.get(&key).ok_or(EvalError::Invalid)?.time;
                continue;
            }
            let old_time = previous
                .and_then(|clocks| clocks.get(&key))
                .map_or(0.0, |c| c.time);
            let clock_evaluator = Evaluator {
                anim_time: Some(old_time),
                ..*evaluator
            };
            clock_evaluator.number(expression as usize, variables, 0.0, budget)?
        } else {
            let tick = evaluator.anim_tick.saturating_sub(weighted.started_tick);
            let alpha = evaluator
                .context
                .attachable
                .map_or(0.0, |input| input.frame_alpha);
            (tick as f32 + alpha) * ACTOR_TICK_DURATION.as_secs_f32()
        };
        if !raw_time.is_finite() {
            return Err(EvalError::Invalid);
        }
        let length = clip.length_seconds.get();
        let time = match clip.loop_mode {
            // Preserve the endpoint and the unbounded clock of expression-only clips.
            EntityAnimationLoop::Loop if length > 0.0 && raw_time > length => raw_time % length,
            EntityAnimationLoop::HoldOnLastFrame => raw_time.min(length),
            _ => raw_time,
        };
        weighted.time = time;
        if clip.anim_time_update.is_some() {
            let finished = raw_time >= length
                || previous
                    .and_then(|clocks| clocks.get(&key))
                    .is_some_and(|c| c.finished);
            clocks.insert(key, ClipClock { time, finished });
        }
    }
    Ok(clocks)
}
