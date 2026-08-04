use std::{collections::BTreeMap, path::Path};

use assets::{
    AssetError, EntityAnimationChannel, EntityAnimationClip, EntityAnimationInterpolation,
    EntityAnimationKeyframe, EntityAnimationLoop, EntityAnimationProperty, EntityAssetSource,
    EntityGeometryScalar,
};
use serde_json::{Map, Value};

use super::super::{SourcePayloads, invalid, json::parse_unique_json, molang::MolangCompiler};

pub(super) enum ClipCompileError {
    UnknownBone,
    UnsupportedExpression,
    Invalid(AssetError),
}

impl From<AssetError> for ClipCompileError {
    fn from(error: AssetError) -> Self {
        Self::Invalid(error)
    }
}

pub(super) fn compile_clip_for_geometry(
    symbol: u32,
    source: u32,
    definition: &Map<String, Value>,
    effective_bones: &[Box<str>],
    clips: &mut Vec<EntityAnimationClip>,
    channels: &mut Vec<EntityAnimationChannel>,
    keyframes: &mut Vec<EntityAnimationKeyframe>,
    molang: &mut MolangCompiler,
) -> Result<u32, ClipCompileError> {
    let mut bone_indices = BTreeMap::<Box<str>, u32>::new();
    for (index, bone) in effective_bones.iter().enumerate() {
        bone_indices.entry(bone.clone()).or_insert(index as u32);
    }
    let mut local_channels = Vec::new();
    let mut local_keyframes = Vec::new();
    let mut maximum_time = 0.0_f32;
    if let Some(bones) = definition.get("bones") {
        let bones = bones.as_object().ok_or_else(|| {
            ClipCompileError::Invalid(invalid("animation bones must be an object"))
        })?;
        for (bone_name, bone) in bones {
            let bone_index = bone_indices
                .get(bone_name.to_ascii_lowercase().as_str())
                .copied()
                .ok_or(ClipCompileError::UnknownBone)?;
            let bone = bone.as_object().ok_or_else(|| {
                ClipCompileError::Invalid(invalid("animation bone must be an object"))
            })?;
            for (field, property) in [
                ("position", EntityAnimationProperty::Translation),
                ("rotation", EntityAnimationProperty::Rotation),
                ("scale", EntityAnimationProperty::Scale),
            ] {
                let Some(value) = bone.get(field) else {
                    continue;
                };
                let first_keyframe = local_keyframes.len() as u32;
                parse_channel(value, &mut local_keyframes, &mut maximum_time, molang)?;
                local_channels.push(EntityAnimationChannel {
                    bone: bone_index,
                    property,
                    first_keyframe,
                    keyframe_count: local_keyframes.len() as u32 - first_keyframe,
                });
            }
        }
    }
    let declared_length = definition
        .get("animation_length")
        .map(parse_number)
        .transpose()
        .map_err(ClipCompileError::Invalid)?
        .unwrap_or(maximum_time)
        .max(maximum_time);
    let loop_mode = match definition.get("loop") {
        None | Some(Value::Bool(false)) => EntityAnimationLoop::Once,
        Some(Value::Bool(true)) => EntityAnimationLoop::Loop,
        Some(Value::String(value)) if value == "hold_on_last_frame" => {
            EntityAnimationLoop::HoldOnLastFrame
        }
        _ => {
            return Err(ClipCompileError::Invalid(invalid(
                "unsupported animation loop mode",
            )));
        }
    };
    let time_expression = definition
        .get("anim_time_update")
        .and_then(Value::as_str)
        .map(|expression| compile_animation_expression(expression, molang))
        .transpose()?;
    let first_channel = channels.len() as u32;
    let first_keyframe = keyframes.len() as u32;
    for channel in &mut local_channels {
        channel.first_keyframe += first_keyframe;
    }
    channels.extend(local_channels);
    keyframes.extend(local_keyframes);
    let clip = clips.len() as u32;
    clips.push(EntityAnimationClip {
        symbol,
        length_seconds: scalar(declared_length).map_err(ClipCompileError::Invalid)?,
        loop_mode,
        time_expression,
        first_channel,
        channel_count: channels.len() as u32 - first_channel,
        source,
    });
    Ok(clip)
}

#[derive(Clone, Copy)]
struct ParsedVector {
    value: [EntityGeometryScalar; 3],
    expressions: [Option<u32>; 3],
}

fn parse_channel(
    value: &Value,
    output: &mut Vec<EntityAnimationKeyframe>,
    maximum_time: &mut f32,
    molang: &mut MolangCompiler,
) -> Result<(), ClipCompileError> {
    if value.is_array() || value.is_number() || value.is_string() {
        let vector = parse_vector(value, molang)?;
        output.push(EntityAnimationKeyframe {
            time_seconds: scalar(0.0)?,
            value: vector.value,
            interpolation: EntityAnimationInterpolation::Linear,
            expressions: vector.expressions,
        });
        return Ok(());
    }
    let timeline = value
        .as_object()
        .ok_or_else(|| invalid("animation channel must be a vector or timeline"))?;
    for (time, value) in timeline {
        let time = time
            .parse::<f32>()
            .map_err(|_| invalid("malformed animation keyframe time"))?;
        if !time.is_finite() || time < 0.0 {
            return Err(invalid("invalid animation keyframe time").into());
        }
        *maximum_time = maximum_time.max(time);
        if let Some(object) = value.as_object() {
            let interpolation = match object.get("lerp_mode").and_then(Value::as_str) {
                None | Some("linear") => EntityAnimationInterpolation::Linear,
                Some("step") => EntityAnimationInterpolation::Step,
                Some("catmullrom") => EntityAnimationInterpolation::CatmullRom,
                _ => {
                    return Err(ClipCompileError::Invalid(invalid(
                        "unsupported animation interpolation",
                    )));
                }
            };
            let mut emitted = false;
            for field in ["pre", "post"] {
                if let Some(vector) = object.get(field) {
                    let vector = parse_vector(vector, molang)?;
                    output.push(EntityAnimationKeyframe {
                        time_seconds: scalar(time)?,
                        value: vector.value,
                        interpolation,
                        expressions: vector.expressions,
                    });
                    emitted = true;
                }
            }
            if !emitted {
                return Err(ClipCompileError::Invalid(invalid(
                    "keyframe object lacks pre/post values",
                )));
            }
        } else {
            let vector = parse_vector(value, molang)?;
            output.push(EntityAnimationKeyframe {
                time_seconds: scalar(time)?,
                value: vector.value,
                interpolation: EntityAnimationInterpolation::Linear,
                expressions: vector.expressions,
            });
        }
    }
    Ok(())
}

pub(super) fn looks_like_expression(value: &str) -> bool {
    value.contains("query.")
        || value.contains("variable.")
        || value.contains("temp.")
        || value.contains("math.")
        || value
            .bytes()
            .any(|byte| matches!(byte, b'+' | b'*' | b'/' | b'?' | b'('))
}

pub(super) fn read_json(
    root: &Path,
    payloads: &SourcePayloads,
    source: &EntityAssetSource,
) -> Result<Value, AssetError> {
    let path = root.join(source.path.as_ref());
    let bytes = payloads
        .get(source.path.as_ref())
        .ok_or_else(|| invalid("retained entity source payload is absent"))?;
    parse_unique_json(&path, bytes)
}

pub(super) fn required_object<'a>(
    value: &'a Value,
    field: &str,
) -> Result<&'a Map<String, Value>, AssetError> {
    let selected = if field.is_empty() {
        value
    } else {
        value
            .get(field)
            .ok_or_else(|| invalid("required object field is absent"))?
    };
    selected
        .as_object()
        .ok_or_else(|| invalid("required JSON object is invalid"))
}

fn parse_vector(
    value: &Value,
    molang: &mut MolangCompiler,
) -> Result<ParsedVector, ClipCompileError> {
    if let Some(number) = value.as_f64() {
        let scalar = scalar(number as f32).map_err(ClipCompileError::Invalid)?;
        return Ok(ParsedVector {
            value: [scalar; 3],
            expressions: [None; 3],
        });
    }
    if let Some(expression) = value.as_str() {
        let expression = compile_animation_expression(expression, molang)?;
        let zero = scalar(0.0).map_err(ClipCompileError::Invalid)?;
        return Ok(ParsedVector {
            value: [zero; 3],
            expressions: [Some(expression); 3],
        });
    }
    let values = value
        .as_array()
        .filter(|values| values.len() == 3)
        .ok_or_else(|| {
            ClipCompileError::Invalid(invalid(
                "animation vector must have exactly three finite numbers or expressions",
            ))
        })?;
    let mut parsed = ParsedVector {
        value: [scalar(0.0).map_err(ClipCompileError::Invalid)?; 3],
        expressions: [None; 3],
    };
    for (axis, value) in values.iter().enumerate() {
        match value {
            Value::String(expression) => {
                parsed.expressions[axis] = Some(compile_animation_expression(expression, molang)?);
            }
            _ => {
                let number = value
                    .as_f64()
                    .ok_or(ClipCompileError::UnsupportedExpression)?;
                parsed.value[axis] = scalar(number as f32).map_err(ClipCompileError::Invalid)?;
            }
        }
    }
    Ok(parsed)
}

fn compile_animation_expression(
    expression: &str,
    molang: &mut MolangCompiler,
) -> Result<u32, ClipCompileError> {
    let mut expression = expression
        .replace("q.", "query.")
        .replace("v.", "variable.")
        .replace("Math.Pi", "math.pi")
        .replace(
            "variable.riding_y_offset ?? 0.0",
            "variable.riding_y_offset",
        )
        .replace(
            "context.player_offhand_arm_height",
            "variable.player_offhand_arm_height",
        );
    for (from, to) in [
        (
            "query.item_remaining_use_duration('main_hand', 1.0)",
            "query.item_remaining_use_duration_main_hand",
        ),
        (
            "query.item_remaining_use_duration('off_hand', 1.0)",
            "query.item_remaining_use_duration_off_hand",
        ),
        (
            "query.get_root_locator_offset('armor_offset.default_neck', 1)",
            "query.root_locator_offset_armor_default_neck",
        ),
        ("query.position_delta(0)", "query.position_delta_x"),
        ("query.position_delta(1)", "query.position_delta_y"),
        ("query.position_delta(2)", "query.position_delta_z"),
        (
            "query.is_riding_any_entity_of_type('minecraft:minecart', 'minecraft:boat', 'minecraft:chest_boat', 'minecraft:strider')",
            "query.is_riding",
        ),
        (
            "query.get_default_bone_pivot('rightarm',1)",
            "query.default_bone_pivot_rightarm_y",
        ),
        (
            "query.get_default_bone_pivot('rightarm',2)",
            "query.default_bone_pivot_rightarm_z",
        ),
        (
            "query.get_default_bone_pivot('leftarm',1)",
            "query.default_bone_pivot_leftarm_y",
        ),
        (
            "query.get_default_bone_pivot('leftarm',2)",
            "query.default_bone_pivot_leftarm_z",
        ),
        (
            "query.get_default_bone_pivot('rightitem',1)",
            "query.default_bone_pivot_rightitem_y",
        ),
        (
            "query.get_default_bone_pivot('rightitem',2)",
            "query.default_bone_pivot_rightitem_z",
        ),
        (
            "query.get_default_bone_pivot('leftitem',1)",
            "query.default_bone_pivot_leftitem_y",
        ),
        (
            "query.get_default_bone_pivot('leftitem',2)",
            "query.default_bone_pivot_leftitem_z",
        ),
        (
            "query.is_item_name_any('slot.weapon.mainhand', 'minecraft:bow')",
            "query.main_hand_is_bow",
        ),
        (
            "query.is_item_name_any('slot.weapon.mainhand', 'minecraft:heavy_core')",
            "query.main_hand_is_heavy_core",
        ),
        (
            "query.get_equipped_item_name('off_hand') == 'shield'",
            "query.off_hand_is_shield",
        ),
        (
            "query.get_equipped_item_name('off_hand') != 'shield'",
            "!query.off_hand_is_shield",
        ),
        (
            "query.get_equipped_item_name('off_hand') == 'filled_map'",
            "query.off_hand_is_filled_map",
        ),
        (
            "query.get_equipped_item_name('off_hand') != 'filled_map'",
            "!query.off_hand_is_filled_map",
        ),
        (
            "query.get_equipped_item_name('main_hand') == 'shield'",
            "query.main_hand_is_shield",
        ),
        (
            "query.get_equipped_item_name('main_hand') == 'filled_map'",
            "query.main_hand_is_filled_map",
        ),
        (
            "query.get_equipped_item_name('main_hand') == 'crossbow'",
            "query.main_hand_is_crossbow",
        ),
        (
            "query.get_equipped_item_name('main_hand') == 'bow'",
            "query.main_hand_is_bow",
        ),
        (
            "query.get_equipped_item_name('main_hand') == 'brush'",
            "query.main_hand_is_brush",
        ),
        (
            "query.get_equipped_item_name(0, 1) == 'filled_map'",
            "query.main_hand_is_filled_map",
        ),
        (
            "query.get_equipped_item_name(0, 1) != 'filled_map'",
            "!query.main_hand_is_filled_map",
        ),
        (
            "query.get_equipped_item_name == 'crossbow'",
            "query.main_hand_is_crossbow",
        ),
        (
            "query.get_equipped_item_name != 'crossbow'",
            "!query.main_hand_is_crossbow",
        ),
        (
            "query.get_equipped_item_name == 'filled_map'",
            "query.main_hand_is_filled_map",
        ),
        (
            "query.get_equipped_item_name != 'filled_map'",
            "!query.main_hand_is_filled_map",
        ),
        (
            "query.get_equipped_item_name == 'shield'",
            "query.main_hand_is_shield",
        ),
        (
            "query.get_equipped_item_name != 'shield'",
            "!query.main_hand_is_shield",
        ),
        (
            "query.get_equipped_item_name == 'bow'",
            "query.main_hand_is_bow",
        ),
        (
            "query.get_equipped_item_name == 'brush'",
            "query.main_hand_is_brush",
        ),
    ] {
        expression = expression.replace(from, to);
    }
    molang
        .compile(&expression)
        .map_err(|_| ClipCompileError::UnsupportedExpression)
}

fn parse_number(value: &Value) -> Result<f32, AssetError> {
    let value = value
        .as_f64()
        .ok_or_else(|| invalid("expected finite numeric scalar"))? as f32;
    scalar(value)?;
    Ok(value)
}

fn scalar(value: f32) -> Result<EntityGeometryScalar, AssetError> {
    EntityGeometryScalar::new(value).ok_or_else(|| invalid("invalid finite entity scalar"))
}

pub(super) fn source_index(
    source: &EntityAssetSource,
    indices: &BTreeMap<&str, u32>,
) -> Result<u32, AssetError> {
    indices
        .get(source.path.as_ref())
        .copied()
        .ok_or_else(|| invalid("entity source is absent"))
}
