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
    evaluate_expression_with_this(
        assets,
        expression_index,
        actor,
        history,
        tick,
        life_tick,
        0.0,
        budget,
    )
}

pub(super) fn evaluate_expression_with_this(
    assets: &RuntimeEntityAssets,
    expression_index: usize,
    actor: &ActorSnapshot,
    history: &VecDeque<ActorTickInput>,
    tick: u64,
    life_tick: u64,
    this_value: f32,
    budget: &mut EvalBudget<'_>,
) -> Result<f32, EvalError> {
    let expression = assets
        .molang_expressions()
        .get(expression_index)
        .ok_or(EvalError::Invalid)?;
    evaluate_ops(
        assets, expression, actor, history, tick, life_tick, this_value, budget,
    )
}

pub(super) fn evaluate_ops(
    assets: &RuntimeEntityAssets,
    expression: &CompiledMolangExpression,
    actor: &ActorSnapshot,
    history: &VecDeque<ActorTickInput>,
    tick: u64,
    life_tick: u64,
    this_value: f32,
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
            MolangOp::LoadVariable(symbol) => {
                let symbol = assets
                    .molang_symbols()
                    .get(symbol as usize)
                    .ok_or(EvalError::Invalid)?;
                stack.push(variable(
                    actor,
                    history,
                    tick,
                    life_tick,
                    &symbol.identifier,
                ));
            }
            MolangOp::LoadThis => stack.push(this_value),
            MolangOp::Add => binary(&mut stack, |a, b| a + b)?,
            MolangOp::Subtract => binary(&mut stack, |a, b| a - b)?,
            MolangOp::Multiply => binary(&mut stack, |a, b| a * b)?,
            MolangOp::Divide => binary(&mut stack, |a, b| if b == 0.0 { 0.0 } else { a / b })?,
            MolangOp::Modulo => binary(&mut stack, |a, b| if b == 0.0 { 0.0 } else { a % b })?,
            MolangOp::Pow => binary(&mut stack, f32::powf)?,
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
        action: Default::default(),
        items: Default::default(),
        distance_moved: 0.0,
        default_bone_pivots: [0.0; 8],
        root_locator_offset: [0.0; 3],
    });
    let ground_speed = input.velocity[0].hypot(input.velocity[2]);
    let vertical_speed = input.velocity[1];
    let target_y_rotation = shortest_angle(input.body_yaw, input.head_yaw);
    let modified_distance_moved = input.distance_moved;
    let action_remaining_use = (input.action.item_use_normalized > 0.0)
        .then(|| (1.0 - input.action.item_use_normalized).max(0.0))
        .unwrap_or(0.0);
    match identifier {
        "anim_time" => tick as f32 * 0.05,
        "life_time" => life_tick as f32 * 0.05,
        "delta_time" => 0.05,
        "modified_move_speed" => actor
            .attributes
            .get("minecraft:movement")
            .or_else(|| actor.attributes.get("movement"))
            .map_or(ground_speed, |attribute| attribute.current),
        "ground_speed" => ground_speed,
        "ground_speed_squared" => ground_speed * ground_speed,
        "vertical_speed" => vertical_speed,
        "modified_distance_moved" | "walk_distance" => modified_distance_moved,
        "is_on_ground" => bool_value(input.on_ground),
        "is_moving" => bool_value(ground_speed > 0.01),
        "is_jumping" => bool_value(!input.on_ground && vertical_speed > 0.01),
        "is_falling" => bool_value(!input.on_ground && vertical_speed < -0.01),
        "is_alive" => 1.0,
        "is_baby" => bool_value(metadata_flag_one(actor, 11)),
        "is_swimming" => bool_value(metadata_flag_one(actor, 57)),
        "is_riding" => bool_value(metadata_flag_one(actor, 2)),
        "is_crawling" => bool_value(metadata_flag_two(actor, 49)),
        "is_emoting" => bool_value(metadata_flag_two(actor, 27)),
        "is_gliding" => bool_value(metadata_flag_one(actor, 32)),
        "has_target" => bool_value(metadata_has_target(actor)),
        "is_spectator" => bool_value(
            actor
                .resolved_game_mode
                .is_some_and(ActorGameMode::is_spectator),
        ),
        "has_head_gear" => bool_value(input.items.armor_layers[0]),
        "is_charging" => bool_value(
            metadata_flag_one(actor, 4)
                || metadata_flag_one(actor, 27)
                || metadata_flag_one(actor, 43)
                || (input.items.main_hand == ActorItemKind::Crossbow
                    && input.action.item_use_normalized > 0.0),
        ),
        "blocking" => bool_value(
            metadata_flag_two(actor, 7)
                || metadata_flag_two(actor, 9)
                || metadata_flag_two(actor, 10)
                || (input.action.item_use_normalized > 0.0
                    && (input.items.main_hand == ActorItemKind::Shield
                        || input.items.off_hand == ActorItemKind::Shield)),
        ),
        "item_is_charged" => bool_value(
            input.items.main_hand_charged
                || input.items.off_hand_charged
                || input.action.item_use_normalized >= 1.0,
        ),
        "item_remaining_use_duration" => input
            .items
            .main_hand_remaining_use_duration
            .max(action_remaining_use),
        "item_remaining_use_duration_main_hand" => input
            .items
            .main_hand_remaining_use_duration
            .max(action_remaining_use),
        "item_remaining_use_duration_off_hand" => input
            .items
            .off_hand_remaining_use_duration
            .max(action_remaining_use),
        "main_hand_item_use_duration" => input.action.item_use_normalized,
        "main_hand_item_max_duration" => 1.0,
        "position_delta_x" => input.velocity[0] * 0.05,
        "position_delta_y" => input.velocity[1] * 0.05,
        "position_delta_z" => input.velocity[2] * 0.05,
        "main_hand_is_shield" => item_bool(input.items.main_hand, ActorItemKind::Shield),
        "main_hand_is_filled_map" => item_bool(input.items.main_hand, ActorItemKind::FilledMap),
        "main_hand_is_crossbow" => item_bool(input.items.main_hand, ActorItemKind::Crossbow),
        "main_hand_is_bow" => item_bool(input.items.main_hand, ActorItemKind::Bow),
        "main_hand_is_brush" => item_bool(input.items.main_hand, ActorItemKind::Brush),
        "main_hand_is_heavy_core" => item_bool(input.items.main_hand, ActorItemKind::HeavyCore),
        "off_hand_is_shield" => item_bool(input.items.off_hand, ActorItemKind::Shield),
        "off_hand_is_filled_map" => item_bool(input.items.off_hand, ActorItemKind::FilledMap),
        "off_hand_is_crossbow" => item_bool(input.items.off_hand, ActorItemKind::Crossbow),
        "off_hand_is_bow" => item_bool(input.items.off_hand, ActorItemKind::Bow),
        "off_hand_is_brush" => item_bool(input.items.off_hand, ActorItemKind::Brush),
        "off_hand_is_heavy_core" => item_bool(input.items.off_hand, ActorItemKind::HeavyCore),
        "helmet_layer_visible" => bool_value(input.items.armor_layers[0]),
        "chest_layer_visible" => bool_value(input.items.armor_layers[1]),
        "leg_layer_visible" | "leggings_layer_visible" => bool_value(input.items.armor_layers[2]),
        "boot_layer_visible" | "boots_layer_visible" => bool_value(input.items.armor_layers[3]),
        "body_layer_visible" => bool_value(input.items.armor_layers[4]),
        "cape_flap_amount" => cape_flap_amount(input),
        "sleep_rotation" => 0.0,
        "root_locator_offset_armor_default_neck" => input.root_locator_offset[1],
        "default_bone_pivot_rightarm_y" => input.default_bone_pivots[0],
        "default_bone_pivot_rightarm_z" => input.default_bone_pivots[1],
        "default_bone_pivot_leftarm_y" => input.default_bone_pivots[2],
        "default_bone_pivot_leftarm_z" => input.default_bone_pivots[3],
        "default_bone_pivot_rightitem_y" => input.default_bone_pivots[4],
        "default_bone_pivot_rightitem_z" => input.default_bone_pivots[5],
        "default_bone_pivot_leftitem_y" => input.default_bone_pivots[6],
        "default_bone_pivot_leftitem_z" => input.default_bone_pivots[7],
        "is_sprinting" => bool_value(metadata_flag_one(actor, 3)),
        "is_sneaking" => bool_value(metadata_flag_one(actor, 1)),
        "is_sleeping" => bool_value(metadata_flag_two(actor, 11) || player_sleeping_flag(actor)),
        "body_y_rotation" => input.body_yaw,
        "head_y_rotation" => input.head_yaw,
        "target_x_rotation" => input.pitch,
        "target_y_rotation" => target_y_rotation,
        _ => 0.0,
    }
}

pub(super) fn variable(
    actor: &ActorSnapshot,
    history: &VecDeque<ActorTickInput>,
    tick: u64,
    life_tick: u64,
    identifier: &str,
) -> f32 {
    let identifier = identifier.strip_prefix("variable.").unwrap_or(identifier);
    let input = history.back().copied().unwrap_or(ActorTickInput {
        velocity: actor.velocity,
        on_ground: actor.on_ground.unwrap_or(false),
        body_yaw: actor.body_yaw,
        head_yaw: actor.head_yaw,
        pitch: actor.pitch,
        action: Default::default(),
        items: Default::default(),
        distance_moved: 0.0,
        default_bone_pivots: [0.0; 8],
        root_locator_offset: [0.0; 3],
    });
    let ground_speed = input.velocity[0].hypot(input.velocity[2]);
    match identifier {
        // The player movement clips phase against distance travelled, not
        // render time. Keep the public carrier's degree-based Molang math
        // here so interpolation does not change the gait.
        "tcos0" => {
            let modified_distance_moved =
                query(actor, history, tick, life_tick, "modified_distance_moved");
            let modified_move_speed = query(actor, history, tick, life_tick, "modified_move_speed");
            let gliding_speed_value = 1.0;
            (modified_distance_moved * 38.17).to_radians().cos() * modified_move_speed
                / gliding_speed_value
                * 57.3
        }
        "gliding_speed_value" => 1.0,
        "moving" | "walking" => bool_value(ground_speed > 0.01),
        "is_holding_left" => bool_value(input.items.off_hand != ActorItemKind::Empty),
        "is_holding_right" => bool_value(input.items.main_hand != ActorItemKind::Empty),
        "is_using_vr"
        | "is_horizontal_splitscreen"
        | "is_vertical_splitscreen"
        | "is_paperdoll" => 0.0,
        "attack_time" => input.action.attack_time,
        "attack_body_rot_y" => {
            let attack_time = input.action.attack_time;
            if attack_time >= 0.0 {
                (360.0 * attack_time.max(0.0).sqrt()).to_radians().sin() * 5.0
            } else {
                0.0
            }
        }
        "tp_melee_spear_base_arm_rotation_x"
        | "tp_melee_spear_attack_arm_rotation_x"
        | "tp_melee_spear_attack_item_rotation_x"
        | "fp_melee_spear_attack_item_position_x"
        | "fp_melee_spear_attack_item_position_y"
        | "fp_melee_spear_attack_item_position_z"
        | "fp_melee_spear_attack_item_rotation_y"
        | "fp_melee_spear_attack_item_rotation_z"
        | "fp_melee_spear_use_item_position_x"
        | "fp_melee_spear_use_item_position_y"
        | "fp_melee_spear_use_item_position_z"
        | "fp_melee_spear_use_item_rotation_y"
        | "fp_melee_spear_use_item_rotation_z"
        | "map_face_icon" => 0.0,
        "map_angle" => (1.0 - input.pitch / 45.1).clamp(0.0, 1.0),
        "player_x_rotation" => input.pitch,
        "damage_nearby_mobs" => 0.0,
        "item_use_normalized" => input.action.item_use_normalized,
        "use_item_interval_progress" => input.action.use_item_interval_progress,
        "use_item_startup_progress" => input.action.use_item_startup_progress,
        "swim_amount" => query(actor, history, tick, life_tick, "is_swimming"),
        "is_baby" => bool_value(metadata_flag_one(actor, 11)),
        "charge_amount" => metadata_number(actor, 22),
        "is_brandishing_spear" => bool_value(
            input.items.main_hand == ActorItemKind::Spear && input.action.item_use_normalized > 0.0,
        ),
        "is_holding_spyglass" => bool_value(
            (input.items.main_hand == ActorItemKind::Spyglass
                || input.items.off_hand == ActorItemKind::Spyglass)
                && input.action.item_use_normalized > 0.0,
        ),
        "is_tooting_goat_horn" => bool_value(
            (input.items.main_hand == ActorItemKind::GoatHorn
                || input.items.off_hand == ActorItemKind::GoatHorn)
                && input.action.item_use_normalized > 0.0,
        ),
        "is_using_brush" => bool_value(
            (input.items.main_hand == ActorItemKind::Brush
                || input.items.off_hand == ActorItemKind::Brush)
                && input.action.item_use_normalized > 0.0,
        ),
        "is_sneaking" => bool_value(metadata_flag_one(actor, 1)),
        "helmet_layer_visible" => bool_value(input.items.armor_layers[0]),
        "chest_layer_visible" => bool_value(input.items.armor_layers[1]),
        "leg_layer_visible" | "leggings_layer_visible" => bool_value(input.items.armor_layers[2]),
        "boot_layer_visible" | "boots_layer_visible" => bool_value(input.items.armor_layers[3]),
        "body_layer_visible" => bool_value(input.items.armor_layers[4]),
        "rightarmswim_amount" | "leftarmswim_amount" => {
            query(actor, history, tick, life_tick, "is_swimming")
        }
        "melee_spear_equipped" => bool_value(input.items.main_hand == ActorItemKind::Spear),
        "is_first_person" => 0.0,
        "first_person_rotation_factor" => (180.0 * (1.0 - input.action.attack_time))
            .to_radians()
            .sin(),
        "player_arm_height" | "player_offhand_arm_height" => 1.0,
        "hand_bob" => bool_value(ground_speed > 0.01),
        "short_arm_offset_left" | "short_arm_offset_right" => 0.0,
        "first_person_item_rotation_factor" => 1.0,
        "bob_animation" => bool_value(ground_speed > 0.01),
        "is_riding" => query(actor, history, tick, life_tick, "is_riding"),
        "riding_y_offset" => (query(actor, history, tick, life_tick, "is_riding") > 0.0)
            .then_some(-3.0)
            .unwrap_or(0.0),
        "life_time" => life_tick as f32 * 0.05,
        _ => 0.0,
    }
}

fn shortest_angle(from: f32, to: f32) -> f32 {
    if !from.is_finite() || !to.is_finite() {
        return 0.0;
    }
    (to - from + 180.0).rem_euclid(360.0) - 180.0
}

fn item_bool(actual: ActorItemKind, expected: ActorItemKind) -> f32 {
    bool_value(actual == expected)
}

fn cape_flap_amount(input: ActorTickInput) -> f32 {
    // The network actor stream does not carry cloth vertices. Keep the
    // animation query tied to authoritative movement so the retained cape
    // clip follows standing, walking, sprinting, and falling consistently.
    let horizontal_speed = input.velocity[0].hypot(input.velocity[2]);
    let vertical_speed = input.velocity[1].abs();
    ((horizontal_speed + vertical_speed * 0.35) / 4.3).clamp(0.0, 1.0)
}

fn metadata_flag_one(actor: &ActorSnapshot, bit: u32) -> bool {
    bit < u64::BITS
        && matches!(
            actor.metadata.get(&0),
            Some(ActorMetadataValue::Flags(flags)) if flags & (1_u64 << bit) != 0
        )
}

fn metadata_flag_two(actor: &ActorSnapshot, bit: u32) -> bool {
    bit < u64::BITS
        && matches!(
            actor.metadata.get(&92),
            Some(ActorMetadataValue::FlagsExtended(flags)) if flags & (1_u64 << bit) != 0
        )
}

pub(super) fn player_sleeping_flag(actor: &ActorSnapshot) -> bool {
    matches!(actor.metadata.get(&26), Some(ActorMetadataValue::Byte(flags)) if (*flags as u8) & (1 << 1) != 0)
}

fn metadata_has_target(actor: &ActorSnapshot) -> bool {
    match actor.metadata.get(&6) {
        Some(ActorMetadataValue::Long(value)) => *value != 0,
        Some(ActorMetadataValue::Int(value)) => *value != 0,
        Some(ActorMetadataValue::Short(value)) => *value != 0,
        Some(ActorMetadataValue::Byte(value)) => *value != 0,
        _ => false,
    }
}

fn metadata_number(actor: &ActorSnapshot, key: i32) -> f32 {
    match actor.metadata.get(&key) {
        Some(ActorMetadataValue::Float(value)) if value.is_finite() => *value,
        Some(ActorMetadataValue::Long(value)) => *value as f32,
        Some(ActorMetadataValue::Int(value)) => *value as f32,
        Some(ActorMetadataValue::Short(value)) => f32::from(*value),
        Some(ActorMetadataValue::Byte(value)) => f32::from(*value),
        _ => 0.0,
    }
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
