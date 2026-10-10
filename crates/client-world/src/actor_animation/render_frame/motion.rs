use super::*;

/// Samples continuous motion queries without advancing simulation, swing or item-use state.
/// The caller must reject frames older than the retained motion history.
pub(super) fn input(state: &ActorRigState, frame: &SwellMotion, alpha: f32) -> ActorTickInput {
    let current = frame.input;
    let Some(previous) = state.history.iter().rev().nth(1) else {
        return current;
    };
    let previous = tick::draw_input(*previous, &frame.context);
    let lerp = |from: f32, to: f32| from + (to - from) * alpha;
    let angle = |from: f32, to: f32| from + query::wrap_degrees(to - from) * alpha;
    // Raw position and action queries keep their completed tick values.
    ActorTickInput {
        body_yaw: angle(previous.body_yaw, current.body_yaw),
        yaw: angle(previous.yaw, current.yaw),
        head_yaw: angle(previous.head_yaw, current.head_yaw),
        pitch: angle(previous.pitch, current.pitch),
        distance_moved: current.distance_moved - state.motion.speed * (1.0 - alpha),
        move_speed: lerp(previous.move_speed, current.move_speed),
        walk_distance: current.walk_distance
            + (current.walk_distance - previous.walk_distance) * alpha,
        ..current
    }
}

/// Samples observed pitch independently of the first-person draw's normalized rotations.
pub(super) fn pitch(state: &ActorRigState, alpha: f32) -> f32 {
    let mut history = state.history.iter().rev();
    let current = history.next().map_or(0.0, |input| input.pitch);
    let previous = history.next().map_or(current, |input| input.pitch);
    previous + query::wrap_degrees(current - previous) * alpha
}

/// Whether an authored clock directly consumes a continuous motion query.
pub(super) fn samples_time(assets: &RuntimeEntityAssets, clip: usize) -> bool {
    let Some(expression) = assets.animation_clips()[clip].anim_time_update else {
        return false;
    };
    let expression = &assets.molang_expressions()[expression as usize];
    let first = expression.first_op as usize;
    assets.molang_ops()[first..first + usize::from(expression.op_count)]
        .iter()
        .any(|op| {
            let symbol = match op {
                MolangOp::LoadQuery(symbol) => *symbol,
                MolangOp::CallQuery(call) => call.symbol,
                _ => return false,
            };
            matches!(
                assets.molang_symbols()[symbol as usize].identifier.as_ref(),
                "query.modified_distance_moved"
                    | "query.modified_move_speed"
                    | "query.walk_distance"
            )
        })
}

/// Recomputes motion-driven authored clip times on scratch data, keeping retained clocks intact.
pub(super) fn clips(
    evaluator: &evaluation::Evaluator<'_>,
    variables: &mut MolangVariables,
    state: &ActorRigState,
    clips: &[tick::WeightedClip],
    budget: &mut EvalBudget<'_>,
) -> Result<Vec<tick::WeightedClip>, EvalError> {
    let mut clips = clips.to_vec();
    let mut times = BTreeMap::new();
    for clip in &mut clips {
        if clip.weight < f32::EPSILON || !samples_time(evaluator.assets, clip.clip) {
            continue;
        }
        let key = (clip.clip, clip.started_tick, clip.clock);
        if let Some(&time) = times.get(&key) {
            clip.time = time;
        } else {
            clock::sample_update(evaluator, variables, clip, &state.clip_clocks, budget)?;
            times.insert(key, clip.time);
        }
    }
    Ok(clips)
}
