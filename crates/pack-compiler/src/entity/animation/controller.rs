use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use assets::{
    AssetError, EntityAssetKind, EntityAssetSource, EntityGeometryScalar,
    MAX_ENTITY_CONTROLLER_NESTING,
};
use serde_json::{Map, Value};

use super::{
    super::{SourcePayloads, invalid, molang::MolangCompiler},
    ClipIndices,
    clip::{read_json, required_object},
    environment::{EntityEnvironment, GeometrySelection, GeometrySelections, selection_for},
};

/// Identifier, owning entity, and geometry: one controller is compiled per rig geometry.
pub(super) type ControllerKey = (Box<str>, u32, u32);

#[derive(Default)]
pub(super) struct PendingController {
    pub owner_entity: u32,
    pub geometry: u32,
    pub symbol: u32,
    pub initial_state: Box<str>,
    pub states: Vec<PendingState>,
}

pub(super) enum PendingTarget {
    Clip(u32),
    Controller(Box<str>),
}

#[derive(Default)]
pub(super) struct PendingState {
    pub blend_transition: EntityGeometryScalar,
    pub blend_via_shortest_path: bool,
    pub name: Box<str>,
    pub animations: Vec<(PendingTarget, Option<u32>)>,
    pub transitions: Vec<(Box<str>, u32)>,
    pub on_entry: Option<u32>,
    pub on_exit: Option<u32>,
}

pub(super) struct ControllerInputs<'a> {
    pub symbol_indices: &'a BTreeMap<(EntityAssetKind, &'a str, u32), u32>,
    pub clip_indices: &'a ClipIndices,
    pub environments: &'a [EntityEnvironment],
    pub geometry_selections: &'a GeometrySelections,
}

pub(super) struct PendingControllers {
    pub controllers: Vec<PendingController>,
    /// Controllers rejected outright, attributed as static fallbacks.
    pub failed: BTreeSet<u32>,
    /// Controllers that compiled with at least one unsupported fragment dropped.
    pub partial: BTreeSet<u32>,
}

pub(super) fn compile_pending_controllers(
    root: &Path,
    payloads: &SourcePayloads,
    sources: &[EntityAssetSource],
    inputs: ControllerInputs<'_>,
    molang: &mut MolangCompiler,
) -> Result<PendingControllers, AssetError> {
    let ControllerInputs {
        symbol_indices,
        clip_indices,
        environments,
        geometry_selections,
    } = inputs;
    let mut output = Vec::new();
    let mut failed = BTreeSet::new();
    let mut partial = BTreeSet::new();
    for source in sources
        .iter()
        .filter(|source| source.path.starts_with("animation_controllers/"))
    {
        let source_index = super::SourceIndex::source_path_index(source, sources)?;
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
                let mut geometries = BTreeSet::from([default_geometry]);
                if let Some(GeometrySelection::Supported(selectable)) =
                    selection_for(environment, geometry_selections)
                {
                    geometries.extend(selectable.iter().map(|candidate| candidate.geometry));
                }
                for geometry in geometries {
                    let mark = molang.mark();
                    let context = ControllerContext {
                        clips: clip_indices,
                        aliases: &environment.animation_aliases,
                        geometry,
                    };
                    match compile_one_controller(definition, &context, molang) {
                        Ok((mut controller, dropped)) => {
                            controller.symbol = symbol;
                            controller.owner_entity = environment.entity_symbol;
                            controller.geometry = geometry;
                            if dropped > 0 {
                                partial.insert(symbol);
                            }
                            output.push(controller);
                        }
                        Err(_) => {
                            molang.rollback(mark);
                            failed.insert(symbol);
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
    Ok(PendingControllers {
        controllers: output,
        failed,
        partial,
    })
}

struct ControllerContext<'a> {
    clips: &'a ClipIndices,
    aliases: &'a BTreeMap<Box<str>, Box<str>>,
    geometry: u32,
}

/// Compiles one controller, dropping each animation, weight, transition, or script statement
/// outside the reviewed surface; returns the controller and the dropped fragment count.
fn compile_one_controller(
    definition: &Map<String, Value>,
    context: &ControllerContext<'_>,
    molang: &mut MolangCompiler,
) -> Result<(PendingController, usize), AssetError> {
    let states = required_object(
        definition
            .get("states")
            .ok_or_else(|| invalid("controller states are absent"))?,
        "",
    )?;
    if states.is_empty() {
        return Err(invalid("controller has no states"));
    }
    let mut dropped = 0;
    let mut pending_states = Vec::new();
    for (name, state) in states {
        let state = state
            .as_object()
            .ok_or_else(|| invalid("controller state must be an object"))?;
        let blend_transition =
            state
                .get("blend_transition")
                .map_or(Ok(EntityGeometryScalar::ZERO), |value| {
                    value
                        .as_f64()
                        .filter(|seconds| *seconds >= 0.0)
                        .and_then(|seconds| EntityGeometryScalar::new(seconds as f32))
                        .ok_or_else(|| {
                            invalid("controller blend duration must be finite and nonnegative")
                        })
                })?;
        let blend_via_shortest_path =
            state
                .get("blend_via_shortest_path")
                .map_or(Ok(false), |value| {
                    value.as_bool().ok_or_else(|| {
                        invalid("controller shortest-path blend flag must be boolean")
                    })
                })?;
        let mut animations = Vec::new();
        if let Some(entries) = state.get("animations") {
            for entry in entries
                .as_array()
                .ok_or_else(|| invalid("state animations must be an array"))?
            {
                // A weighted object may bind several animations (camel's `moving` and
                // `baby_moving`); every binding compiles.
                let bindings: Vec<(&String, Option<String>)> = match entry {
                    Value::String(identifier) => vec![(identifier, None)],
                    Value::Object(weighted) => weighted
                        .iter()
                        .map(|(identifier, weight)| {
                            let weight = match weight {
                                Value::String(text) => text.clone(),
                                Value::Number(number) => number.to_string(),
                                _ => return Err(invalid("animation weight must be Molang")),
                            };
                            Ok((identifier, Some(weight)))
                        })
                        .collect::<Result<_, _>>()?,
                    _ => return Err(invalid("invalid controller animation binding")),
                };
                for (identifier, weight) in bindings {
                    let target = resolve_target(identifier, context);
                    let weight = weight.map(|weight| molang.compile(&weight)).transpose();
                    match (target, weight) {
                        (Some(target), Ok(weight)) => animations.push((target, weight)),
                        _ => dropped += 1,
                    }
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
                if !states.contains_key(target) {
                    return Err(invalid("controller transition target is absent"));
                }
                match molang.compile(condition) {
                    Ok(condition) => transitions.push((target.as_str().into(), condition)),
                    Err(_) => dropped += 1,
                }
            }
        }
        let mut script = |field: &str| -> Result<Option<u32>, AssetError> {
            let (script, skipped) = molang.compile_script_value(state.get(field))?;
            dropped += skipped;
            Ok(script)
        };
        let on_entry = script("on_entry")?;
        let on_exit = script("on_exit")?;
        pending_states.push(PendingState {
            name: name.as_str().into(),
            animations,
            transitions,
            on_entry,
            on_exit,
            blend_transition,
            blend_via_shortest_path,
        });
    }
    pending_states.sort_by(|left, right| left.name.cmp(&right.name));
    let initial_state = definition
        .get("initial_state")
        .and_then(Value::as_str)
        .unwrap_or_else(|| pending_states[0].name.as_ref());
    Ok((
        PendingController {
            initial_state: initial_state.into(),
            states: pending_states,
            ..PendingController::default()
        },
        dropped,
    ))
}

fn resolve_target(identifier: &str, context: &ControllerContext<'_>) -> Option<PendingTarget> {
    let identifier = context
        .aliases
        .get(identifier)
        .map_or(identifier, AsRef::as_ref);
    if identifier.starts_with("controller.animation.") {
        return Some(PendingTarget::Controller(identifier.into()));
    }
    context
        .clips
        .get(&(identifier.into(), context.geometry))
        .map(|clip| PendingTarget::Clip(*clip))
}

/// Levels of controllers below `key`, or `None` when it cycles or exceeds the runtime bound.
pub(super) fn nesting_depth(
    key: &ControllerKey,
    controllers: &BTreeMap<ControllerKey, &PendingController>,
    visiting: &mut Vec<ControllerKey>,
) -> Option<usize> {
    if visiting.contains(key) || visiting.len() >= MAX_ENTITY_CONTROLLER_NESTING {
        return None;
    }
    let controller = controllers.get(key)?;
    visiting.push(key.clone());
    let mut depth = 0;
    for state in &controller.states {
        for (target, _) in &state.animations {
            let PendingTarget::Controller(nested) = target else {
                continue;
            };
            let nested = (nested.clone(), key.1, key.2);
            if controllers.contains_key(&nested) {
                depth = depth.max(1 + nesting_depth(&nested, controllers, visiting)?);
            }
        }
    }
    visiting.pop();
    Some(depth)
}
