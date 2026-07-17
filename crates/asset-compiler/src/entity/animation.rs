use std::{collections::BTreeMap, path::Path};

use assets::{
    AssetError, EntityAnimationChannel, EntityAnimationClip, EntityAnimationController,
    EntityAnimationKeyframe, EntityAssetKind, EntityAssetSource, EntityAssetSymbol,
    EntityControllerAnimation, EntityControllerState, EntityControllerTransition, EntityGeometry,
    EntityRigAnimationBinding, EntityRigBinding, EntityRigControllerBinding, EntityRigFallback,
    EntityRigGeometryBinding,
};
use serde_json::{Map, Value};

use super::{SourcePayloads, invalid, molang::MolangCompiler};

mod clip;
mod environment;
mod outcome;

use clip::{
    ClipCompileError, compile_clip_for_geometry, has_string_leaf, looks_like_expression, read_json,
    required_object, source_index,
};
use environment::{
    EntityEnvironment, GeometrySelection, GeometrySelections,
    collect as collect_entity_environments, compile_geometry_selections,
    controller_clip_references, default_geometry, effective_bone_names, parse_aliases,
    selection_for, unique_geometry_indices,
};
pub use outcome::{CompileReferenceOutcome, FallbackReason, RejectReason};

pub(super) struct AnimationPayload {
    pub clips: Box<[EntityAnimationClip]>,
    pub channels: Box<[EntityAnimationChannel]>,
    pub keyframes: Box<[EntityAnimationKeyframe]>,
    pub controllers: Box<[EntityAnimationController]>,
    pub controller_states: Box<[EntityControllerState]>,
    pub controller_animations: Box<[EntityControllerAnimation]>,
    pub controller_transitions: Box<[EntityControllerTransition]>,
    pub rig_bindings: Box<[EntityRigBinding]>,
    pub rig_geometries: Box<[EntityRigGeometryBinding]>,
    pub rig_animations: Box<[EntityRigAnimationBinding]>,
    pub rig_controllers: Box<[EntityRigControllerBinding]>,
    pub outcomes: Box<[CompileReferenceOutcome<u32>]>,
}

#[derive(Default)]
struct PendingController {
    owner_entity: u32,
    geometry: u32,
    symbol: u32,
    initial_state: Box<str>,
    states: Vec<PendingState>,
}

type ClipIndices = BTreeMap<(Box<str>, u32), u32>;

struct RigInputs<'a> {
    clip_indices: &'a ClipIndices,
    controllers: &'a [PendingController],
    geometry_selections: &'a GeometrySelections,
}

struct ControllerInputs<'a> {
    symbol_indices: &'a BTreeMap<(EntityAssetKind, &'a str, u32), u32>,
    clip_indices: &'a ClipIndices,
    environments: &'a [EntityEnvironment],
    geometry_selections: &'a GeometrySelections,
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
    payloads: &SourcePayloads,
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
    let environments = collect_entity_environments(root, payloads, sources, symbols, geometries)?;
    let effective_bones = effective_bone_names(geometries)?;
    let controller_clip_references = controller_clip_references(root, payloads, sources)?;
    let geometry_selections =
        compile_geometry_selections(root, payloads, sources, geometries, &environments, molang)?;

    let mut pending_clips = Vec::new();
    for source in sources
        .iter()
        .filter(|source| source.path.starts_with("animations/"))
    {
        let value = read_json(root, payloads, source)?;
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

    let mut outcomes = Vec::new();
    let mut clips = Vec::new();
    let mut channels = Vec::new();
    let mut keyframes = Vec::new();
    let mut clip_indices = BTreeMap::<(Box<str>, u32), u32>::new();
    for (symbol, source, definition) in pending_clips {
        let identifier = symbols[symbol as usize].identifier.as_ref();
        let used_geometries = environments
            .iter()
            .flat_map(|environment| {
                let referenced = environment.geometry.is_some_and(|_| {
                    environment
                        .animation_aliases
                        .values()
                        .any(|target| target.as_ref() == identifier)
                        || environment.controller_aliases.values().any(|controller| {
                            controller_clip_references
                                .get(controller)
                                .is_some_and(|references| {
                                    references.iter().any(|reference| {
                                        environment
                                            .animation_aliases
                                            .get(reference)
                                            .map_or(reference.as_ref(), AsRef::as_ref)
                                            == identifier
                                    })
                                })
                        })
                });
                let mut geometries = Vec::new();
                if referenced && let Some(default_geometry) = environment.geometry {
                    geometries.push(default_geometry);
                    if let Some(GeometrySelection::Supported(selectable)) =
                        selection_for(environment, &geometry_selections)
                    {
                        geometries.extend(selectable.iter().map(|candidate| candidate.geometry));
                    }
                }
                geometries
            })
            .collect::<std::collections::BTreeSet<_>>();
        if used_geometries.is_empty() {
            outcomes.push(CompileReferenceOutcome::OptionalStaticFallback {
                source,
                symbol,
                reason: FallbackReason::UnreferencedDefinition,
            });
            continue;
        }
        let definition = definition
            .as_object()
            .ok_or_else(|| invalid("animation definition must be an object"))?;
        if definition.get("bones").is_some_and(has_string_leaf)
            || definition
                .get("animation_length")
                .and_then(Value::as_str)
                .is_some_and(looks_like_expression)
        {
            outcomes.push(CompileReferenceOutcome::OptionalStaticFallback {
                source,
                symbol,
                reason: FallbackReason::UnsupportedOptionalExpression,
            });
            continue;
        }
        for geometry in used_geometries {
            match compile_clip_for_geometry(
                symbol,
                source,
                definition,
                &effective_bones[geometry as usize],
                &mut clips,
                &mut channels,
                &mut keyframes,
            ) {
                Ok(clip) => {
                    clip_indices.insert((identifier.into(), geometry), clip);
                }
                Err(ClipCompileError::UnknownBone) => {
                    outcomes.push(CompileReferenceOutcome::OptionalStaticFallback {
                        source,
                        symbol,
                        reason: FallbackReason::UnsupportedGeometryBinding,
                    });
                }
                Err(ClipCompileError::Invalid(error)) => return Err(error),
            }
        }
    }
    let (pending_controllers, controller_fallbacks) = compile_pending_controllers(
        root,
        payloads,
        sources,
        ControllerInputs {
            symbol_indices: &symbol_indices,
            clip_indices: &clip_indices,
            environments: &environments,
            geometry_selections: &geometry_selections,
        },
        molang,
    )?;
    let compiled_controller_symbols = pending_controllers
        .iter()
        .map(|controller| controller.symbol)
        .collect::<std::collections::BTreeSet<_>>();
    for (symbol, definition) in symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| symbol.kind == EntityAssetKind::AnimationController)
    {
        if controller_fallbacks.contains(&(symbol as u32)) {
            outcomes.push(CompileReferenceOutcome::OptionalStaticFallback {
                source: definition.source_index,
                symbol: symbol as u32,
                reason: FallbackReason::UnsupportedOptionalExpression,
            });
        }
        if compiled_controller_symbols.contains(&(symbol as u32)) {
            continue;
        }
        let referenced = environments.iter().any(|environment| {
            environment
                .controller_aliases
                .values()
                .any(|target| target == &definition.identifier)
        });
        outcomes.push(CompileReferenceOutcome::OptionalStaticFallback {
            source: definition.source_index,
            symbol: symbol as u32,
            reason: if referenced {
                FallbackReason::UnsupportedOptionalExpression
            } else {
                FallbackReason::UnreferencedDefinition
            },
        });
    }
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
        payloads,
        sources,
        symbols,
        geometries,
        RigInputs {
            clip_indices: &clip_indices,
            controllers: &pending_controllers,
            geometry_selections: &geometry_selections,
        },
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
    let controller_keys = pending_controllers
        .iter()
        .map(|controller| {
            (
                symbols[controller.symbol as usize].identifier.clone(),
                controller.owner_entity,
                controller.geometry,
            )
        })
        .collect::<Vec<_>>();
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

    let controller_indices = controller_keys
        .into_iter()
        .enumerate()
        .map(|(index, key)| (key, index as u32))
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
        rig_geometries: finalized_rigs.geometries,
        rig_animations: finalized_rigs.animations,
        rig_controllers: finalized_rigs.controllers,
        outcomes: {
            outcomes.extend(finalized_rigs.outcomes);
            outcomes.into_boxed_slice()
        },
    })
}

fn unique_symbol_indices(
    symbols: &[EntityAssetSymbol],
    kind: EntityAssetKind,
) -> BTreeMap<Box<str>, Option<u32>> {
    let mut indices = BTreeMap::new();
    for (index, symbol) in symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| symbol.kind == kind)
    {
        indices
            .entry(symbol.identifier.clone())
            .and_modify(|value| *value = None)
            .or_insert(Some(index as u32));
    }
    indices
}

fn compile_pending_controllers(
    root: &Path,
    payloads: &SourcePayloads,
    sources: &[EntityAssetSource],
    inputs: ControllerInputs<'_>,
    molang: &mut MolangCompiler,
) -> Result<(Vec<PendingController>, std::collections::BTreeSet<u32>), AssetError> {
    let ControllerInputs {
        symbol_indices,
        clip_indices,
        environments,
        geometry_selections,
    } = inputs;
    let mut output = Vec::new();
    let mut fallbacks = std::collections::BTreeSet::new();
    for source in sources
        .iter()
        .filter(|source| source.path.starts_with("animation_controllers/"))
    {
        let source_index = source.source_path_index(sources)?;
        let value = read_json(root, payloads, source)?;
        for (identifier, definition) in required_object(&value, "animation_controllers")? {
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
            for environment in environments.iter().filter(|environment| {
                environment
                    .controller_aliases
                    .values()
                    .any(|target| target.as_ref() == identifier)
            }) {
                let Some(default_geometry) = environment.geometry else {
                    continue;
                };
                let mut geometries = std::collections::BTreeSet::from([default_geometry]);
                if let Some(GeometrySelection::Supported(selectable)) =
                    selection_for(environment, geometry_selections)
                {
                    geometries.extend(selectable.iter().map(|candidate| candidate.geometry));
                }
                for geometry in geometries {
                    let mut transaction = molang.clone();
                    match compile_one_controller(
                        symbol,
                        environment.entity_symbol,
                        definition,
                        clip_indices,
                        &environment.animation_aliases,
                        geometry,
                        &mut transaction,
                    ) {
                        Ok(mut controller) => {
                            controller.owner_entity = environment.entity_symbol;
                            controller.geometry = geometry;
                            *molang = transaction;
                            output.push(controller);
                        }
                        Err(_) => {
                            fallbacks.insert(symbol);
                        }
                    }
                }
            }
        }
    }
    output.sort_by_key(|controller| {
        (
            controller.symbol,
            controller.owner_entity,
            controller.geometry,
        )
    });
    Ok((output, fallbacks))
}

fn compile_one_controller(
    symbol: u32,
    owner_entity: u32,
    definition: &Map<String, Value>,
    clip_indices: &ClipIndices,
    aliases: &BTreeMap<Box<str>, Box<str>>,
    geometry: u32,
    molang: &mut MolangCompiler,
) -> Result<PendingController, AssetError> {
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
                    Value::String(identifier) => animations.push((
                        resolve_clip(identifier, clip_indices, aliases, geometry)?,
                        None,
                    )),
                    Value::Object(weighted) if weighted.len() == 1 => {
                        let (identifier, weight) = weighted.iter().next().unwrap();
                        let weight = weight
                            .as_str()
                            .ok_or_else(|| invalid("animation weight must be Molang"))?;
                        animations.push((
                            resolve_clip(identifier, clip_indices, aliases, geometry)?,
                            Some(molang.compile(weight)?),
                        ));
                    }
                    _ => return Err(invalid("invalid controller animation binding")),
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
                let condition = condition
                    .as_str()
                    .ok_or_else(|| invalid("controller transition condition must be Molang"))?;
                transitions.push((target.as_str().into(), molang.compile(condition)?));
            }
        }
        pending_states.push(PendingState {
            name: name.as_str().into(),
            animations,
            transitions,
            on_entry: compile_optional_expression(state.get("on_entry"), molang)?,
            on_exit: compile_optional_expression(state.get("on_exit"), molang)?,
        });
    }
    pending_states.sort_by(|left, right| left.name.cmp(&right.name));
    let initial_state = definition
        .get("initial_state")
        .and_then(Value::as_str)
        .unwrap_or_else(|| pending_states[0].name.as_ref());
    Ok(PendingController {
        owner_entity,
        geometry,
        symbol,
        initial_state: initial_state.into(),
        states: pending_states,
    })
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

struct PendingRigPayload {
    rigs: Vec<PendingRig>,
    outcomes: Vec<CompileReferenceOutcome<u32>>,
}

struct PendingRig {
    entity_symbol: u32,
    render_controller: u32,
    geometries: Vec<PendingRigGeometry>,
    fallback: EntityRigFallback,
}

struct PendingRigGeometry {
    geometry: u32,
    condition: Option<u32>,
    animations: Vec<(Box<str>, u32)>,
    controllers: Vec<(Box<str>, Box<str>)>,
}

struct FinalRigPayload {
    bindings: Box<[EntityRigBinding]>,
    geometries: Box<[EntityRigGeometryBinding]>,
    animations: Box<[EntityRigAnimationBinding]>,
    controllers: Box<[EntityRigControllerBinding]>,
    outcomes: Box<[CompileReferenceOutcome<u32>]>,
}

impl PendingRigPayload {
    fn finalize(
        self,
        name_index: &impl Fn(&str) -> Result<u32, AssetError>,
        controller_indices: &BTreeMap<(Box<str>, u32, u32), u32>,
    ) -> Result<FinalRigPayload, AssetError> {
        let mut bindings = Vec::new();
        let mut geometries = Vec::new();
        let mut animations = Vec::new();
        let mut controllers = Vec::new();
        for rig in self.rigs {
            let first_geometry = geometries.len() as u32;
            for candidate in rig.geometries {
                let first_animation = animations.len() as u32;
                for (name, clip) in candidate.animations {
                    animations.push(EntityRigAnimationBinding {
                        name: name_index(&name)?,
                        clip,
                    });
                }
                let first_controller = controllers.len() as u32;
                for (name, controller) in candidate.controllers {
                    controllers.push(EntityRigControllerBinding {
                        name: name_index(&name)?,
                        controller: *controller_indices
                            .get(&(controller, rig.entity_symbol, candidate.geometry))
                            .ok_or_else(|| invalid("rig controller is absent"))?,
                    });
                }
                geometries.push(EntityRigGeometryBinding {
                    geometry: candidate.geometry,
                    condition: candidate.condition,
                    first_animation,
                    animation_count: (animations.len() as u32 - first_animation) as u16,
                    first_controller,
                    controller_count: (controllers.len() as u32 - first_controller) as u16,
                });
            }
            bindings.push(EntityRigBinding {
                entity_symbol: rig.entity_symbol,
                render_controller: rig.render_controller,
                first_geometry,
                geometry_count: (geometries.len() as u32 - first_geometry) as u16,
                fallback: rig.fallback,
            });
        }
        Ok(FinalRigPayload {
            bindings: bindings.into_boxed_slice(),
            geometries: geometries.into_boxed_slice(),
            animations: animations.into_boxed_slice(),
            controllers: controllers.into_boxed_slice(),
            outcomes: self.outcomes.into_boxed_slice(),
        })
    }
}

fn compile_rigs(
    root: &Path,
    payloads: &SourcePayloads,
    sources: &[EntityAssetSource],
    symbols: &[EntityAssetSymbol],
    geometries: &[EntityGeometry],
    inputs: RigInputs<'_>,
) -> Result<(PendingRigPayload, Vec<Box<str>>), AssetError> {
    let RigInputs {
        clip_indices,
        controllers,
        geometry_selections,
    } = inputs;
    let geometry_indices = unique_geometry_indices(geometries);
    let render_indices = unique_symbol_indices(symbols, EntityAssetKind::RenderController);
    let animation_symbols = unique_symbol_indices(symbols, EntityAssetKind::Animation);
    let controller_symbols = unique_symbol_indices(symbols, EntityAssetKind::AnimationController);
    let controller_names = controllers
        .iter()
        .map(|controller| {
            (
                symbols[controller.symbol as usize].identifier.as_ref(),
                controller.owner_entity,
                controller.geometry,
            )
        })
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
        let value = read_json(root, payloads, source)?;
        let description = value
            .get("minecraft:client_entity")
            .and_then(|value| value.get("description"))
            .and_then(Value::as_object)
            .ok_or_else(|| invalid("client entity description is absent"))?;
        let Some(geometry_name) = default_geometry(description.get("geometry")) else {
            outcomes.push(CompileReferenceOutcome::RequiredRigRejected {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: RejectReason::MissingGeometryReference,
            });
            continue;
        };
        let Some(geometry) = geometry_indices.get(geometry_name) else {
            outcomes.push(CompileReferenceOutcome::RequiredRigRejected {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: RejectReason::MissingGeometryReference,
            });
            continue;
        };
        let Some(geometry) = *geometry else {
            outcomes.push(CompileReferenceOutcome::RequiredRigRejected {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: RejectReason::AmbiguousGeometryReference,
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
        let Some(render_controller) = render_indices.get(render_name) else {
            outcomes.push(CompileReferenceOutcome::RequiredRigRejected {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: RejectReason::MissingRenderControllerReference,
            });
            continue;
        };
        let Some(render_controller) = *render_controller else {
            outcomes.push(CompileReferenceOutcome::RequiredRigRejected {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: RejectReason::AmbiguousRenderControllerReference,
            });
            continue;
        };
        let mut rejected = false;
        let mut static_fallback = false;
        let mut geometry_candidates = vec![(geometry, None)];
        match geometry_selections.get(&(render_name.into(), entity_symbol as u32)) {
            Some(GeometrySelection::Supported(selectable)) => geometry_candidates.extend(
                selectable
                    .iter()
                    .map(|candidate| (candidate.geometry, Some(candidate.condition))),
            ),
            Some(GeometrySelection::Unsupported) => static_fallback = true,
            None => {}
        }
        let animation_aliases = parse_aliases(description.get("animations"))?;
        let controller_aliases = parse_aliases(description.get("animation_controllers"))?;
        let mut pending_geometries = Vec::new();
        for (candidate_geometry, condition) in geometry_candidates {
            let mut animation_bindings: Vec<(Box<str>, u32)> = Vec::new();
            let mut controller_bindings: Vec<(Box<str>, Box<str>)> = Vec::new();
            for (name, target) in &animation_aliases {
                names.push(name.clone());
                if target.starts_with("controller.animation.") {
                    if controller_symbols
                        .get(target.as_ref())
                        .is_some_and(Option::is_none)
                    {
                        rejected = true;
                    } else if controller_names.contains(&(
                        target.as_ref(),
                        entity_symbol as u32,
                        candidate_geometry,
                    )) {
                        controller_bindings.push((name.clone(), target.clone()));
                    } else {
                        static_fallback = true;
                    }
                } else if animation_symbols
                    .get(target.as_ref())
                    .is_some_and(Option::is_none)
                {
                    rejected = true;
                } else if let Some(&clip) = clip_indices.get(&(target.clone(), candidate_geometry))
                {
                    animation_bindings.push((name.clone(), clip));
                } else if animation_symbols.contains_key(target.as_ref()) {
                    static_fallback = true;
                } else {
                    rejected = true;
                }
            }
            for (name, target) in &controller_aliases {
                names.push(name.clone());
                if controller_symbols
                    .get(target.as_ref())
                    .is_some_and(Option::is_none)
                {
                    rejected = true;
                } else if controller_names.contains(&(
                    target.as_ref(),
                    entity_symbol as u32,
                    candidate_geometry,
                )) {
                    controller_bindings.push((name.clone(), target.clone()));
                } else if controller_symbols.contains_key(target.as_ref()) {
                    static_fallback = true;
                } else {
                    rejected = true;
                }
            }
            animation_bindings.sort_by(|left, right| left.0.cmp(&right.0));
            controller_bindings.sort_by(|left, right| left.0.cmp(&right.0));
            controller_bindings.dedup();
            pending_geometries.push(PendingRigGeometry {
                geometry: candidate_geometry,
                condition,
                animations: animation_bindings,
                controllers: controller_bindings,
            });
        }
        if rejected {
            outcomes.push(CompileReferenceOutcome::RequiredRigRejected {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: if description
                    .get("animations")
                    .and_then(Value::as_object)
                    .is_some_and(|animations| {
                        animations.values().filter_map(Value::as_str).any(|target| {
                            animation_symbols.get(target).is_some_and(Option::is_none)
                                || controller_symbols.get(target).is_some_and(Option::is_none)
                        })
                    }) {
                    RejectReason::AmbiguousAnimationReference
                } else {
                    RejectReason::MissingAnimationReference
                },
            });
            continue;
        }
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
            render_controller,
            geometries: pending_geometries,
            fallback,
        });
        outcomes.push(CompileReferenceOutcome::Resolved(rig_index));
    }
    Ok((PendingRigPayload { rigs, outcomes }, names))
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
    clips: &ClipIndices,
    aliases: &BTreeMap<Box<str>, Box<str>>,
    geometry: u32,
) -> Result<u32, AssetError> {
    let identifier = aliases
        .get(identifier)
        .map_or(identifier, |identifier| identifier.as_ref());
    clips
        .get(&(identifier.into(), geometry))
        .copied()
        .ok_or_else(|| invalid("controller references a missing animation"))
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
