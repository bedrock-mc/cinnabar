use std::{collections::BTreeMap, path::Path};

use assets::{
    AssetError, EntityAnimationChannel, EntityAnimationClip, EntityAnimationController,
    EntityAnimationKeyframe, EntityAssetKind, EntityAssetSource, EntityAssetSymbol,
    EntityControllerAnimation, EntityControllerState, EntityControllerTransition, EntityGeometry,
    EntityRigAnimationBinding, EntityRigBinding, EntityRigControllerBinding, EntityRigFallback,
};
use serde_json::{Map, Value};

use super::{SourcePayloads, invalid, molang::MolangCompiler};

mod clip;
mod outcome;

use clip::{
    ClipCompileError, compile_clip_for_geometry, has_string_leaf, looks_like_expression, read_json,
    required_object, source_index,
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
    pub rig_animations: Box<[EntityRigAnimationBinding]>,
    pub rig_controllers: Box<[EntityRigControllerBinding]>,
    pub outcomes: Box<[CompileReferenceOutcome<u32>]>,
}

#[derive(Default)]
struct PendingController {
    owner_entity: u32,
    symbol: u32,
    initial_state: Box<str>,
    states: Vec<PendingState>,
}

struct EntityEnvironment {
    entity_symbol: u32,
    geometry: Option<u32>,
    geometry_aliases: BTreeMap<Box<str>, Box<str>>,
    render_controllers: Vec<Box<str>>,
    aliases: BTreeMap<Box<str>, Box<str>>,
}

type ClipIndices = BTreeMap<(Box<str>, u32), u32>;
type GeometrySelections = BTreeMap<(Box<str>, u32), Option<u32>>;

struct RigInputs<'a> {
    clip_indices: &'a ClipIndices,
    controllers: &'a [PendingController],
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
            .filter_map(|environment| {
                environment.geometry.filter(|_| {
                    environment.aliases.values().any(|target| {
                        !target.starts_with("controller.animation.")
                            && target.as_ref() == identifier
                    })
                })
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
                &geometries[geometry as usize],
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
    let geometry_selections =
        compile_render_collections(root, payloads, sources, geometries, &environments, molang)?;
    let pending_controllers = compile_pending_controllers(
        root,
        payloads,
        sources,
        &symbol_indices,
        &clip_indices,
        &environments,
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
        if compiled_controller_symbols.contains(&(symbol as u32)) {
            continue;
        }
        let referenced = environments.iter().any(|environment| {
            environment
                .aliases
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
        rig_animations: finalized_rigs.animations,
        rig_controllers: finalized_rigs.controllers,
        outcomes: {
            outcomes.extend(finalized_rigs.outcomes);
            outcomes.into_boxed_slice()
        },
    })
}

fn collect_entity_environments(
    root: &Path,
    payloads: &SourcePayloads,
    sources: &[EntityAssetSource],
    symbols: &[EntityAssetSymbol],
    geometries: &[EntityGeometry],
) -> Result<Vec<EntityEnvironment>, AssetError> {
    let geometry_indices = unique_geometry_indices(geometries);
    let mut environments = Vec::new();
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
        let geometry_aliases = description
            .get("geometry")
            .and_then(Value::as_object)
            .map(|aliases| {
                aliases
                    .iter()
                    .filter_map(|(alias, target)| {
                        target
                            .as_str()
                            .map(|target| (alias.as_str().into(), target.into()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let geometry = first_string(description.get("geometry"))
            .and_then(|identifier| geometry_indices.get(identifier).copied().flatten());
        let render_controllers = description
            .get("render_controllers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| match entry {
                Value::String(identifier) => Some(identifier.as_str().into()),
                Value::Object(condition) if condition.len() == 1 => condition
                    .keys()
                    .next()
                    .map(|identifier| identifier.as_str().into()),
                _ => None,
            })
            .collect();
        let aliases = description
            .get("animations")
            .and_then(Value::as_object)
            .map(|aliases| {
                aliases
                    .iter()
                    .filter_map(|(alias, target)| {
                        target
                            .as_str()
                            .map(|target| (alias.as_str().into(), target.into()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        environments.push(EntityEnvironment {
            entity_symbol: entity_symbol as u32,
            geometry,
            geometry_aliases,
            render_controllers,
            aliases,
        });
    }
    Ok(environments)
}

fn unique_geometry_indices(geometries: &[EntityGeometry]) -> BTreeMap<Box<str>, Option<u32>> {
    let mut indices = BTreeMap::new();
    for (index, geometry) in geometries.iter().enumerate() {
        indices
            .entry(geometry.identifier.clone())
            .and_modify(|value| *value = None)
            .or_insert(Some(index as u32));
    }
    indices
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
    symbol_indices: &BTreeMap<(EntityAssetKind, &str, u32), u32>,
    clip_indices: &ClipIndices,
    environments: &[EntityEnvironment],
    molang: &mut MolangCompiler,
) -> Result<Vec<PendingController>, AssetError> {
    let mut output = Vec::new();
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
                    .aliases
                    .values()
                    .any(|target| target.as_ref() == identifier)
            }) {
                let Some(geometry) = environment.geometry else {
                    continue;
                };
                let mut transaction = molang.clone();
                if let Ok(mut controller) = compile_one_controller(
                    symbol,
                    environment.entity_symbol,
                    definition,
                    clip_indices,
                    &environment.aliases,
                    geometry,
                    &mut transaction,
                ) {
                    controller.owner_entity = environment.entity_symbol;
                    *molang = transaction;
                    output.push(controller);
                }
            }
        }
    }
    output.sort_by_key(|controller| (controller.symbol, controller.owner_entity));
    Ok(output)
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

fn compile_render_collections(
    root: &Path,
    payloads: &SourcePayloads,
    sources: &[EntityAssetSource],
    geometries: &[EntityGeometry],
    environments: &[EntityEnvironment],
    molang: &mut MolangCompiler,
) -> Result<GeometrySelections, AssetError> {
    let geometry_indices = unique_geometry_indices(geometries);
    let mut selections = BTreeMap::new();
    for source in sources
        .iter()
        .filter(|source| source.path.starts_with("render_controllers/"))
    {
        let value = read_json(root, payloads, source)?;
        for (controller_name, definition) in required_object(&value, "render_controllers")? {
            let definition = definition
                .as_object()
                .ok_or_else(|| invalid("render controller must be an object"))?;
            for environment in environments.iter().filter(|environment| {
                environment.geometry.is_some()
                    && environment
                        .render_controllers
                        .iter()
                        .any(|identifier| identifier.as_ref() == controller_name)
            }) {
                let mut transaction = molang.clone();
                let mut local_collections = BTreeMap::<Box<str>, Box<str>>::new();
                if let Some(arrays) = definition.get("arrays") {
                    let arrays = arrays
                        .as_object()
                        .ok_or_else(|| invalid("render-controller arrays must be an object"))?;
                    if let Some(geometries_array) = arrays.get("geometries") {
                        let geometries_array = geometries_array.as_object().ok_or_else(|| {
                            invalid("render-controller arrays.geometries is invalid")
                        })?;
                        for (name, members) in geometries_array {
                            let values = members
                                .as_array()
                                .ok_or_else(|| {
                                    invalid("render-controller collection must be an array")
                                })?
                                .iter()
                                .map(|member| {
                                    let member = member.as_str().ok_or_else(|| {
                                        invalid("collection member must be a name")
                                    })?;
                                    let identifier = member
                                        .strip_prefix("Geometry.")
                                        .and_then(|alias| environment.geometry_aliases.get(alias))
                                        .map_or(member, |identifier| identifier.as_ref());
                                    geometry_indices
                                        .get(identifier)
                                        .copied()
                                        .flatten()
                                        .map(|index| index as f32)
                                        .ok_or_else(|| {
                                            invalid(format!(
                                                "unknown or ambiguous geometry collection member `{member}`"
                                            ))
                                        })
                                })
                                .collect::<Result<Vec<_>, AssetError>>()?;
                            let scoped_name = format!(
                                "{}#{controller_name}#{}#{name}",
                                source.path, environment.entity_symbol
                            );
                            transaction.add_collection(&scoped_name, values)?;
                            local_collections.insert(name.as_str().into(), scoped_name.into());
                        }
                    }
                }
                if let Some(geometry) = definition.get("geometry").and_then(Value::as_str)
                    && let Some((collection, index)) = split_collection_selection(geometry)
                    && let Some(scoped) = local_collections.get(collection)
                {
                    let key = (controller_name.as_str().into(), environment.entity_symbol);
                    match transaction.compile_collection_selection(scoped, index) {
                        Ok(expression) => {
                            *molang = transaction;
                            selections.insert(key, Some(expression));
                        }
                        Err(_) => {
                            selections.insert(key, None);
                        }
                    }
                }
            }
        }
    }
    Ok(selections)
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
    geometry_selection: Option<u32>,
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
        controller_indices: &BTreeMap<(Box<str>, u32), u32>,
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
                        .get(&(controller, rig.entity_symbol))
                        .ok_or_else(|| invalid("rig controller is absent"))?,
                });
            }
            bindings.push(EntityRigBinding {
                entity_symbol: rig.entity_symbol,
                geometry: rig.geometry,
                render_controller: rig.render_controller,
                geometry_selection: rig.geometry_selection,
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
        let Some(geometry_name) = first_string(description.get("geometry")) else {
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
        let mut animation_bindings: Vec<(Box<str>, u32)> = Vec::new();
        let mut controller_bindings: Vec<(Box<str>, Box<str>)> = Vec::new();
        let mut rejected = false;
        let mut static_fallback = false;
        let geometry_selection = geometry_selections
            .get(&(render_name.into(), entity_symbol as u32))
            .copied()
            .flatten();
        static_fallback |= geometry_selections
            .get(&(render_name.into(), entity_symbol as u32))
            .is_some_and(Option::is_none);
        if let Some(animations) = description.get("animations").and_then(Value::as_object) {
            for (name, target) in animations {
                let target = target
                    .as_str()
                    .ok_or_else(|| invalid("entity animation binding must be a string"))?;
                names.push(name.as_str().into());
                if target.starts_with("controller.animation.") {
                    if controller_symbols.get(target).is_some_and(Option::is_none) {
                        rejected = true;
                    } else if controller_names.contains(&(target, entity_symbol as u32)) {
                        controller_bindings.push((name.as_str().into(), target.into()));
                    } else {
                        static_fallback = true;
                    }
                } else if animation_symbols.get(target).is_some_and(Option::is_none) {
                    rejected = true;
                } else if let Some(&clip) = clip_indices.get(&(target.into(), geometry)) {
                    animation_bindings.push((name.as_str().into(), clip));
                } else if animation_symbols.contains_key(target) {
                    static_fallback = true;
                } else {
                    rejected = true;
                }
            }
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
            geometry_selection,
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
