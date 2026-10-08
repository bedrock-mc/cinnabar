//! Clip time assignment before bone evaluation.
use super::{tick::WeightedClip, *};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Basis {
    Direct,
    Controller,
    Lifetime,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ClipClock {
    pub(super) time: f32,
    pub(super) finished: bool,
    sampled: Option<(u64, u32)>,
    active: bool,
}

pub(super) type ClipClocks = BTreeMap<(usize, u64, Basis), ClipClock>;

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
    clocks.retain(|(clip, started, basis), clock| {
        if *basis == Basis::Lifetime {
            return clips.iter().any(|weighted| {
                weighted.clock == Basis::Lifetime
                    && weighted.clip == *clip
                    && weighted.started_tick == *started
            });
        }
        if *basis == Basis::Direct {
            clock.active = false;
            return true;
        }
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
        let key = (weighted.clip, weighted.started_tick, weighted.clock);
        if weighted.weight < f32::EPSILON {
            continue;
        }
        let retained = clip.anim_time_update.is_some() || weighted.clock == Basis::Direct;
        if retained && !updated.insert(key) {
            weighted.time = clocks.get(&key).ok_or(EvalError::Invalid)?.time;
            continue;
        }
        let old = previous.and_then(|clocks| clocks.get(&key));
        let stamp = (
            evaluator.anim_tick,
            evaluator
                .context
                .attachable
                .map_or(0.0, |input| input.frame_alpha)
                .to_bits(),
        );
        let raw_time = if let Some(expression) = clip.anim_time_update {
            let old_time = old.map_or(0.0, |c| c.time);
            let clock_evaluator = Evaluator {
                anim_time: Some(old_time),
                ..*evaluator
            };
            clock_evaluator.number(expression as usize, variables, 0.0, budget)?
        } else if weighted.clock == Basis::Direct {
            let delta = direct_delta(evaluator, old, stamp);
            old.map_or(0.0, |clock| clock.time) + delta
        } else {
            elapsed_time(evaluator, weighted.started_tick, weighted.clock)
        };
        if !raw_time.is_finite() {
            return Err(EvalError::Invalid);
        }
        let length = clip.length_seconds.get();
        let time = wrapped_time(clip, raw_time);
        weighted.time = time;
        if retained {
            let finished = raw_time >= length || old.is_some_and(|c| c.finished);
            clocks.insert(
                key,
                ClipClock {
                    time,
                    finished,
                    sampled: Some(stamp),
                    active: true,
                },
            );
        }
    }
    Ok(clocks)
}

fn direct_delta(evaluator: &Evaluator<'_>, old: Option<&ClipClock>, stamp: (u64, u32)) -> f32 {
    if let Some(delta) = evaluator
        .context
        .attachable
        .and_then(|input| input.delta_seconds)
    {
        return delta;
    }
    if let Some(clock) = old.filter(|clock| clock.active)
        && let Some((tick, alpha)) = clock.sampled
    {
        return (stamp.0.saturating_sub(tick) as f32 + f32::from_bits(stamp.1)
            - f32::from_bits(alpha))
        .max(0.0)
            * ACTOR_TICK_DURATION.as_secs_f32();
    }
    query::delta_time(evaluator.context)
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
        weighted.time = if let Some(clock) =
            clocks.get(&(weighted.clip, weighted.started_tick, weighted.clock))
        {
            clock.time
        } else {
            let time = if clip.anim_time_update.is_some() || weighted.clock == Basis::Direct {
                0.0
            } else {
                elapsed_time(evaluator, weighted.started_tick, weighted.clock)
            };
            wrapped_time(clip, time)
        };
    }
    Ok(())
}

/// Evaluates a presentation-dependent clock update on scratch clip data only.
pub(super) fn sample_update(
    evaluator: &Evaluator<'_>,
    variables: &mut MolangVariables,
    weighted: &mut WeightedClip,
    budget: &mut EvalBudget<'_>,
) -> Result<(), EvalError> {
    let clip = evaluator
        .assets
        .animation_clips()
        .get(weighted.clip)
        .ok_or(EvalError::Invalid)?;
    if let Some(expression) = clip.anim_time_update {
        let evaluator = Evaluator {
            anim_time: Some(weighted.time),
            ..*evaluator
        };
        let time = evaluator.number(expression as usize, variables, 0.0, budget)?;
        if !time.is_finite() {
            return Err(EvalError::Invalid);
        }
        weighted.time = wrapped_time(clip, time);
    }
    Ok(())
}

/// Ordinary clips derive their fixed-step time from their controller's entry tick.
fn elapsed_time(evaluator: &Evaluator<'_>, started_tick: u64, basis: Basis) -> f32 {
    let tick = (if basis == Basis::Lifetime {
        evaluator.life_tick
    } else {
        evaluator.anim_tick
    })
    .saturating_sub(started_tick);
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
