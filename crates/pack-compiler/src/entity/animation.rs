use std::{collections::BTreeMap, path::Path};

use assets::{
    AssetError, EntityAnimationChannel, EntityAnimationClip, EntityAnimationController,
    EntityAnimationKeyframe, EntityAssetKind, EntityAssetSource, EntityAssetSymbol,
    EntityControllerAnimation, EntityControllerAnimationTarget, EntityControllerState,
    EntityControllerTransition, EntityGeometry, EntityRenderData, EntityRigAnimationBinding,
    EntityRigBinding, EntityRigControllerBinding, EntityRigGeometryBinding,
};
use serde_json::Value;

use super::{SourcePayloads, invalid, molang::MolangCompiler};

mod clip;
mod controller;
mod environment;
mod outcome;
mod render;
mod rig;
pub(crate) mod roots;
mod selection;

use clip::{
    ClipCompileError, ClipOutputs, compile_clip_for_geometry, looks_like_expression, read_json,
    required_object, source_index,
};
use controller::{
    ControllerInputs, ControllerKey, PendingController, PendingControllers, PendingTarget,
    compile_pending_controllers, nesting_depth,
};
use environment::{
    GeometrySelection, collect as collect_entity_environments, compile_geometry_selections,
    controller_clip_references, effective_bone_names, selection_for,
};
pub use outcome::{CompileReferenceOutcome, FallbackReason, RejectReason};
use render::{RenderSources, compile_render};
use rig::{RigInputs, RigSources, compile_rigs};

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
    pub render: EntityRenderData,
    pub outcomes: Box<[CompileReferenceOutcome<u32>]>,
}

type ClipIndices = BTreeMap<(Box<str>, u32), u32>;

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
    let mut clip_indices = ClipIndices::new();
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
                    // Every render controller poses its own model with the actor's clips.
                    geometries.extend(environment.alias_geometries.iter().copied());
                }
                geometries
            })
            .collect::<std::collections::BTreeSet<_>>();
        if used_geometries.is_empty() {
            outcomes.push(fallback(
                source,
                symbol,
                FallbackReason::UnreferencedDefinition,
            ));
            continue;
        }
        let definition = definition
            .as_object()
            .ok_or_else(|| invalid("animation definition must be an object"))?;
        if definition
            .get("animation_length")
            .and_then(Value::as_str)
            .is_some_and(looks_like_expression)
        {
            outcomes.push(fallback(
                source,
                symbol,
                FallbackReason::UnsupportedOptionalExpression,
            ));
            continue;
        }
        let mut partial = false;
        for geometry in used_geometries {
            match compile_clip_for_geometry(
                symbol,
                source,
                definition,
                (geometry, &effective_bones[geometry as usize]),
                ClipOutputs {
                    clips: &mut clips,
                    channels: &mut channels,
                    keyframes: &mut keyframes,
                    molang,
                },
            ) {
                Ok((clip, dropped)) => {
                    partial |= dropped > 0;
                    clip_indices.insert((identifier.into(), geometry), clip);
                }
                Err(ClipCompileError::Invalid(error)) => return Err(error),
            }
        }
        if partial {
            outcomes.push(fallback(
                source,
                symbol,
                FallbackReason::UnsupportedOptionalExpression,
            ));
        }
    }
    let PendingControllers {
        controllers: pending_controllers,
        failed: controller_fallbacks,
        partial: partial_controllers,
    } = compile_pending_controllers(
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
            outcomes.push(fallback(
                definition.source_index,
                symbol as u32,
                FallbackReason::UnsupportedOptionalExpression,
            ));
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
        outcomes.push(fallback(
            definition.source_index,
            symbol as u32,
            if referenced {
                FallbackReason::UnsupportedOptionalExpression
            } else {
                FallbackReason::UnreferencedDefinition
            },
        ));
    }

    // Name indices are stable because Name sorts before Query/Variable/Temporary.
    let mut names = pending_controllers
        .iter()
        .flat_map(|controller| controller.states.iter().map(|state| state.name.clone()))
        .collect::<Vec<_>>();
    let (rig_payload, rig_names) = compile_rigs(
        RigSources {
            root,
            payloads,
            sources,
            symbols,
            geometries,
        },
        RigInputs {
            clip_indices: &clip_indices,
            controllers: &pending_controllers,
            geometry_selections: &geometry_selections,
        },
        molang,
    )?;
    names.extend(rig_names);
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

    let finalized = finalize_controllers(pending_controllers, symbols, &name_index)?;
    for symbol in partial_controllers.union(&finalized.partial) {
        outcomes.push(fallback(
            symbols[*symbol as usize].source_index,
            *symbol,
            FallbackReason::UnsupportedOptionalExpression,
        ));
    }
    let finalized_rigs = rig_payload.finalize(&name_index, &finalized.indices)?;
    outcomes.extend(finalized_rigs.outcomes);
    let render = compile_render(
        RenderSources {
            root,
            payloads,
            sources,
            symbols,
            geometries,
            rigs: &finalized_rigs.bindings,
            rig_geometries: &finalized_rigs.geometries,
        },
        molang,
    )?;

    Ok(AnimationPayload {
        clips: clips.into_boxed_slice(),
        channels: channels.into_boxed_slice(),
        keyframes: keyframes.into_boxed_slice(),
        controllers: finalized.controllers.into_boxed_slice(),
        controller_states: finalized.states.into_boxed_slice(),
        controller_animations: finalized.animations.into_boxed_slice(),
        controller_transitions: finalized.transitions.into_boxed_slice(),
        rig_bindings: finalized_rigs.bindings,
        rig_geometries: finalized_rigs.geometries,
        rig_animations: finalized_rigs.animations,
        rig_controllers: finalized_rigs.controllers,
        render,
        outcomes: outcomes.into_boxed_slice(),
    })
}

fn fallback(source: u32, symbol: u32, reason: FallbackReason) -> CompileReferenceOutcome<u32> {
    CompileReferenceOutcome::OptionalStaticFallback {
        source,
        symbol,
        reason,
    }
}

struct FinalControllers {
    controllers: Vec<EntityAnimationController>,
    states: Vec<EntityControllerState>,
    animations: Vec<EntityControllerAnimation>,
    transitions: Vec<EntityControllerTransition>,
    indices: BTreeMap<ControllerKey, u32>,
    /// Controllers that lost a nested reference the rig cannot evaluate.
    partial: std::collections::BTreeSet<u32>,
}

fn finalize_controllers(
    pending: Vec<PendingController>,
    symbols: &[EntityAssetSymbol],
    name_index: &impl Fn(&str) -> Result<u32, AssetError>,
) -> Result<FinalControllers, AssetError> {
    let indices = pending
        .iter()
        .enumerate()
        .map(|(index, controller)| {
            (
                (
                    symbols[controller.symbol as usize].identifier.clone(),
                    controller.owner_entity,
                    controller.geometry,
                ),
                index as u32,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let by_key = indices
        .iter()
        .map(|(key, index)| (key.clone(), &pending[*index as usize]))
        .collect::<BTreeMap<_, _>>();
    let mut output = FinalControllers {
        controllers: Vec::new(),
        states: Vec::new(),
        animations: Vec::new(),
        transitions: Vec::new(),
        indices: BTreeMap::new(),
        partial: std::collections::BTreeSet::new(),
    };
    for controller in &pending {
        let first_state = output.states.len() as u32;
        let state_indices = controller
            .states
            .iter()
            .enumerate()
            .map(|(index, state)| (state.name.as_ref(), index as u16))
            .collect::<BTreeMap<_, _>>();
        for state in &controller.states {
            let first_animation = output.animations.len() as u32;
            for (target, weight) in &state.animations {
                let target = match target {
                    PendingTarget::Clip(clip) => Some(EntityControllerAnimationTarget::Clip(*clip)),
                    PendingTarget::Controller(nested) => {
                        let key = (nested.clone(), controller.owner_entity, controller.geometry);
                        let depth = nesting_depth(&key, &by_key, &mut Vec::new());
                        depth
                            .filter(|depth| depth + 2 <= assets::MAX_ENTITY_CONTROLLER_NESTING)
                            .and_then(|_| indices.get(&key))
                            .map(|index| EntityControllerAnimationTarget::Controller(*index))
                    }
                };
                match target {
                    Some(target) => output.animations.push(EntityControllerAnimation {
                        target,
                        weight: *weight,
                    }),
                    None => {
                        output.partial.insert(controller.symbol);
                    }
                }
            }
            let first_transition = output.transitions.len() as u32;
            for (target, condition) in &state.transitions {
                output.transitions.push(EntityControllerTransition {
                    target_state: *state_indices
                        .get(target.as_ref())
                        .ok_or_else(|| invalid("controller transition target is absent"))?,
                    condition: *condition,
                });
            }
            output.states.push(EntityControllerState {
                name: name_index(&state.name)?,
                first_animation,
                animation_count: (output.animations.len() as u32 - first_animation) as u16,
                first_transition,
                transition_count: (output.transitions.len() as u32 - first_transition) as u16,
                on_entry: state.on_entry,
                on_exit: state.on_exit,
                blend_transition: state.blend_transition,
                blend_via_shortest_path: state.blend_via_shortest_path,
            });
        }
        let initial_state = *state_indices
            .get(controller.initial_state.as_ref())
            .ok_or_else(|| invalid("controller initial state is absent"))?;
        output.controllers.push(EntityAnimationController {
            symbol: controller.symbol,
            first_state,
            state_count: controller.states.len() as u16,
            initial_state,
        });
    }
    output.indices = indices;
    Ok(output)
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
