use std::{collections::BTreeMap, fs, path::Path};

use assets::{
    AssetError, EntityAnimationChannel, EntityAnimationClip, EntityAnimationController,
    EntityAnimationInterpolation, EntityAnimationKeyframe, EntityAnimationLoop,
    EntityAnimationProperty, EntityAssetKind, EntityAssetSource, EntityAssetSymbol,
    EntityControllerAnimation, EntityControllerState, EntityControllerTransition, EntityGeometry,
    EntityGeometryScalar, EntityRigAnimationBinding, EntityRigBinding, EntityRigControllerBinding,
    EntityRigFallback,
};
use serde::Serialize;
use serde_json::{Map, Value};

use super::{invalid, json::parse_unique_json, molang::MolangCompiler};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    UnsupportedOptionalExpression,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    MissingRequiredReference,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "detail")]
pub enum CompileReferenceOutcome<T> {
    Resolved(T),
    OptionalStaticFallback {
        source: u32,
        symbol: u32,
        reason: FallbackReason,
    },
    RequiredRigRejected {
        source: u32,
        symbol: u32,
        reason: RejectReason,
    },
}

pub(super) struct AnimationPayload {
    pub clips: Box<[EntityAnimationClip]>,
    pub channels: Box<[EntityAnimationChannel]>,
    pub keyframes: Box<[EntityAnimationKeyframe]>,
    pub controllers: Box<[EntityAnimationController]>,
    pub controller_states: Box<[EntityControllerState]>,
    pub controller_animations: Box<[EntityControllerAnimation]>,
    pub controller_transitions: Box<[EntityControllerTransition]>,
    pub rig_bindings: Box<[EntityRigBinding]>,
    pub rig_animations: Box<[EntityRigAnimationBinding]>,
    pub rig_controllers: Box<[EntityRigControllerBinding]>,
    pub outcomes: Box<[CompileReferenceOutcome<u32>]>,
}

#[derive(Default)]
struct PendingController {
    symbol: u32,
    initial_state: Box<str>,
    states: Vec<PendingState>,
}

#[derive(Default)]
struct PendingState {
    name: Box<str>,
    animations: Vec<(u32, Option<u32>)>,
    transitions: Vec<(Box<str>, u32)>,
    on_entry: Option<u32>,
    on_exit: Option<u32>,
}

pub(super) fn compile(
    root: &Path,
    sources: &[EntityAssetSource],
    symbols: &[EntityAssetSymbol],
    geometries: &[EntityGeometry],
    molang: &mut MolangCompiler,
) -> Result<AnimationPayload, AssetError> {
    let source_indices = sources
        .iter()
        .enumerate()
        .map(|(index, source)| (source.path.as_ref(), index as u32))
        .collect::<BTreeMap<_, _>>();
    let symbol_indices = symbols
        .iter()
        .enumerate()
        .map(|(index, symbol)| {
            (
                (symbol.kind, symbol.identifier.as_ref(), symbol.source_index),
                index as u32,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut bone_indices = BTreeMap::<Box<str>, u32>::new();
    for geometry in geometries {
        for (index, bone) in geometry.bones.iter().enumerate() {
            bone_indices
                .entry(bone.name.to_ascii_lowercase().into_boxed_str())
                .or_insert(index as u32);
        }
    }

    let mut pending_clips = Vec::new();
    for source in sources
        .iter()
        .filter(|source| source.path.starts_with("animations/"))
    {
        let value = read_json(root, source)?;
        let entries = required_object(&value, "animations")?;
        for (identifier, definition) in entries {
            let symbol = *symbol_indices
                .get(&(
                    EntityAssetKind::Animation,
                    identifier.as_str(),
                    source_index(source, &source_indices)?,
                ))
                .ok_or_else(|| invalid("animation symbol is absent from catalog"))?;
            pending_clips.push((
                symbol,
                source.source_path_index(sources)?,
                definition.clone(),
            ));
        }
    }
    pending_clips.sort_by_key(|(symbol, _, _)| *symbol);

    let mut clips = Vec::new();
    let mut channels = Vec::new();
    let mut keyframes = Vec::new();
    for (symbol, source, definition) in pending_clips {
        let definition = definition
            .as_object()
            .ok_or_else(|| invalid("animation definition must be an object"))?;
        if definition.get("bones").is_some_and(has_string_leaf)
            || definition
                .get("animation_length")
                .and_then(Value::as_str)
                .is_some_and(looks_like_expression)
        {
            // The reviewed carrier intentionally has no expression-valued
            // keyframe channel. The owning rig is attributed as a fallback.
            continue;
        }
        let first_channel = channels.len() as u32;
        let mut maximum_time = 0.0_f32;
        if let Some(bones) = definition.get("bones") {
            let bones = bones
                .as_object()
                .ok_or_else(|| invalid("animation bones must be an object"))?;
            for (bone_name, bone) in bones {
                let bone_index = bone_indices
                    .get(bone_name.to_ascii_lowercase().as_str())
                    .copied()
                    .ok_or_else(|| invalid("animation references an unknown bone"))?;
                let bone = bone
                    .as_object()
                    .ok_or_else(|| invalid("animation bone must be an object"))?;
                for (field, property) in [
                    ("position", EntityAnimationProperty::Translation),
                    ("rotation", EntityAnimationProperty::Rotation),
                    ("scale", EntityAnimationProperty::Scale),
                ] {
                    let Some(value) = bone.get(field) else {
                        continue;
                    };
                    let first_keyframe = keyframes.len() as u32;
                    parse_channel(value, &mut keyframes, &mut maximum_time)?;
                    channels.push(EntityAnimationChannel {
                        bone: bone_index,
                        property,
                        first_keyframe,
                        keyframe_count: keyframes.len() as u32 - first_keyframe,
                    });
                }
            }
        }
        let declared_length = definition
            .get("animation_length")
            .map(parse_number)
            .transpose()?
            .unwrap_or(maximum_time);
        let declared_length = declared_length.max(maximum_time);
        let loop_mode = match definition.get("loop") {
            None | Some(Value::Bool(false)) => EntityAnimationLoop::Once,
            Some(Value::Bool(true)) => EntityAnimationLoop::Loop,
            Some(Value::String(value)) if value == "hold_on_last_frame" => {
                EntityAnimationLoop::HoldOnLastFrame
            }
            _ => return Err(invalid("unsupported animation loop mode")),
        };
        clips.push(EntityAnimationClip {
            symbol,
            length_seconds: scalar(declared_length)?,
            loop_mode,
            first_channel,
            channel_count: channels.len() as u32 - first_channel,
            source,
        });
    }
    let clip_indices = clips
        .iter()
        .enumerate()
        .map(|(index, clip)| {
            (
                symbols[clip.symbol as usize].identifier.as_ref(),
                index as u32,
            )
        })
        .collect::<BTreeMap<_, _>>();

    compile_render_collections(root, sources, geometries, molang)?;
    let pending_controllers = compile_pending_controllers(
        root,
        sources,
        symbols,
        &symbol_indices,
        &clip_indices,
        molang,
    )?;
    let molang_preview_names = pending_controllers
        .iter()
        .flat_map(|controller| controller.states.iter().map(|state| state.name.as_ref()))
        .collect::<Vec<_>>();
    for name in molang_preview_names {
        molang.add_name(name)?;
    }

    // Name indices are stable because Name sorts before Query/Variable/Temporary.
    let mut names = pending_controllers
        .iter()
        .flat_map(|controller| controller.states.iter().map(|state| state.name.clone()))
        .collect::<Vec<_>>();
    let (rig_payload, rig_names) = compile_rigs(
        root,
        sources,
        symbols,
        geometries,
        &clip_indices,
        &pending_controllers,
    )?;
    names.extend(rig_names.iter().cloned());
    names.sort();
    names.dedup();
    for name in &names {
        molang.add_name(name)?;
    }
    let name_index = |name: &str| {
        names
            .binary_search_by(|candidate| candidate.as_ref().cmp(name))
            .map(|index| index as u32)
            .map_err(|_| invalid("Molang name is absent"))
    };

    let mut controllers = Vec::new();
    let mut controller_states = Vec::new();
    let mut controller_animations = Vec::new();
    let mut controller_transitions = Vec::new();
    for controller in pending_controllers {
        let first_state = controller_states.len() as u32;
        let state_indices = controller
            .states
            .iter()
            .enumerate()
            .map(|(index, state)| (state.name.as_ref(), index as u16))
            .collect::<BTreeMap<_, _>>();
        for state in &controller.states {
            let first_animation = controller_animations.len() as u32;
            controller_animations.extend(state.animations.iter().map(|(clip, weight)| {
                EntityControllerAnimation {
                    clip: *clip,
                    weight: *weight,
                }
            }));
            let first_transition = controller_transitions.len() as u32;
            for (target, condition) in &state.transitions {
                controller_transitions.push(EntityControllerTransition {
                    target_state: *state_indices
                        .get(target.as_ref())
                        .ok_or_else(|| invalid("controller transition target is absent"))?,
                    condition: *condition,
                });
            }
            controller_states.push(EntityControllerState {
                name: name_index(&state.name)?,
                first_animation,
                animation_count: (controller_animations.len() as u32 - first_animation) as u16,
                first_transition,
                transition_count: (controller_transitions.len() as u32 - first_transition) as u16,
                on_entry: state.on_entry,
                on_exit: state.on_exit,
            });
        }
        let initial_state = *state_indices
            .get(controller.initial_state.as_ref())
            .ok_or_else(|| invalid("controller initial state is absent"))?;
        controllers.push(EntityAnimationController {
            symbol: controller.symbol,
            first_state,
            state_count: controller.states.len() as u16,
            initial_state,
        });
    }

    let controller_indices = controllers
        .iter()
        .enumerate()
        .map(|(index, controller)| {
            (
                symbols[controller.symbol as usize].identifier.as_ref(),
                index as u32,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let finalized_rigs = rig_payload.finalize(&name_index, &controller_indices)?;

    Ok(AnimationPayload {
        clips: clips.into_boxed_slice(),
        channels: channels.into_boxed_slice(),
        keyframes: keyframes.into_boxed_slice(),
        controllers: controllers.into_boxed_slice(),
        controller_states: controller_states.into_boxed_slice(),
        controller_animations: controller_animations.into_boxed_slice(),
        controller_transitions: controller_transitions.into_boxed_slice(),
        rig_bindings: finalized_rigs.bindings,
        rig_animations: finalized_rigs.animations,
        rig_controllers: finalized_rigs.controllers,
        outcomes: finalized_rigs.outcomes,
    })
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

fn has_string_leaf(value: &Value) -> bool {
    match value {
        Value::String(value) => !matches!(value.as_str(), "linear" | "step" | "catmullrom"),
        Value::Array(values) => values.iter().any(has_string_leaf),
        Value::Object(values) => values.values().any(has_string_leaf),
        _ => false,
    }
}

fn looks_like_expression(value: &str) -> bool {
    value.contains("query.")
        || value.contains("variable.")
        || value.contains("temp.")
        || value.contains("math.")
        || value
            .bytes()
            .any(|byte| matches!(byte, b'+' | b'*' | b'/' | b'?' | b'('))
}

fn compile_pending_controllers(
    root: &Path,
    sources: &[EntityAssetSource],
    symbols: &[EntityAssetSymbol],
    symbol_indices: &BTreeMap<(EntityAssetKind, &str, u32), u32>,
    clip_indices: &BTreeMap<&str, u32>,
    molang: &mut MolangCompiler,
) -> Result<Vec<PendingController>, AssetError> {
    let aliases = collect_animation_aliases(root, sources)?;
    let mut output = Vec::new();
    for source in sources
        .iter()
        .filter(|source| source.path.starts_with("animation_controllers/"))
    {
        let source_index = source.source_path_index(sources)?;
        let value = read_json(root, source)?;
        'controller: for (identifier, definition) in
            required_object(&value, "animation_controllers")?
        {
            let symbol = *symbol_indices
                .get(&(
                    EntityAssetKind::AnimationController,
                    identifier,
                    source_index,
                ))
                .ok_or_else(|| invalid("controller symbol is absent"))?;
            let definition = definition
                .as_object()
                .ok_or_else(|| invalid("animation controller must be an object"))?;
            let states = required_object(
                definition
                    .get("states")
                    .ok_or_else(|| invalid("controller states are absent"))?,
                "",
            )?;
            if states.is_empty() {
                return Err(invalid("controller has no states"));
            }
            let mut pending_states = Vec::new();
            for (name, state) in states {
                let state = state
                    .as_object()
                    .ok_or_else(|| invalid("controller state must be an object"))?;
                let mut animations = Vec::new();
                if let Some(entries) = state.get("animations") {
                    for entry in entries
                        .as_array()
                        .ok_or_else(|| invalid("state animations must be an array"))?
                    {
                        match entry {
                            Value::String(identifier) => {
                                let Ok(clip) = resolve_clip(identifier, clip_indices, &aliases)
                                else {
                                    continue 'controller;
                                };
                                animations.push((clip, None));
                            }
                            Value::Object(weighted) if weighted.len() == 1 => {
                                let (identifier, weight) = weighted.iter().next().unwrap();
                                let weight = weight
                                    .as_str()
                                    .ok_or_else(|| invalid("animation weight must be Molang"))?;
                                let (Ok(clip), Ok(weight)) = (
                                    resolve_clip(identifier, clip_indices, &aliases),
                                    molang.compile(weight),
                                ) else {
                                    continue 'controller;
                                };
                                animations.push((clip, Some(weight)));
                            }
                            _ => continue 'controller,
                        }
                    }
                }
                let mut transitions = Vec::new();
                if let Some(entries) = state.get("transitions") {
                    for entry in entries
                        .as_array()
                        .ok_or_else(|| invalid("state transitions must be an array"))?
                    {
                        let entry = entry
                            .as_object()
                            .filter(|entry| entry.len() == 1)
                            .ok_or_else(|| invalid("invalid controller transition"))?;
                        let (target, condition) = entry.iter().next().unwrap();
                        let Some(condition) = condition.as_str() else {
                            continue 'controller;
                        };
                        let Ok(condition) = molang.compile(condition) else {
                            continue 'controller;
                        };
                        transitions.push((target.as_str().into(), condition));
                    }
                }
                let on_entry = match compile_optional_expression(state.get("on_entry"), molang) {
                    Ok(value) => value,
                    Err(_) => continue 'controller,
                };
                let on_exit = match compile_optional_expression(state.get("on_exit"), molang) {
                    Ok(value) => value,
                    Err(_) => continue 'controller,
                };
                pending_states.push(PendingState {
                    name: name.as_str().into(),
                    animations,
                    transitions,
                    on_entry,
                    on_exit,
                });
            }
            pending_states.sort_by(|left, right| left.name.cmp(&right.name));
            let initial_state = definition
                .get("initial_state")
                .and_then(Value::as_str)
                .unwrap_or_else(|| pending_states[0].name.as_ref());
            output.push(PendingController {
                symbol,
                initial_state: initial_state.into(),
                states: pending_states,
            });
        }
    }
    output.sort_by_key(|controller| controller.symbol);
    let _ = symbols;
    Ok(output)
}

fn compile_optional_expression(
    value: Option<&Value>,
    molang: &mut MolangCompiler,
) -> Result<Option<u32>, AssetError> {
    match value {
        None => Ok(None),
        Some(Value::String(expression)) => molang.compile(expression).map(Some),
        _ => Err(invalid(
            "controller entry/exit expression must be one string",
        )),
    }
}

fn compile_render_collections(
    root: &Path,
    sources: &[EntityAssetSource],
    geometries: &[EntityGeometry],
    molang: &mut MolangCompiler,
) -> Result<(), AssetError> {
    let geometry_indices = geometries
        .iter()
        .enumerate()
        .map(|(index, geometry)| (geometry.identifier.as_ref(), index as f32))
        .collect::<BTreeMap<_, _>>();
    for source in sources
        .iter()
        .filter(|source| source.path.starts_with("render_controllers/"))
    {
        let value = read_json(root, source)?;
        for (controller_name, definition) in required_object(&value, "render_controllers")? {
            let definition = definition
                .as_object()
                .ok_or_else(|| invalid("render controller must be an object"))?;
            let mut local_collections = BTreeMap::<Box<str>, Box<str>>::new();
            if let Some(arrays) = definition.get("arrays") {
                let arrays = arrays
                    .as_object()
                    .ok_or_else(|| invalid("render-controller arrays must be an object"))?;
                let Some(geometries_array) = arrays.get("geometries") else {
                    continue;
                };
                let geometries_array = geometries_array
                    .as_object()
                    .ok_or_else(|| invalid("render-controller arrays.geometries is invalid"))?;
                for (name, members) in geometries_array {
                    let values = members
                        .as_array()
                        .ok_or_else(|| invalid("render-controller collection must be an array"))?
                        .iter()
                        .map(|member| {
                            let member = member
                                .as_str()
                                .ok_or_else(|| invalid("collection member must be a name"))?;
                            if member
                                .get(..9)
                                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("geometry."))
                            {
                                Ok(0.0)
                            } else {
                                geometry_indices.get(member).copied().ok_or_else(|| {
                                    invalid(format!(
                                        "unknown geometry collection member `{member}`"
                                    ))
                                })
                            }
                        })
                        .collect::<Result<Vec<_>, AssetError>>()?;
                    let scoped_name = format!("{}#{controller_name}#{name}", source.path);
                    molang.add_collection(&scoped_name, values)?;
                    local_collections.insert(name.as_str().into(), scoped_name.into());
                }
            }
            if let Some(geometry) = definition.get("geometry").and_then(Value::as_str)
                && let Some((collection, index)) = split_collection_selection(geometry)
                && let Some(scoped) = local_collections.get(collection)
            {
                let _optional_selection = molang.compile_collection_selection(scoped, index).ok();
            }
        }
    }
    Ok(())
}

fn split_collection_selection(value: &str) -> Option<(&str, &str)> {
    let open = value.find('[')?;
    value
        .ends_with(']')
        .then(|| (&value[..open], &value[open + 1..value.len() - 1]))
}

struct PendingRigPayload {
    rigs: Vec<PendingRig>,
    outcomes: Vec<CompileReferenceOutcome<u32>>,
}

struct PendingRig {
    entity_symbol: u32,
    geometry: u32,
    render_controller: u32,
    animations: Vec<(Box<str>, u32)>,
    controllers: Vec<(Box<str>, Box<str>)>,
    fallback: EntityRigFallback,
}

struct FinalRigPayload {
    bindings: Box<[EntityRigBinding]>,
    animations: Box<[EntityRigAnimationBinding]>,
    controllers: Box<[EntityRigControllerBinding]>,
    outcomes: Box<[CompileReferenceOutcome<u32>]>,
}

impl PendingRigPayload {
    fn finalize(
        self,
        name_index: &impl Fn(&str) -> Result<u32, AssetError>,
        controller_indices: &BTreeMap<&str, u32>,
    ) -> Result<FinalRigPayload, AssetError> {
        let mut bindings = Vec::new();
        let mut animations = Vec::new();
        let mut controllers = Vec::new();
        for rig in self.rigs {
            let first_animation = animations.len() as u32;
            for (name, clip) in rig.animations {
                animations.push(EntityRigAnimationBinding {
                    name: name_index(&name)?,
                    clip,
                });
            }
            let first_controller = controllers.len() as u32;
            for (name, controller) in rig.controllers {
                controllers.push(EntityRigControllerBinding {
                    name: name_index(&name)?,
                    controller: *controller_indices
                        .get(controller.as_ref())
                        .ok_or_else(|| invalid("rig controller is absent"))?,
                });
            }
            bindings.push(EntityRigBinding {
                entity_symbol: rig.entity_symbol,
                geometry: rig.geometry,
                render_controller: rig.render_controller,
                first_animation,
                animation_count: (animations.len() as u32 - first_animation) as u16,
                first_controller,
                controller_count: (controllers.len() as u32 - first_controller) as u16,
                fallback: rig.fallback,
            });
        }
        Ok(FinalRigPayload {
            bindings: bindings.into_boxed_slice(),
            animations: animations.into_boxed_slice(),
            controllers: controllers.into_boxed_slice(),
            outcomes: self.outcomes.into_boxed_slice(),
        })
    }
}

fn compile_rigs(
    root: &Path,
    sources: &[EntityAssetSource],
    symbols: &[EntityAssetSymbol],
    geometries: &[EntityGeometry],
    clip_indices: &BTreeMap<&str, u32>,
    controllers: &[PendingController],
) -> Result<(PendingRigPayload, Vec<Box<str>>), AssetError> {
    let geometry_indices = geometries
        .iter()
        .enumerate()
        .map(|(index, geometry)| (geometry.identifier.as_ref(), index as u32))
        .collect::<BTreeMap<_, _>>();
    let render_indices = symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| symbol.kind == EntityAssetKind::RenderController)
        .map(|(index, symbol)| (symbol.identifier.as_ref(), index as u32))
        .collect::<BTreeMap<_, _>>();
    let controller_names = controllers
        .iter()
        .map(|controller| symbols[controller.symbol as usize].identifier.as_ref())
        .collect::<std::collections::BTreeSet<_>>();
    let mut rigs = Vec::new();
    let mut outcomes = Vec::new();
    let mut names = Vec::new();
    for (entity_symbol, entity) in symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| symbol.kind == EntityAssetKind::Entity)
    {
        let source = &sources[entity.source_index as usize];
        let value = read_json(root, source)?;
        let description = value
            .get("minecraft:client_entity")
            .and_then(|value| value.get("description"))
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("client entity description is absent"))?;
        let Some(geometry_name) = first_string(description.get("geometry")) else {
            outcomes.push(CompileReferenceOutcome::RequiredRigRejected {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: RejectReason::MissingRequiredReference,
            });
            continue;
        };
        let Some(&geometry) = geometry_indices.get(geometry_name) else {
            outcomes.push(CompileReferenceOutcome::RequiredRigRejected {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: RejectReason::MissingRequiredReference,
            });
            continue;
        };
        let Some((render_name, optional_condition)) =
            first_render_controller(description.get("render_controllers"))
        else {
            outcomes.push(CompileReferenceOutcome::RequiredRigRejected {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: RejectReason::MissingRequiredReference,
            });
            continue;
        };
        let Some(&render_controller) = render_indices.get(render_name) else {
            outcomes.push(CompileReferenceOutcome::RequiredRigRejected {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: RejectReason::MissingRequiredReference,
            });
            continue;
        };
        let mut animation_bindings: Vec<(Box<str>, u32)> = Vec::new();
        let mut controller_bindings: Vec<(Box<str>, Box<str>)> = Vec::new();
        let mut rejected = false;
        let mut static_fallback = false;
        if let Some(animations) = description.get("animations").and_then(Value::as_object) {
            for (name, target) in animations {
                let target = target
                    .as_str()
                    .ok_or_else(|| invalid("entity animation binding must be a string"))?;
                names.push(name.as_str().into());
                if target.starts_with("controller.animation.") {
                    if controller_names.contains(target) {
                        controller_bindings.push((name.as_str().into(), target.into()));
                    } else {
                        static_fallback = true;
                    }
                } else if let Some(&clip) = clip_indices.get(target) {
                    animation_bindings.push((name.as_str().into(), clip));
                } else {
                    rejected = true;
                }
            }
        }
        if rejected {
            outcomes.push(CompileReferenceOutcome::RequiredRigRejected {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: RejectReason::MissingRequiredReference,
            });
            continue;
        }
        animation_bindings.sort_by(|left, right| left.0.cmp(&right.0));
        controller_bindings.sort_by(|left, right| left.0.cmp(&right.0));
        static_fallback |= optional_condition.is_some_and(|expression| {
            super::molang::MolangCompiler::default()
                .compile(expression)
                .is_err()
        });
        let fallback = if static_fallback {
            outcomes.push(CompileReferenceOutcome::OptionalStaticFallback {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: FallbackReason::UnsupportedOptionalExpression,
            });
            EntityRigFallback::GeometryOnly
        } else {
            EntityRigFallback::Skip
        };
        let rig_index = rigs.len() as u32;
        rigs.push(PendingRig {
            entity_symbol: entity_symbol as u32,
            geometry,
            render_controller,
            animations: animation_bindings,
            controllers: controller_bindings,
            fallback,
        });
        outcomes.push(CompileReferenceOutcome::Resolved(rig_index));
    }
    Ok((PendingRigPayload { rigs, outcomes }, names))
}

fn first_string(value: Option<&Value>) -> Option<&str> {
    match value? {
        Value::String(value) => Some(value),
        Value::Object(values) => values.values().find_map(Value::as_str),
        Value::Array(values) => values.iter().find_map(Value::as_str),
        _ => None,
    }
}

fn first_render_controller(value: Option<&Value>) -> Option<(&str, Option<&str>)> {
    for entry in value?.as_array()? {
        match entry {
            Value::String(identifier) => return Some((identifier, None)),
            Value::Object(conditional) => {
                if let Some((identifier, condition)) = conditional.iter().next() {
                    return Some((identifier, condition.as_str()));
                }
            }
            _ => {}
        }
    }
    None
}

fn resolve_clip(
    identifier: &str,
    clips: &BTreeMap<&str, u32>,
    aliases: &BTreeMap<Box<str>, Box<str>>,
) -> Result<u32, AssetError> {
    let identifier = aliases
        .get(identifier)
        .map_or(identifier, |identifier| identifier.as_ref());
    clips
        .get(identifier)
        .copied()
        .ok_or_else(|| invalid("controller references a missing animation"))
}

fn collect_animation_aliases(
    root: &Path,
    sources: &[EntityAssetSource],
) -> Result<BTreeMap<Box<str>, Box<str>>, AssetError> {
    let mut candidates = BTreeMap::<Box<str>, Option<Box<str>>>::new();
    for source in sources
        .iter()
        .filter(|source| source.path.starts_with("entity/"))
    {
        let value = read_json(root, source)?;
        let Some(animations) = value
            .get("minecraft:client_entity")
            .and_then(|entity| entity.get("description"))
            .and_then(|description| description.get("animations"))
            .and_then(Value::as_object)
        else {
            continue;
        };
        for (alias, target) in animations {
            let Some(target) = target.as_str() else {
                continue;
            };
            if target.starts_with("controller.animation.") {
                continue;
            }
            candidates
                .entry(alias.as_str().into())
                .and_modify(|current| {
                    if current.as_deref() != Some(target) {
                        *current = None;
                    }
                })
                .or_insert_with(|| Some(target.into()));
        }
    }
    Ok(candidates
        .into_iter()
        .filter_map(|(alias, target)| target.map(|target| (alias, target)))
        .collect())
}

fn read_json(root: &Path, source: &EntityAssetSource) -> Result<Value, AssetError> {
    let path = root.join(source.path.as_ref());
    let bytes = fs::read(&path).map_err(|source| AssetError::Io {
        path: path.clone(),
        source,
    })?;
    parse_unique_json(&path, &bytes)
}

fn required_object<'a>(
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

fn source_index(
    source: &EntityAssetSource,
    indices: &BTreeMap<&str, u32>,
) -> Result<u32, AssetError> {
    indices
        .get(source.path.as_ref())
        .copied()
        .ok_or_else(|| invalid("entity source is absent"))
}

trait SourceIndex {
    fn source_path_index(&self, sources: &[EntityAssetSource]) -> Result<u32, AssetError>;
}

impl SourceIndex for EntityAssetSource {
    fn source_path_index(&self, sources: &[EntityAssetSource]) -> Result<u32, AssetError> {
        sources
            .binary_search_by(|source| source.path.cmp(&self.path))
            .map(|index| index as u32)
            .map_err(|_| invalid("entity source is absent"))
    }
}
