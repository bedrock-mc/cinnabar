use std::{collections::BTreeMap, path::Path};

use assets::{
    AssetError, EntityAnimationChannel, EntityAnimationClip, EntityAnimationInterpolation,
    EntityAnimationKeyframe, EntityAnimationLoop, EntityAnimationProperty, EntityAssetSource,
    EntityGeometryScalar,
};
use serde_json::{Map, Value};

use super::super::{SourcePayloads, invalid, json::parse_unique_json};

pub(super) enum ClipCompileError {
    UnknownBone,
    Invalid(AssetError),
}

pub(super) fn compile_clip_for_geometry(
    symbol: u32,
    source: u32,
    definition: &Map<String, Value>,
    effective_bones: &[Box<str>],
    clips: &mut Vec<EntityAnimationClip>,
    channels: &mut Vec<EntityAnimationChannel>,
    keyframes: &mut Vec<EntityAnimationKeyframe>,
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
                parse_channel(value, &mut local_keyframes, &mut maximum_time)
                    .map_err(ClipCompileError::Invalid)?;
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
        first_channel,
        channel_count: channels.len() as u32 - first_channel,
        source,
    });
    Ok(clip)
}

fn parse_channel(
    value: &Value,
    output: &mut Vec<EntityAnimationKeyframe>,
    maximum_time: &mut f32,
) -> Result<(), AssetError> {
    if value.is_array() || value.is_number() {
        output.push(EntityAnimationKeyframe {
            time_seconds: scalar(0.0)?,
            value: parse_vector(value)?,
            interpolation: EntityAnimationInterpolation::Linear,
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
            return Err(invalid("invalid animation keyframe time"));
        }
        *maximum_time = maximum_time.max(time);
        if let Some(object) = value.as_object() {
            let interpolation = match object.get("lerp_mode").and_then(Value::as_str) {
                None | Some("linear") => EntityAnimationInterpolation::Linear,
                Some("step") => EntityAnimationInterpolation::Step,
                Some("catmullrom") => EntityAnimationInterpolation::CatmullRom,
                _ => return Err(invalid("unsupported animation interpolation")),
            };
            let mut emitted = false;
            for field in ["pre", "post"] {
                if let Some(vector) = object.get(field) {
                    output.push(EntityAnimationKeyframe {
                        time_seconds: scalar(time)?,
                        value: parse_vector(vector)?,
                        interpolation,
                    });
                    emitted = true;
                }
            }
            if !emitted {
                return Err(invalid("keyframe object lacks pre/post values"));
            }
        } else {
            output.push(EntityAnimationKeyframe {
                time_seconds: scalar(time)?,
                value: parse_vector(value)?,
                interpolation: EntityAnimationInterpolation::Linear,
            });
        }
    }
    Ok(())
}

pub(super) fn has_string_leaf(value: &Value) -> bool {
    match value {
        Value::String(value) => !matches!(value.as_str(), "linear" | "step" | "catmullrom"),
        Value::Array(values) => values.iter().any(has_string_leaf),
        Value::Object(values) => values.values().any(has_string_leaf),
        _ => false,
    }
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

fn parse_vector(value: &Value) -> Result<[EntityGeometryScalar; 3], AssetError> {
    if let Some(number) = value.as_f64() {
        let scalar = scalar(number as f32)?;
        return Ok([scalar; 3]);
    }
    let values = value
        .as_array()
        .filter(|values| values.len() == 3)
        .ok_or_else(|| invalid("animation vector must have exactly three finite numbers"))?;
    Ok([
        scalar(parse_number(&values[0])?)?,
        scalar(parse_number(&values[1])?)?,
        scalar(parse_number(&values[2])?)?,
    ])
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
