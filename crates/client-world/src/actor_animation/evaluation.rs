use super::*;

pub(super) fn evaluate_expression(
    assets: &RuntimeEntityAssets,
    expression_index: usize,
    actor: &ActorSnapshot,
    history: &VecDeque<ActorTickInput>,
    tick: u64,
    life_tick: u64,
    budget: &mut EvalBudget<'_>,
) -> Result<f32, EvalError> {
    let expression = assets
        .molang_expressions()
        .get(expression_index)
        .ok_or(EvalError::Invalid)?;
    evaluate_ops(assets, expression, actor, history, tick, life_tick, budget)
}

pub(super) fn evaluate_ops(
    assets: &RuntimeEntityAssets,
    expression: &CompiledMolangExpression,
    actor: &ActorSnapshot,
    history: &VecDeque<ActorTickInput>,
    tick: u64,
    life_tick: u64,
    budget: &mut EvalBudget<'_>,
) -> Result<f32, EvalError> {
    let first = expression.first_op as usize;
    let end = first
        .checked_add(expression.op_count as usize)
        .ok_or(EvalError::Invalid)?;
    let ops = assets
        .molang_ops()
        .get(first..end)
        .ok_or(EvalError::Invalid)?;
    let mut stack = Vec::with_capacity(expression.max_stack as usize);
    for op in ops {
        budget.charge()?;
        match *op {
            MolangOp::Push(value) => stack.push(value.get()),
            MolangOp::LoadQuery(symbol) => {
                let symbol = assets
                    .molang_symbols()
                    .get(symbol as usize)
                    .ok_or(EvalError::Invalid)?;
                stack.push(query(actor, history, tick, life_tick, &symbol.identifier));
            }
            MolangOp::LoadVariable(_) => stack.push(0.0),
            MolangOp::Add => binary(&mut stack, |a, b| a + b)?,
            MolangOp::Subtract => binary(&mut stack, |a, b| a - b)?,
            MolangOp::Multiply => binary(&mut stack, |a, b| a * b)?,
            MolangOp::Divide => binary(&mut stack, |a, b| if b == 0.0 { 0.0 } else { a / b })?,
            MolangOp::Modulo => binary(&mut stack, |a, b| if b == 0.0 { 0.0 } else { a % b })?,
            MolangOp::Negate => unary(&mut stack, |value| -value)?,
            MolangOp::Not => unary(&mut stack, |value| bool_value(!truthy(value)))?,
            MolangOp::Abs => unary(&mut stack, f32::abs)?,
            MolangOp::Ceil => unary(&mut stack, f32::ceil)?,
            MolangOp::Floor => unary(&mut stack, f32::floor)?,
            MolangOp::Round => unary(&mut stack, f32::round)?,
            MolangOp::Sqrt => unary(&mut stack, |value| value.max(0.0).sqrt())?,
            MolangOp::Sin => unary(&mut stack, |value| value.to_radians().sin())?,
            MolangOp::Cos => unary(&mut stack, |value| value.to_radians().cos())?,
            MolangOp::And => binary(&mut stack, |a, b| bool_value(truthy(a) && truthy(b)))?,
            MolangOp::Or => binary(&mut stack, |a, b| bool_value(truthy(a) || truthy(b)))?,
            MolangOp::Equal => binary(&mut stack, |a, b| bool_value(a == b))?,
            MolangOp::NotEqual => binary(&mut stack, |a, b| bool_value(a != b))?,
            MolangOp::Less => binary(&mut stack, |a, b| bool_value(a < b))?,
            MolangOp::LessEqual => binary(&mut stack, |a, b| bool_value(a <= b))?,
            MolangOp::Greater => binary(&mut stack, |a, b| bool_value(a > b))?,
            MolangOp::GreaterEqual => binary(&mut stack, |a, b| bool_value(a >= b))?,
            MolangOp::Min => binary(&mut stack, f32::min)?,
            MolangOp::Max => binary(&mut stack, f32::max)?,
            MolangOp::Select => ternary(
                &mut stack,
                |condition, yes, no| {
                    if truthy(condition) { yes } else { no }
                },
            )?,
            MolangOp::Clamp => {
                let max = pop(&mut stack)?;
                let min = pop(&mut stack)?;
                let value = pop(&mut stack)?;
                if min > max {
                    return Err(EvalError::Invalid);
                }
                stack.push(value.max(min).min(max));
            }
            MolangOp::Lerp => ternary(&mut stack, |start, end, amount| {
                start + (end - start) * amount
            })?,
            MolangOp::SelectCollection(collection) => {
                let index = pop(&mut stack)?;
                let collection = assets
                    .molang_collections()
                    .get(collection as usize)
                    .ok_or(EvalError::Invalid)?;
                if collection.item_count == 0 {
                    return Err(EvalError::Invalid);
                }
                let clamped = index
                    .floor()
                    .clamp(0.0, f32::from(collection.item_count - 1))
                    as usize;
                let item = assets
                    .molang_collection_items()
                    .get(collection.first_item as usize + clamped)
                    .ok_or(EvalError::Invalid)?;
                stack.push(item.value.get());
            }
        }
        if stack.last().is_some_and(|value| !value.is_finite()) {
            return Err(EvalError::Invalid);
        }
    }
    if stack.len() != 1 {
        return Err(EvalError::Invalid);
    }
    pop(&mut stack)
}

pub(super) fn query(
    actor: &ActorSnapshot,
    history: &VecDeque<ActorTickInput>,
    tick: u64,
    life_tick: u64,
    identifier: &str,
) -> f32 {
    let identifier = identifier.strip_prefix("query.").unwrap_or(identifier);
    let input = history.back().copied().unwrap_or(ActorTickInput {
        velocity: actor.velocity,
        on_ground: actor.on_ground.unwrap_or(false),
        body_yaw: actor.body_yaw,
        head_yaw: actor.head_yaw,
        pitch: actor.pitch,
    });
    let ground_speed = input.velocity[0].hypot(input.velocity[2]);
    match identifier {
        "anim_time" => tick as f32 * 0.05,
        "life_time" => life_tick as f32 * 0.05,
        "modified_move_speed" => actor
            .attributes
            .get("minecraft:movement")
            .or_else(|| actor.attributes.get("movement"))
            .map_or(ground_speed, |attribute| attribute.current),
        "ground_speed" => ground_speed,
        "is_on_ground" => bool_value(input.on_ground),
        "is_moving" => bool_value(
            input
                .velocity
                .iter()
                .any(|value| value.abs() > f32::EPSILON),
        ),
        "is_sprinting" => bool_value(generic_metadata_flag(actor, 3)),
        "is_sneaking" => bool_value(generic_metadata_flag(actor, 1)),
        "is_sleeping" => bool_value(
            generic_metadata_flag(actor, 2)
                || player_sleeping_flag(actor)
                || extended_sleeping_flag(actor),
        ),
        "body_y_rotation" => input.body_yaw,
        "head_y_rotation" => input.head_yaw,
        "target_x_rotation" => input.pitch,
        _ => 0.0,
    }
}

pub(super) fn generic_metadata_flag(actor: &ActorSnapshot, bit: u32) -> bool {
    matches!(actor.metadata.get(&0), Some(ActorMetadataValue::Flags(flags)) if flags & (1_u64 << bit) != 0)
}

pub(super) fn player_sleeping_flag(actor: &ActorSnapshot) -> bool {
    matches!(actor.metadata.get(&26), Some(ActorMetadataValue::Byte(flags)) if (*flags as u8) & (1 << 1) != 0)
}

pub(super) fn extended_sleeping_flag(actor: &ActorSnapshot) -> bool {
    matches!(actor.metadata.get(&92), Some(ActorMetadataValue::FlagsExtended(flags)) if flags & (1 << 11) != 0)
}

pub(super) fn compose_pose(
    bones: &[RuntimeBone],
    local: &[LocalDelta],
) -> Option<Vec<BoneTransform>> {
    let mut transforms = vec![None; bones.len()];
    let mut visiting = vec![false; bones.len()];
    for index in 0..bones.len() {
        compose_bone(index, bones, local, &mut transforms, &mut visiting)?;
    }
    transforms.into_iter().collect()
}

pub(super) fn compose_bone(
    index: usize,
    bones: &[RuntimeBone],
    local: &[LocalDelta],
    transforms: &mut [Option<BoneTransform>],
    visiting: &mut [bool],
) -> Option<BoneTransform> {
    if let Some(transform) = transforms.get(index).copied().flatten() {
        return Some(transform);
    }
    if *visiting.get(index)? {
        return None;
    }
    visiting[index] = true;
    let bone = bones.get(index)?;
    let delta = local.get(index).copied().unwrap_or_default();
    if (delta.scale[0] - delta.scale[1]).abs() > f32::EPSILON
        || (delta.scale[0] - delta.scale[2]).abs() > f32::EPSILON
    {
        return None;
    }
    let translation = std::array::from_fn(|axis| {
        let parent_pivot = bone
            .parent
            .and_then(|parent| bones.get(parent))
            .map_or(0.0, |parent| parent.pivot[axis]);
        bone.pivot[axis] - parent_pivot + delta.translation[axis]
    });
    let rotation = quat_from_euler(std::array::from_fn(|axis| {
        bone.rotation[axis] + delta.rotation[axis]
    }));
    let scale = delta.scale[0];
    let transform = if let Some(parent_index) = bone.parent {
        let parent = compose_bone(parent_index, bones, local, transforms, visiting)?;
        let parent_scale = parent.translation_scale[3];
        let scaled = translation.map(|value| value * parent_scale);
        let rotated = rotate_vector(parent.rotation, scaled);
        BoneTransform {
            rotation: quat_multiply(parent.rotation, rotation),
            translation_scale: [
                parent.translation_scale[0] + rotated[0],
                parent.translation_scale[1] + rotated[1],
                parent.translation_scale[2] + rotated[2],
                parent_scale * scale,
            ],
        }
    } else {
        BoneTransform {
            rotation,
            translation_scale: [translation[0], translation[1], translation[2], scale],
        }
    };
    if transform
        .rotation
        .iter()
        .chain(transform.translation_scale.iter())
        .any(|value| !value.is_finite())
    {
        return None;
    }
    visiting[index] = false;
    transforms[index] = Some(transform);
    Some(transform)
}

pub(super) fn quat_from_euler(rotation: [f32; 3]) -> [f32; 4] {
    let [x, y, z] = rotation.map(|value| value.to_radians() * 0.5);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    [
        sx * cy * cz - cx * sy * sz,
        cx * sy * cz + sx * cy * sz,
        cx * cy * sz - sx * sy * cz,
        cx * cy * cz + sx * sy * sz,
    ]
}

pub(super) fn quat_multiply(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

pub(super) fn rotate_vector(rotation: [f32; 4], vector: [f32; 3]) -> [f32; 3] {
    let qvector = [vector[0], vector[1], vector[2], 0.0];
    let inverse = [-rotation[0], -rotation[1], -rotation[2], rotation[3]];
    let result = quat_multiply(quat_multiply(rotation, qvector), inverse);
    [result[0], result[1], result[2]]
}

pub(super) fn lerp3(left: [f32; 3], right: [f32; 3], amount: f32) -> [f32; 3] {
    std::array::from_fn(|axis| left[axis] + (right[axis] - left[axis]) * amount)
}

pub(super) fn catmull(p0: f32, p1: f32, p2: f32, p3: f32, amount: f32) -> f32 {
    let amount2 = amount * amount;
    let amount3 = amount2 * amount;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * amount
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * amount2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * amount3)
}

pub(super) fn pop(stack: &mut Vec<f32>) -> Result<f32, EvalError> {
    stack.pop().ok_or(EvalError::Invalid)
}

pub(super) fn unary(
    stack: &mut Vec<f32>,
    operation: impl FnOnce(f32) -> f32,
) -> Result<(), EvalError> {
    let value = pop(stack)?;
    stack.push(operation(value));
    Ok(())
}

pub(super) fn binary(
    stack: &mut Vec<f32>,
    operation: impl FnOnce(f32, f32) -> f32,
) -> Result<(), EvalError> {
    let right = pop(stack)?;
    let left = pop(stack)?;
    stack.push(operation(left, right));
    Ok(())
}

pub(super) fn ternary(
    stack: &mut Vec<f32>,
    operation: impl FnOnce(f32, f32, f32) -> f32,
) -> Result<(), EvalError> {
    let third = pop(stack)?;
    let second = pop(stack)?;
    let first = pop(stack)?;
    stack.push(operation(first, second, third));
    Ok(())
}

pub(super) fn truthy(value: f32) -> bool {
    value != 0.0
}

pub(super) fn bool_value(value: bool) -> f32 {
    u8::from(value).into()
}
