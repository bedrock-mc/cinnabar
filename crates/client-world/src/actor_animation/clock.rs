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
            || controllers.iter().any(|state| {
                state.entered_tick == *started
                    || state
                        .blend_from
                        .is_some_and(|(_, tick, _)| tick == *started)
            })
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
            elapsed_time(evaluator, weighted.started_tick)
        };
        if !raw_time.is_finite() {
            return Err(EvalError::Invalid);
        }
        let length = clip.length_seconds.get();
        let time = wrapped_time(clip, raw_time);
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

/// Samples completed clip times without evaluating authored updates or changing retained clocks.
pub(super) fn sample(
    evaluator: &Evaluator<'_>,
    clocks: &ClipClocks,
    clips: &mut [WeightedClip],
    budget: &mut EvalBudget<'_>,
) -> Result<(), EvalError> {
    for weighted in clips {
        budget.charge_work()?;
        let clip = evaluator
            .assets
            .animation_clips()
            .get(weighted.clip)
            .ok_or(EvalError::Invalid)?;
        weighted.time = if let Some(clock) = clocks.get(&(weighted.clip, weighted.started_tick)) {
            clock.time
        } else {
            let time = if clip.anim_time_update.is_some() {
                0.0
            } else {
                elapsed_time(evaluator, weighted.started_tick)
            };
            wrapped_time(clip, time)
        };
    }
    Ok(())
}

/// Ordinary clips derive their fixed-step time from their controller's entry tick.
fn elapsed_time(evaluator: &Evaluator<'_>, started_tick: u64) -> f32 {
    let tick = evaluator.anim_tick.saturating_sub(started_tick);
    let alpha = evaluator
        .context
        .attachable
        .map_or(0.0, |input| input.frame_alpha);
    (tick as f32 + alpha) * ACTOR_TICK_DURATION.as_secs_f32()
}

/// Looping preserves the endpoint; held clips stop at their authored length.
fn wrapped_time(clip: &assets::EntityAnimationClip, time: f32) -> f32 {
    let length = clip.length_seconds.get();
    match clip.loop_mode {
        EntityAnimationLoop::Loop if length > 0.0 && time > length => time % length,
        EntityAnimationLoop::HoldOnLastFrame => time.min(length),
        _ => time,
    }
}
