use super::*;

pub(super) fn compile_rigs(
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
        player_slim_condition,
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
        let is_player = entity.identifier.as_ref() == "minecraft:player";
        if symbols
            .iter()
            .filter(|candidate| {
                candidate.kind == EntityAssetKind::Entity
                    && candidate.identifier == entity.identifier
            })
            .take(2)
            .count()
            != 1
        {
            outcomes.push(CompileReferenceOutcome::RequiredRigRejected {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: RejectReason::AmbiguousRequiredReference,
            });
            continue;
        }
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
        let player_geometry = |identifier: &str| {
            geometries.iter().enumerate().find_map(|(index, geometry)| {
                (geometry.identifier.as_ref() == identifier
                    && sources
                        .get(geometry.source_index as usize)
                        .is_some_and(|source| source.path.as_ref() == "models/mobs.json"))
                .then_some(index as u32)
            })
        };
        let geometry_resolution = if is_player {
            player_geometry(geometry_name).map(Some)
        } else {
            geometry_indices.get(geometry_name).copied()
        };
        let Some(geometry) = geometry_resolution else {
            outcomes.push(CompileReferenceOutcome::RequiredRigRejected {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: RejectReason::MissingGeometryReference,
            });
            continue;
        };
        let Some(geometry) = geometry else {
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
        let mut incomplete_player_animation = false;
        let mut static_fallback = false;
        let mut geometry_candidates = vec![(geometry, None)];
        if is_player {
            let Some(slim) = player_geometry("geometry.humanoid.customSlim") else {
                outcomes.push(CompileReferenceOutcome::RequiredRigRejected {
                    source: entity.source_index,
                    symbol: entity_symbol as u32,
                    reason: RejectReason::MissingGeometryReference,
                });
                continue;
            };
            if slim != geometry {
                geometry_candidates.push((slim, Some(player_slim_condition)));
            }
        }
        match (!is_player)
            .then(|| geometry_selections.get(&(render_name.into(), entity_symbol as u32)))
            .flatten()
        {
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
                    } else if is_player {
                        incomplete_player_animation = true;
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
                    if is_player {
                        incomplete_player_animation = true;
                    } else {
                        static_fallback = true;
                    }
                } else if is_player {
                    incomplete_player_animation = true;
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
                    if is_player {
                        incomplete_player_animation = true;
                    } else {
                        static_fallback = true;
                    }
                } else if is_player {
                    incomplete_player_animation = true;
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
        static_fallback |= optional_condition
            .is_some_and(|expression| MolangCompiler::default().compile(expression).is_err());
        if incomplete_player_animation {
            outcomes.push(CompileReferenceOutcome::OptionalStaticFallback {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: FallbackReason::IncompleteAnimationReferences,
            });
        }
        if static_fallback {
            outcomes.push(CompileReferenceOutcome::OptionalStaticFallback {
                source: entity.source_index,
                symbol: entity_symbol as u32,
                reason: FallbackReason::UnsupportedOptionalExpression,
            });
        }
        let fallback = if static_fallback || incomplete_player_animation {
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
