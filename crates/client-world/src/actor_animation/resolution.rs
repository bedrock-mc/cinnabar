use super::*;

pub(super) fn local_player_resolution_actor() -> ActorSnapshot {
    let pose = crate::actor_store::ActorPose {
        position: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
    };
    ActorSnapshot {
        unique_id: 0,
        runtime_id: 0,
        spawn_revision: 0,
        movement_revision: 0,
        kind: ActorKind::Player {
            uuid: [0; 16],
            username: Arc::from(""),
        },
        game_mode: None,
        resolved_game_mode: None,
        game_mode_tick: None,
        position: pose.position,
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        previous_pose: pose,
        received_pose: pose,
        interpolation_ticks_remaining: 0,
        body_yaw: 0.0,
        on_ground: Some(true),
        teleported: false,
        player_mode: None,
        source_tick: None,
        metadata: HashMap::new(),
        attributes: HashMap::new(),
        int_properties: HashMap::new(),
        float_properties: HashMap::new(),
    }
}

pub(super) fn local_player_animation_actor(input: LocalPlayerAnimationTickInput) -> ActorSnapshot {
    let mut actor = local_player_resolution_actor();
    actor.velocity = input.velocity;
    actor.on_ground = Some(input.on_ground);
    actor.body_yaw = input.body_yaw;
    actor.yaw = input.body_yaw;
    actor.head_yaw = input.head_yaw;
    actor.pitch = input.pitch;
    actor
}

pub(super) fn resolve_rig_for_skin_geometry(
    assets: &RuntimeEntityAssets,
    actor: &ActorSnapshot,
    completed_tick: u64,
    geometry: Option<&PlayerSkinGeometry>,
) -> Option<ActorRigState> {
    let requested = match geometry {
        Some(PlayerSkinGeometry::Wide) => Some(("geometry.humanoid.custom", None)),
        Some(PlayerSkinGeometry::Slim) => Some(("geometry.humanoid.customSlim", None)),
        Some(PlayerSkinGeometry::Custom {
            identifier,
            data_sha256,
        }) => Some((identifier.as_ref(), Some(data_sha256))),
        None => None,
    };
    let state = resolve_rig_for_geometry(assets, actor, completed_tick, requested)?;
    Some(state)
}

pub(super) fn resolve_rig_for_geometry(
    assets: &RuntimeEntityAssets,
    actor: &ActorSnapshot,
    completed_tick: u64,
    requested_geometry: Option<(&str, Option<&[u8; 32]>)>,
) -> Option<ActorRigState> {
    let identifier = match &actor.kind {
        ActorKind::Player { .. } => "minecraft:player",
        ActorKind::Entity { identifier } => identifier,
    };
    let entity_symbols = assets.symbol_candidates(EntityAssetKind::Entity, identifier);
    let [entity_symbol] = entity_symbols else {
        return None;
    };
    let entity_symbol_index = assets
        .symbols()
        .iter()
        .position(|symbol| std::ptr::eq(symbol, entity_symbol))?;
    let mut rigs = assets
        .rig_bindings()
        .iter()
        .filter(|rig| rig.entity_symbol as usize == entity_symbol_index);
    let rig = rigs.next()?;
    if rigs.next().is_some() {
        return None;
    }
    let texture = rig.default_texture.map(|index| index as usize);
    let first = rig.first_geometry as usize;
    let end = first.checked_add(rig.geometry_count as usize)?;
    let candidates = assets.rig_geometries().get(first..end)?;
    let mut world_left = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    let mut budget = EvalBudget {
        actor_left: MAX_MOLANG_OPS_PER_ACTOR_TICK,
        world_left: &mut world_left,
        work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
        transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
        used: 0,
    };
    let requested_offset = requested_geometry.map(|(identifier, expected_sha256)| {
        let mut matches = candidates
            .iter()
            .enumerate()
            .filter_map(|(offset, candidate)| {
                assets
                    .geometries()
                    .get(candidate.geometry as usize)
                    .is_some_and(|geometry| {
                        geometry.identifier.as_ref() == identifier
                            && expected_sha256
                                .is_none_or(|expected| &geometry.semantic_sha256 == expected)
                    })
                    .then_some(offset)
            });
        let selected = matches.next()?;
        matches.next().is_none().then_some(selected)
    });
    let mut candidate_offset = match requested_offset {
        Some(Some(offset)) => offset,
        Some(None) => return None,
        None => 0,
    };
    let empty_history = VecDeque::new();
    for (offset, candidate) in candidates.iter().enumerate().skip(1) {
        if requested_geometry.is_some() {
            break;
        }
        let selected = evaluate_expression(
            assets,
            candidate.condition? as usize,
            actor,
            &empty_history,
            0,
            0,
            &mut budget,
        )
        .ok()?;
        if truthy(selected) {
            candidate_offset = offset;
            break;
        }
    }
    let geometry_binding = first + candidate_offset;
    let candidate = &assets.rig_geometries()[geometry_binding];
    if candidate.animation_count as usize + candidate.controller_count as usize
        > MAX_RUNTIME_BINDINGS_PER_RIG
    {
        return None;
    }
    let bones = resolve_bones(assets, candidate.geometry as usize)?;
    let current = compose_pose(&bones, &[])?;
    let controller_first = candidate.first_controller as usize;
    let controller_end = controller_first.checked_add(candidate.controller_count as usize)?;
    let controllers = assets
        .rig_controllers()
        .get(controller_first..controller_end)?
        .iter()
        .map(|binding| {
            let controller = assets.controllers().get(binding.controller as usize)?;
            Some(ControllerState {
                controller: binding.controller as usize,
                state: controller.initial_state,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(ActorRigState {
        // The renderer needs the resolved geometry candidate, not only the
        // entity-level binding that may contain several candidates.
        rig: EntityRigId(geometry_binding as u32),
        geometry_binding,
        bones,
        controllers,
        previous: current.clone(),
        current,
        reset_generation: 0,
        reset_pending: false,
        lifetime_epoch: completed_tick,
        animation_epoch: completed_tick,
        completed_tick,
        fallback: rig.fallback,
        texture,
        history: VecDeque::with_capacity(MAX_ACTOR_ACTION_HISTORY),
    })
}

fn resolve_bones(assets: &RuntimeEntityAssets, geometry_index: usize) -> Option<Vec<RuntimeBone>> {
    let parents = validate_entity_geometry_inheritance(assets.geometries()).ok()?;
    let mut chain = Vec::new();
    let mut current = geometry_index;
    for _ in 0..=parents.len() {
        chain.push(current);
        let Some(parent) = parents.get(current).copied().flatten() else {
            break;
        };
        current = parent;
    }
    if chain
        .last()
        .and_then(|index| parents.get(*index))
        .copied()
        .flatten()
        .is_some()
    {
        return None;
    }
    chain.reverse();
    let mut merged: Vec<EntityGeometryBone> = Vec::new();
    for index in chain {
        for child in assets.geometries().get(index)?.bones.iter() {
            if let Some(existing) = merged
                .iter_mut()
                .find(|bone| bone.name.eq_ignore_ascii_case(&child.name))
            {
                overlay_bone(existing, child);
            } else {
                merged.push(child.clone());
            }
        }
    }
    if merged.len() > MAX_RUNTIME_BONES_PER_RIG {
        return None;
    }
    merged
        .iter()
        .map(|bone| {
            let parent = bone.parent.as_ref().map(|name| {
                merged
                    .iter()
                    .position(|candidate| candidate.name.eq_ignore_ascii_case(name))
            });
            Some(RuntimeBone {
                name: bone.name.clone(),
                parent: match parent {
                    Some(Some(index)) => Some(index),
                    Some(None) => return None,
                    None => None,
                },
                pivot: scalars(bone.pivot.as_ref()),
                rotation: scalars(bone.rotation.as_ref()),
            })
        })
        .collect()
}

fn overlay_bone(base: &mut EntityGeometryBone, child: &EntityGeometryBone) {
    if child.parent.is_some() {
        base.parent.clone_from(&child.parent);
    }
    if child.pivot.is_some() {
        base.pivot = child.pivot;
    }
    if child.rotation.is_some() {
        base.rotation = child.rotation;
    }
    if child.mirror.is_some() {
        base.mirror = child.mirror;
    }
    if child.inflate.is_some() {
        base.inflate = child.inflate;
    }
    if child.never_render.is_some() {
        base.never_render = child.never_render;
    }
    if child.reset.is_some() {
        base.reset = child.reset;
    }
    if !child.cubes.is_empty() {
        base.cubes.clone_from(&child.cubes);
    }
}

fn scalars(values: Option<&[assets::EntityGeometryScalar; 3]>) -> [f32; 3] {
    values.map_or([0.0; 3], |values| values.map(|value| value.get()))
}
