use super::*;

/// Samples continuous motion queries without advancing simulation, swing or item-use state.
pub(super) fn input(state: &ActorRigState, current: ActorTickInput, alpha: f32) -> ActorTickInput {
    let Some(previous) = state.history.iter().rev().nth(1) else {
        return current;
    };
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
        let Some(expression) = evaluator.assets.animation_clips()[clip.clip].anim_time_update
        else {
            continue;
        };
        let expression = &evaluator.assets.molang_expressions()[expression as usize];
        let first = expression.first_op as usize;
        let motion_time = evaluator.assets.molang_ops()
            [first..first + usize::from(expression.op_count)]
            .iter()
            .any(|op| {
                let symbol = match op {
                    MolangOp::LoadQuery(symbol) => *symbol,
                    MolangOp::CallQuery(call) => call.symbol,
                    _ => return false,
                };
                matches!(
                    evaluator.assets.molang_symbols()[symbol as usize]
                        .identifier
                        .as_ref(),
                    "query.modified_distance_moved"
                        | "query.modified_move_speed"
                        | "query.walk_distance"
                )
            });
        if !motion_time || clip.weight < f32::EPSILON {
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
