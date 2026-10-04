use std::{collections::BTreeMap, path::Path};

use assets::{
    AssetError, EntityAnimationChannel, EntityAnimationClip, EntityAnimationInterpolation,
    EntityAnimationKeyframe, EntityAnimationLoop, EntityAnimationProperty, EntityAssetSource,
    EntityGeometryScalar,
};
use serde_json::{Map, Value};

use super::super::{SourcePayloads, invalid, json::parse_unique_json, molang::MolangCompiler};

pub(super) enum ClipCompileError {
    Invalid(AssetError),
}

pub(super) struct ClipOutputs<'a> {
    pub clips: &'a mut Vec<EntityAnimationClip>,
    pub channels: &'a mut Vec<EntityAnimationChannel>,
    pub keyframes: &'a mut Vec<EntityAnimationKeyframe>,
    pub molang: &'a mut MolangCompiler,
}

/// Compiles one clip for one geometry; returns its index and how many channels were dropped
/// because an axis expression is outside the reviewed Molang surface. Bones the geometry lacks
/// are skipped, as vanilla binds animations to each model by bone name, but still set the length.
pub(super) fn compile_clip_for_geometry(
    symbol: u32,
    source: u32,
    definition: &Map<String, Value>,
    (geometry, effective_bones): (u32, &[Box<str>]),
    outputs: ClipOutputs<'_>,
) -> Result<(u32, usize), ClipCompileError> {
    let ClipOutputs {
        clips,
        channels,
        keyframes,
        molang,
    } = outputs;
    let anim_time_update = definition
        .get("anim_time_update")
        .map(|value| compile_time_update(value, molang))
        .transpose()
        .map_err(ClipCompileError::Invalid)?;
    let mut dropped = 0;
    let mut uncompiled = 0;
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
                .copied();
            let bone = bone.as_object().ok_or_else(|| {
                ClipCompileError::Invalid(invalid("animation bone must be an object"))
            })?;
            let rotation_relative_to_entity = bone
                .get("relative_to")
                .and_then(|relative| relative.get("rotation"))
                .and_then(Value::as_str)
                == Some("entity");
            // A frame-only bone still changes its orientation frame, even without angles.
            let neutral_rotation =
                rotation_relative_to_entity.then(|| serde_json::json!([0.0, 0.0, 0.0]));
            for (field, property) in [
                ("position", EntityAnimationProperty::Translation),
                ("rotation", EntityAnimationProperty::Rotation),
                ("scale", EntityAnimationProperty::Scale),
            ] {
                let Some(value) = bone.get(field).or_else(|| {
                    neutral_rotation
                        .as_ref()
                        .filter(|_| property == EntityAnimationProperty::Rotation)
                }) else {
                    continue;
                };
                let first_keyframe = local_keyframes.len() as u32;
                let mark = molang.mark();
                match parse_channel(
                    value,
                    &mut local_keyframes,
                    &mut maximum_time,
                    &mut Axes {
                        molang,
                        uncompiled: &mut uncompiled,
                    },
                ) {
                    Ok(()) => {}
                    Err(ChannelError::Unsupported) => {
                        molang.rollback(mark);
                        local_keyframes.truncate(first_keyframe as usize);
                        dropped += 1;
                        continue;
                    }
                    Err(ChannelError::Invalid(error)) => {
                        return Err(ClipCompileError::Invalid(error));
                    }
                }
                let Some(bone_index) = bone_index else {
                    molang.rollback(mark);
                    local_keyframes.truncate(first_keyframe as usize);
                    continue;
                };
                local_channels.push(EntityAnimationChannel {
                    bone: bone_index,
                    property,
                    first_keyframe,
                    keyframe_count: local_keyframes.len() as u32 - first_keyframe,
                    rotation_relative_to_entity,
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
        override_previous: definition
            .get("override_previous_animation")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        geometry: Some(geometry),
        anim_time_update,
    });
    Ok((clip, dropped + uncompiled))
}

fn compile_time_update(value: &Value, molang: &mut MolangCompiler) -> Result<u32, AssetError> {
    let expression = match value {
        Value::String(text) => text.clone(),
        Value::Number(_) => parse_number(value)?.to_string(),
        _ => {
            return Err(invalid(
                "animation anim_time_update must be a Molang string or number",
            ));
        }
    };
    molang.compile(&expression).map_err(|error| {
        invalid(format!(
            "invalid animation anim_time_update expression: {error}"
        ))
    })
}

enum ChannelError {
    Unsupported,
    Invalid(AssetError),
}

impl From<AssetError> for ChannelError {
    fn from(error: AssetError) -> Self {
        Self::Invalid(error)
    }
}

/// Compiles axis expressions; an uncompilable one reads 0.0, as vanilla evaluates it.
struct Axes<'a> {
    molang: &'a mut MolangCompiler,
    uncompiled: &'a mut usize,
}

fn parse_channel(
    value: &Value,
    output: &mut Vec<EntityAnimationKeyframe>,
    maximum_time: &mut f32,
    molang: &mut Axes<'_>,
) -> Result<(), ChannelError> {
    if !value.is_object() {
        let (value, expressions) = parse_vector(value, molang)?;
        output.push(EntityAnimationKeyframe {
            time_seconds: scalar(0.0)?,
            value,
            interpolation: EntityAnimationInterpolation::Linear,
            expressions,
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
                _ => return Err(invalid("unsupported animation interpolation").into()),
            };
            let mut emitted = false;
            for field in ["pre", "post"] {
                if let Some(vector) = object.get(field) {
                    let (value, expressions) = parse_vector(vector, molang)?;
                    output.push(EntityAnimationKeyframe {
                        time_seconds: scalar(time)?,
                        value,
                        interpolation,
                        expressions,
                    });
                    emitted = true;
                }
            }
            if !emitted {
                return Err(invalid("keyframe object lacks pre/post values").into());
            }
        } else {
            let (value, expressions) = parse_vector(value, molang)?;
            output.push(EntityAnimationKeyframe {
                time_seconds: scalar(time)?,
                value,
                interpolation: EntityAnimationInterpolation::Linear,
                expressions,
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

const ZERO: EntityGeometryScalar = EntityGeometryScalar::ZERO;

type ParsedVector = ([EntityGeometryScalar; 3], [Option<u32>; 3]);

fn parse_vector(value: &Value, molang: &mut Axes<'_>) -> Result<ParsedVector, ChannelError> {
    if value.is_number() || value.is_string() {
        let axis = parse_axis(value, molang)?;
        return Ok(([axis.0; 3], [axis.1; 3]));
    }
    let values = value
        .as_array()
        .filter(|values| values.len() == 3)
        .ok_or_else(|| invalid("animation vector must have exactly three components"))?;
    let [x, y, z] = [
        parse_axis(&values[0], molang)?,
        parse_axis(&values[1], molang)?,
        parse_axis(&values[2], molang)?,
    ];
    Ok(([x.0, y.0, z.0], [x.1, y.1, z.1]))
}

fn parse_axis(
    value: &Value,
    axes: &mut Axes<'_>,
) -> Result<(EntityGeometryScalar, Option<u32>), ChannelError> {
    match value {
        Value::String(text) => match text.trim().parse::<f32>() {
            Ok(number) => Ok((scalar(number)?, None)),
            Err(_) => Ok(match axes.molang.compile(text) {
                Ok(expression) => (ZERO, Some(expression)),
                Err(_) => {
                    *axes.uncompiled += 1;
                    (ZERO, None)
                }
            }),
        },
        // Per-axis rotation-order objects are an unsupported authoring form, not malformed.
        Value::Object(_) => Err(ChannelError::Unsupported),
        _ => Ok((scalar(parse_number(value)?)?, None)),
    }
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
