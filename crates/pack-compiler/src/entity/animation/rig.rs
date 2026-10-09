use std::{collections::BTreeMap, path::Path};

use assets::{
    AssetError, EntityAssetKind, EntityAssetSource, EntityAssetSymbol, EntityGeometry,
    EntityGeometryScalar, EntityRigAnimationBinding, EntityRigBinding, EntityRigControllerBinding,
    EntityRigFallback, EntityRigGeometryBinding,
};
use serde_json::{Map, Value};

use super::{
    super::{SourcePayloads, invalid, molang::MolangCompiler},
    ClipIndices, CompileReferenceOutcome, FallbackReason, RejectReason,
    clip::read_json,
    controller::{ControllerKey, PendingController},
    environment::{
        GeometrySelection, GeometrySelections, default_geometry, unique_geometry_indices,
    },
    roots,
};

pub(super) struct RigInputs<'a> {
    pub clip_indices: &'a ClipIndices,
    pub controllers: &'a [PendingController],
    pub geometry_selections: &'a GeometrySelections,
}

pub(super) struct PendingRigPayload {
    rigs: Vec<PendingRig>,
    outcomes: Vec<CompileReferenceOutcome<u32>>,
}

struct PendingRig {
    entity_symbol: u32,
    render_controller: u32,
    geometries: Vec<PendingRigGeometry>,
    fallback: EntityRigFallback,
    initialize: Option<u32>,
    pre_animation: Option<u32>,
    scale: EntityGeometryScalar,
    scale_expressions: Option<[u32; 4]>,
}

/// `(alias, target, weight, authored order)` of one root binding.
type PendingBinding<T> = (Box<str>, T, Option<u32>, u16);

struct PendingRigGeometry {
    geometry: u32,
    condition: Option<u32>,
    animations: Vec<PendingBinding<u32>>,
    controllers: Vec<PendingBinding<Box<str>>>,
}

pub(super) struct FinalRigPayload {
    pub bindings: Box<[EntityRigBinding]>,
    pub geometries: Box<[EntityRigGeometryBinding]>,
    pub animations: Box<[EntityRigAnimationBinding]>,
    pub controllers: Box<[EntityRigControllerBinding]>,
    pub outcomes: Box<[CompileReferenceOutcome<u32>]>,
}

impl PendingRigPayload {
    pub(super) fn finalize(
        self,
        name_index: &impl Fn(&str) -> Result<u32, AssetError>,
        controller_indices: &BTreeMap<ControllerKey, u32>,
    ) -> Result<FinalRigPayload, AssetError> {
        let mut bindings = Vec::new();
        let mut geometries = Vec::new();
        let mut animations = Vec::new();
        let mut controllers = Vec::new();
        for rig in self.rigs {
            let first_geometry = geometries.len() as u32;
            for candidate in rig.geometries {
                let first_animation = animations.len() as u32;
                for (name, clip, weight, order) in candidate.animations {
                    animations.push(EntityRigAnimationBinding {
                        name: name_index(&name)?,
                        clip,
                        weight,
                        order,
                    });
                }
                let first_controller = controllers.len() as u32;
                for (name, controller, weight, order) in candidate.controllers {
                    controllers.push(EntityRigControllerBinding {
                        name: name_index(&name)?,
                        controller: *controller_indices
                            .get(&(controller, rig.entity_symbol, candidate.geometry))
                            .ok_or_else(|| invalid("rig controller is absent"))?,
                        weight,
                        order,
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
                initialize: rig.initialize,
                pre_animation: rig.pre_animation,
                scale: rig.scale,
                scale_expressions: rig.scale_expressions,
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

pub(super) struct RigSources<'a> {
    pub root: &'a Path,
    pub payloads: &'a SourcePayloads,
    pub sources: &'a [EntityAssetSource],
    pub symbols: &'a [EntityAssetSymbol],
    pub geometries: &'a [EntityGeometry],
}

pub(super) fn compile_rigs(
    input: RigSources<'_>,
    inputs: RigInputs<'_>,
    molang: &mut MolangCompiler,
) -> Result<(PendingRigPayload, Vec<Box<str>>), AssetError> {
    let RigSources {
        root,
        payloads,
        sources,
        symbols,
        geometries,
    } = input;
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
    for (entity_symbol, entity) in symbols.iter().enumerate().filter(|(_, symbol)| {
        matches!(
            symbol.kind,
            EntityAssetKind::Entity | EntityAssetKind::Attachable
        )
    }) {
        let source = &sources[entity.source_index as usize];
        let value = read_json(root, payloads, source)?;
        let description = roots::description(&value)
            .ok_or_else(|| invalid("client entity description is absent"))?;
        let reject = |reason| CompileReferenceOutcome::RequiredRigRejected {
            source: entity.source_index,
            symbol: entity_symbol as u32,
            reason,
        };
        let Some(geometry_name) = default_geometry(description.get("geometry")) else {
            outcomes.push(reject(RejectReason::MissingGeometryReference));
            continue;
        };
        let geometry = match geometry_indices.get(geometry_name) {
            None => {
                outcomes.push(reject(RejectReason::MissingGeometryReference));
                continue;
            }
            Some(None) => {
                outcomes.push(reject(RejectReason::AmbiguousGeometryReference));
                continue;
            }
            Some(Some(geometry)) => *geometry,
        };
        let Some((render_name, optional_condition)) =
            select_render_controller(description.get("render_controllers"))
        else {
            outcomes.push(reject(RejectReason::MissingRequiredReference));
            continue;
        };
        let render_controller = match render_indices.get(render_name) {
            None => {
                outcomes.push(reject(RejectReason::MissingRenderControllerReference));
                continue;
            }
            Some(None) => {
                outcomes.push(reject(RejectReason::AmbiguousRenderControllerReference));
                continue;
            }
            Some(Some(render_controller)) => *render_controller,
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
        let animation_aliases = roots::animation_aliases(&value)?;
        let controller_aliases = roots::legacy_controller_aliases(&value)?;
        let roots = roots::activation_roots(&value);
        let root_condition = |name: &str| {
            roots.as_ref().and_then(|roots| {
                roots
                    .iter()
                    .find(|root| root.alias == name)
                    .and_then(|root| root.condition.clone())
            })
        };
        let is_root = |name: &str| {
            roots
                .as_ref()
                .is_none_or(|roots| roots.iter().any(|root| root.alias == name))
        };
        // Roots play in their authored order, clips and controllers interleaved.
        let root_order = |name: &str| {
            roots
                .as_ref()
                .and_then(|roots| roots.iter().position(|root| root.alias == name))
                .map_or(0, |position| u16::try_from(position).unwrap_or(u16::MAX))
        };
        let mut pending_geometries = Vec::new();
        for (candidate_geometry, condition) in geometry_candidates {
            let mut animation_bindings = Vec::new();
            let mut controller_bindings = Vec::new();
            let targets = animation_aliases
                .iter()
                .map(|(name, target)| (name, target, target.starts_with("controller.animation.")))
                .chain(
                    controller_aliases
                        .iter()
                        .map(|(name, target)| (name, target, true)),
                );
            for (name, target, is_controller) in targets {
                let symbols = if is_controller {
                    &controller_symbols
                } else {
                    &animation_symbols
                };
                // Ambiguity rejects the rig even for an inactive alias a controller may reach.
                if symbols.get(target.as_ref()).is_some_and(Option::is_none) {
                    rejected = true;
                    continue;
                }
                if !is_root(name) {
                    continue;
                }
                names.push(name.clone());
                let weight = match root_condition(name) {
                    None => None,
                    Some(condition) => match molang.compile(&condition) {
                        Ok(weight) => Some(weight),
                        Err(_) => {
                            static_fallback = true;
                            continue;
                        }
                    },
                };
                if is_controller
                    && controller_names.contains(&(
                        target.as_ref(),
                        entity_symbol as u32,
                        candidate_geometry,
                    ))
                {
                    controller_bindings.push((
                        name.clone(),
                        target.clone(),
                        weight,
                        root_order(name),
                    ));
                } else if let Some(&clip) = (!is_controller)
                    .then(|| clip_indices.get(&(target.clone(), candidate_geometry)))
                    .flatten()
                {
                    animation_bindings.push((name.clone(), clip, weight, root_order(name)));
                } else {
                    // A reference the pack never defines leaves the rig static.
                    static_fallback = true;
                }
            }
            animation_bindings.sort_by(|left, right| (left.3, &left.0).cmp(&(right.3, &right.0)));
            controller_bindings.sort_by(|left, right| (left.3, &left.0).cmp(&(right.3, &right.0)));
            controller_bindings.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
            pending_geometries.push(PendingRigGeometry {
                geometry: candidate_geometry,
                condition,
                animations: animation_bindings,
                controllers: controller_bindings,
            });
        }
        if rejected {
            outcomes.push(reject(
                if description
                    .get("animations")
                    .and_then(Value::as_object)
                    .is_some_and(|animations| {
                        animations.values().filter_map(Value::as_str).any(|target| {
                            animation_symbols.get(target).is_some_and(Option::is_none)
                                || controller_symbols.get(target).is_some_and(Option::is_none)
                        })
                    })
                {
                    RejectReason::AmbiguousAnimationReference
                } else {
                    RejectReason::MissingAnimationReference
                },
            ));
            continue;
        }
        static_fallback |= optional_condition
            .is_some_and(|expression| MolangCompiler::default().compile(expression).is_err());
        let scripts = RigScripts::compile(description, molang)?;
        if static_fallback || scripts.dropped > 0 {
            outcomes.push(CompileReferenceOutcome::OptionalStaticFallback {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: FallbackReason::UnsupportedOptionalExpression,
            });
        }
        let rig_index = rigs.len() as u32;
        rigs.push(PendingRig {
            entity_symbol: entity_symbol as u32,
            render_controller,
            geometries: pending_geometries,
            fallback: if static_fallback {
                EntityRigFallback::GeometryOnly
            } else {
                EntityRigFallback::Skip
            },
            initialize: scripts.initialize,
            pre_animation: scripts.pre_animation,
            scale: scripts.scale,
            scale_expressions: scripts.scale_expressions,
        });
        outcomes.push(CompileReferenceOutcome::Resolved(rig_index));
    }
    Ok((PendingRigPayload { rigs, outcomes }, names))
}

struct RigScripts {
    initialize: Option<u32>,
    pre_animation: Option<u32>,
    scale: EntityGeometryScalar,
    scale_expressions: Option<[u32; 4]>,
    dropped: usize,
}

impl RigScripts {
    fn compile(
        description: &Map<String, Value>,
        molang: &mut MolangCompiler,
    ) -> Result<Self, AssetError> {
        let scripts = description.get("scripts").and_then(Value::as_object);
        let mut dropped = 0;
        let mut script = |field: &str| -> Result<Option<u32>, AssetError> {
            let (script, skipped) =
                molang.compile_script_value(scripts.and_then(|scripts| scripts.get(field)))?;
            dropped += skipped;
            Ok(script)
        };
        let initialize = script("initialize")?;
        let pre_animation = script("pre_animation")?;
        let field = |name: &str| scripts.and_then(|scripts| scripts.get(name));
        let constant = match field("scale") {
            None => Some(1.0),
            Some(Value::Number(number)) => number.as_f64().map(|value| value as f32),
            Some(Value::String(text)) => text.trim().parse::<f32>().ok(),
            Some(_) => None,
        };
        // A Molang scale (cow's `query.is_baby ? 2.0 : 1.0`) or any axis scale is evaluated
        // per tick; only a lone constant stays a constant.
        let mut scale_expressions = None;
        if constant.is_none()
            || ["scaleX", "scaleY", "scaleZ"]
                .iter()
                .any(|name| field(name).is_some())
        {
            let mut compiled = [0; 4];
            let mut complete = true;
            for (slot, name) in ["scale", "scaleX", "scaleY", "scaleZ"].iter().enumerate() {
                let text = match field(name) {
                    None => "1.0".to_owned(),
                    Some(Value::Number(number)) => number.to_string(),
                    Some(Value::String(text)) => text.clone(),
                    Some(_) => {
                        complete = false;
                        break;
                    }
                };
                match molang.compile(&text) {
                    Ok(index) => compiled[slot] = index,
                    Err(_) => {
                        complete = false;
                        break;
                    }
                }
            }
            if complete {
                scale_expressions = Some(compiled);
            } else {
                dropped += 1;
            }
        }
        let scale = constant
            .filter(|scale| *scale > 0.0)
            .and_then(EntityGeometryScalar::new)
            .or_else(|| EntityGeometryScalar::new(1.0))
            .ok_or_else(|| invalid("unit scale is representable"))?;
        Ok(Self {
            initialize,
            pre_animation,
            scale,
            scale_expressions,
            dropped,
        })
    }
}

/// First render controller that a remote third-person view selects: an unconditional entry or
/// one whose condition holds with every query and variable at its zero default.
fn select_render_controller(value: Option<&Value>) -> Option<(&str, Option<&str>)> {
    let mut first = None;
    for entry in value?.as_array()? {
        let (identifier, condition) = match entry {
            Value::String(identifier) => (identifier.as_str(), None),
            Value::Object(conditional) => match conditional.iter().next() {
                Some((identifier, condition)) => (identifier.as_str(), condition.as_str()),
                None => continue,
            },
            _ => continue,
        };
        first.get_or_insert((identifier, condition));
        if condition.is_none_or(|condition| {
            MolangCompiler::evaluate_default(condition).is_some_and(|value| value != 0.0)
        }) {
            return Some((identifier, condition));
        }
    }
    first
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
