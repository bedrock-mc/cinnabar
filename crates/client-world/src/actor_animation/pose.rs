use super::evaluation::{
    catmull, compose_pose, evaluate_expression, evaluate_expression_with_this, lerp3, truthy,
};
use super::*;

pub(super) fn evaluate_state(
    assets: &RuntimeEntityAssets,
    state: &mut ActorRigState,
    actor: &ActorSnapshot,
    action: ActorActionInput,
    items: ActorItemInput,
    riding_y_offset: f32,
    tick: u64,
    budget: &mut EvalBudget<'_>,
) -> Result<EvaluatedState, EvalError> {
    let animation_tick = if state.reset_pending {
        0
    } else {
        tick.saturating_sub(state.animation_epoch)
    };
    let life_tick = tick.saturating_sub(state.lifetime_epoch);
    let mut history = if state.reset_pending {
        VecDeque::with_capacity(MAX_ACTOR_ACTION_HISTORY)
    } else {
        state.history.clone()
    };
    if history.len() == MAX_ACTOR_ACTION_HISTORY {
        history.pop_front();
    }
    let hand_bob_target = if actor.on_ground.unwrap_or(false) {
        (actor.velocity[0].hypot(actor.velocity[2]) * 0.05).clamp(0.0, 0.1)
    } else {
        0.0
    };
    if state.reset_pending {
        state.hand_bob = 0.0;
    } else {
        state.hand_bob += (hand_bob_target - state.hand_bob) * 0.02;
    }
    history.push_back(ActorTickInput {
        velocity: actor.velocity,
        on_ground: actor.on_ground.unwrap_or(false),
        body_yaw: actor.body_yaw,
        head_yaw: actor.head_yaw,
        pitch: actor.pitch,
        action,
        items,
        distance_moved: state.distance_moved,
        hand_bob: state.hand_bob,
        riding_y_offset,
        default_bone_pivots: default_bone_pivots(&state.bones),
        root_locator_offset: root_locator_offset(&state.bones),
    });
    let mut controllers = state.controllers.clone();
    if state.reset_pending {
        for runtime in &mut controllers {
            runtime.state = assets
                .controllers()
                .get(runtime.controller)
                .ok_or(EvalError::Invalid)?
                .initial_state;
        }
    }
    let mut weighted_clips = Vec::new();
    let candidate = assets
        .rig_geometries()
        .get(state.geometry_binding)
        .ok_or(EvalError::Invalid)?;
    let direct_first = candidate.first_animation as usize;
    let direct_end = direct_first
        .checked_add(candidate.animation_count as usize)
        .ok_or(EvalError::Invalid)?;
    for binding in assets
        .rig_animations()
        .get(direct_first..direct_end)
        .ok_or(EvalError::Invalid)?
    {
        budget.charge_work()?;
        weighted_clips.push((binding.clip as usize, 1.0));
    }
    for runtime in &mut controllers {
        budget.charge_work()?;
        advance_controller(
            assets,
            runtime,
            actor,
            &history,
            animation_tick,
            life_tick,
            budget,
        )?;
        let controller = assets
            .controllers()
            .get(runtime.controller)
            .ok_or(EvalError::Invalid)?;
        if runtime.state >= controller.state_count {
            return Err(EvalError::Invalid);
        }
        let state_index = controller.first_state as usize + runtime.state as usize;
        let controller_state = assets
            .controller_states()
            .get(state_index)
            .ok_or(EvalError::Invalid)?;
        let first = controller_state.first_animation as usize;
        let end = first
            .checked_add(controller_state.animation_count as usize)
            .ok_or(EvalError::Invalid)?;
        for animation in assets
            .controller_animations()
            .get(first..end)
            .ok_or(EvalError::Invalid)?
        {
            budget.charge_work()?;
            let weight = animation.weight.map_or(Ok(1.0), |expression| {
                evaluate_expression(
                    assets,
                    expression as usize,
                    actor,
                    &history,
                    animation_tick,
                    life_tick,
                    budget,
                )
            })?;
            if weight.is_finite() && weight != 0.0 {
                weighted_clips.push((animation.clip as usize, weight));
            }
        }
    }
    let local = sample_clips(
        assets,
        state.bones.len(),
        &weighted_clips,
        actor,
        &history,
        animation_tick,
        life_tick,
        budget,
    )?;
    compose_pose(&state.bones, &local)
        .map(|pose| EvaluatedState {
            pose,
            controllers,
            history,
        })
        .ok_or(EvalError::Invalid)
}

fn advance_controller(
    assets: &RuntimeEntityAssets,
    runtime: &mut ControllerState,
    actor: &ActorSnapshot,
    history: &VecDeque<ActorTickInput>,
    tick: u64,
    life_tick: u64,
    budget: &mut EvalBudget<'_>,
) -> Result<(), EvalError> {
    let controller = assets
        .controllers()
        .get(runtime.controller)
        .ok_or(EvalError::Invalid)?;
    loop {
        if runtime.state >= controller.state_count {
            return Err(EvalError::Invalid);
        }
        let state_index = controller.first_state as usize + runtime.state as usize;
        let state = assets
            .controller_states()
            .get(state_index)
            .ok_or(EvalError::Invalid)?;
        let first = state.first_transition as usize;
        let end = first
            .checked_add(state.transition_count as usize)
            .ok_or(EvalError::Invalid)?;
        let mut target = None;
        for transition in assets
            .controller_transitions()
            .get(first..end)
            .ok_or(EvalError::Invalid)?
        {
            budget.charge_work()?;
            let condition = evaluate_expression(
                assets,
                transition.condition as usize,
                actor,
                history,
                tick,
                life_tick,
                budget,
            )?;
            if truthy(condition) {
                target = Some(transition.target_state);
                break;
            }
        }
        let Some(target) = target else {
            return Ok(());
        };
        if !budget.take_transition() {
            return Ok(());
        }
        if target >= controller.state_count {
            return Err(EvalError::Invalid);
        }
        if let Some(expression) = state.on_exit {
            evaluate_expression(
                assets,
                expression as usize,
                actor,
                history,
                tick,
                life_tick,
                budget,
            )?;
        }
        runtime.state = target;
        let target_state = assets
            .controller_states()
            .get(controller.first_state as usize + target as usize)
            .ok_or(EvalError::Invalid)?;
        if let Some(expression) = target_state.on_entry {
            evaluate_expression(
                assets,
                expression as usize,
                actor,
                history,
                tick,
                life_tick,
                budget,
            )?;
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct LocalDelta {
    pub(crate) translation: [f32; 3],
    pub(crate) rotation: [f32; 3],
    pub(crate) scale: [f32; 3],
}

impl Default for LocalDelta {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
        }
    }
}

fn sample_clips(
    assets: &RuntimeEntityAssets,
    bone_count: usize,
    clips: &[(usize, f32)],
    actor: &ActorSnapshot,
    history: &VecDeque<ActorTickInput>,
    tick: u64,
    life_tick: u64,
    budget: &mut EvalBudget<'_>,
) -> Result<Vec<LocalDelta>, EvalError> {
    let mut local = vec![LocalDelta::default(); bone_count];
    for &(clip_index, weight) in clips {
        budget.charge_work()?;
        let clip = assets
            .animation_clips()
            .get(clip_index)
            .ok_or(EvalError::Invalid)?;
        let length = clip.length_seconds.get();
        let raw_time = clip
            .time_expression
            .map(|expression| {
                evaluate_expression(
                    assets,
                    expression as usize,
                    actor,
                    history,
                    tick,
                    life_tick,
                    budget,
                )
            })
            .transpose()?
            .unwrap_or(tick as f32 * 0.05);
        let time = match clip.loop_mode {
            EntityAnimationLoop::Loop if length > 0.0 => raw_time.rem_euclid(length),
            EntityAnimationLoop::Once | EntityAnimationLoop::HoldOnLastFrame => {
                raw_time.clamp(0.0, length)
            }
            EntityAnimationLoop::Loop => 0.0,
        };
        let first = clip.first_channel as usize;
        let end = first
            .checked_add(clip.channel_count as usize)
            .ok_or(EvalError::Invalid)?;
        for channel in assets
            .animation_channels()
            .get(first..end)
            .ok_or(EvalError::Invalid)?
        {
            budget.charge_work()?;
            let bone = local
                .get_mut(channel.bone as usize)
                .ok_or(EvalError::Invalid)?;
            let this_value = match channel.property {
                EntityAnimationProperty::Translation => bone.translation,
                EntityAnimationProperty::Rotation => bone.rotation,
                EntityAnimationProperty::Scale => bone.scale,
            };
            let value = sample_channel(
                assets,
                channel.first_keyframe,
                channel.keyframe_count,
                time,
                this_value,
                actor,
                history,
                tick,
                life_tick,
                budget,
            )?;
            match channel.property {
                EntityAnimationProperty::Translation => {
                    for (axis, value) in value.into_iter().enumerate() {
                        bone.translation[axis] += value * weight;
                    }
                }
                EntityAnimationProperty::Rotation => {
                    for (axis, value) in value.into_iter().enumerate() {
                        bone.rotation[axis] += value * weight;
                    }
                }
                EntityAnimationProperty::Scale => {
                    for (axis, value) in value.into_iter().enumerate() {
                        bone.scale[axis] *= 1.0 + (value - 1.0) * weight;
                    }
                }
            }
        }
    }
    Ok(local)
}

fn sample_channel(
    assets: &RuntimeEntityAssets,
    first: u32,
    count: u32,
    time: f32,
    this_value: [f32; 3],
    actor: &ActorSnapshot,
    history: &VecDeque<ActorTickInput>,
    tick: u64,
    life_tick: u64,
    budget: &mut EvalBudget<'_>,
) -> Result<[f32; 3], EvalError> {
    let first = first as usize;
    let frames = assets
        .animation_keyframes()
        .get(
            first
                ..first
                    .checked_add(count as usize)
                    .ok_or(EvalError::Invalid)?,
        )
        .ok_or(EvalError::Invalid)?;
    let first_frame = frames.first().ok_or(EvalError::Invalid)?;
    if time < first_frame.time_seconds.get() {
        return sample_keyframe(
            assets,
            first_frame,
            this_value,
            actor,
            history,
            tick,
            life_tick,
            budget,
        );
    }
    let exact_end = frames.partition_point(|frame| frame.time_seconds.get() <= time);
    if exact_end > 0 && frames[exact_end - 1].time_seconds.get() == time {
        return sample_keyframe(
            assets,
            &frames[exact_end - 1],
            this_value,
            actor,
            history,
            tick,
            life_tick,
            budget,
        );
    }
    if exact_end == frames.len() {
        return sample_keyframe(
            assets,
            &frames[frames.len() - 1],
            this_value,
            actor,
            history,
            tick,
            life_tick,
            budget,
        );
    }
    let left_index = exact_end - 1;
    let right_index = exact_end;
    let left = &frames[left_index];
    let right = &frames[right_index];
    let left_time = left.time_seconds.get();
    let right_time = right.time_seconds.get();
    let amount = ((time - left_time) / (right_time - left_time)).clamp(0.0, 1.0);
    let left_value = sample_keyframe(
        assets, left, this_value, actor, history, tick, life_tick, budget,
    )?;
    let right_value = sample_keyframe(
        assets, right, this_value, actor, history, tick, life_tick, budget,
    )?;
    match left.interpolation {
        EntityAnimationInterpolation::Step => Ok(left_value),
        EntityAnimationInterpolation::Linear => Ok(lerp3(left_value, right_value, amount)),
        EntityAnimationInterpolation::CatmullRom => {
            let previous_frame = frames
                .get(left_index.saturating_sub(1))
                .unwrap_or(left)
                .to_owned();
            let next_frame = frames.get(right_index + 1).unwrap_or(right).to_owned();
            let previous = sample_keyframe(
                assets,
                &previous_frame,
                this_value,
                actor,
                history,
                tick,
                life_tick,
                budget,
            )?;
            let next = sample_keyframe(
                assets,
                &next_frame,
                this_value,
                actor,
                history,
                tick,
                life_tick,
                budget,
            )?;
            Ok(std::array::from_fn(|axis| {
                catmull(
                    previous[axis],
                    left_value[axis],
                    right_value[axis],
                    next[axis],
                    amount,
                )
            }))
        }
    }
}

fn sample_keyframe(
    assets: &RuntimeEntityAssets,
    frame: &assets::EntityAnimationKeyframe,
    this_value: [f32; 3],
    actor: &ActorSnapshot,
    history: &VecDeque<ActorTickInput>,
    tick: u64,
    life_tick: u64,
    budget: &mut EvalBudget<'_>,
) -> Result<[f32; 3], EvalError> {
    let mut value = frame.value.map(|value| value.get());
    for (axis, expression) in frame.expressions.iter().enumerate() {
        if let Some(expression) = expression {
            value[axis] = evaluate_expression_with_this(
                assets,
                *expression as usize,
                actor,
                history,
                tick,
                life_tick,
                this_value[axis],
                budget,
            )?;
        }
    }
    Ok(value)
}
