use super::{evaluation::Evaluator, *};
use assets::{EntityControllerAnimationTarget, EntityGeometryBone};

impl<'a> ActorRigSnapshot<'a> {
    /// Resolves the rig's geometry in its owning vanilla or session-pack catalog.
    pub fn geometry_source(&self) -> Option<(&'a RuntimeEntityAssets, usize)> {
        let assets = self.animation_variables.asset_catalog()?;
        let binding = self
            .rig
            .0
            .checked_sub(assets::PACK_RIG_ID_BASE)
            .unwrap_or(self.rig.0);
        let geometry = assets.rig_geometries().get(binding as usize)?.geometry;
        Some((assets, geometry as usize))
    }
}

#[cfg(test)]
#[path = "geometry/fixtures.rs"]
mod fixtures;
#[cfg(test)]
#[path = "geometry/horse_tests.rs"]
mod horse_tests;
#[cfg(test)]
#[path = "geometry/inherited_cubes_tests.rs"]
mod inherited_cubes_tests;
#[cfg(test)]
#[path = "geometry/pig_tests.rs"]
mod pig_tests;
#[cfg(test)]
#[path = "geometry/pinned_baby_tests.rs"]
mod pinned_baby_tests;

pub(super) fn resolve_binding(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    actor: &ActorSnapshot,
    completed_tick: u64,
    rig_binding: usize,
) -> Option<ActorRigState> {
    let rig = &assets.rig_bindings()[rig_binding];
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
        stack: Vec::new(),
    };
    let mut candidate_offset = 0;
    let input = ActorTickInput {
        position: actor.position,
        velocity: actor.velocity,
        on_ground: actor.on_ground.unwrap_or(false),
        body_yaw: actor.body_yaw,
        yaw: actor.yaw,
        head_yaw: actor.head_yaw,
        pitch: actor.pitch,
        ..ActorTickInput::default()
    };
    let context = ActorTickContext::default();
    let evaluator = Evaluator {
        assets,
        layout,
        actor,
        input: &input,
        context: &context,
        anim_tick: 0,
        anim_time: None,
        life_tick: 0,
        finished: (false, false),
        bones: &[],
        bone_names: &[],
    };
    let random_seed = actor.runtime_id ^ actor.spawn_revision.rotate_left(32);
    let mut variables = layout.fresh(random_seed);
    for (offset, candidate) in candidates.iter().enumerate().skip(1) {
        let selected = evaluator
            .run(
                candidate.condition? as usize,
                &mut variables,
                0.0,
                &mut budget,
            )
            .ok()?;
        if selected.truthy() {
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
    let (bones, bone_names) = resolve_bones(assets, candidate.geometry as usize)?;
    let current = compose_pose(&bones, &[])?;
    let controller_first = candidate.first_controller as usize;
    let controller_end = controller_first.checked_add(candidate.controller_count as usize)?;
    let mut controllers = Vec::new();
    for binding in assets
        .rig_controllers()
        .get(controller_first..controller_end)?
    {
        collect_controllers(assets, binding.controller as usize, 0, &mut controllers)?;
    }
    let mut motion = MotionState::spawn(actor.body_yaw, actor.head_yaw);
    motion.horse = super::horse::AnimationState::new(random_seed);
    let samples_camera_poses = super::render_frame::camera::needs_camera_sampling(
        assets,
        rig_binding,
        geometry_binding,
        &controllers,
    );
    Some(ActorRigState {
        pack: false,
        // The renderer needs the resolved geometry candidate, not only the
        // entity-level binding that may contain several candidates.
        rig: EntityRigId(geometry_binding as u32),
        rig_binding,
        geometry_binding,
        bones,
        bone_names,
        render: Vec::new(),
        scale: None,
        layer_skeletons: BTreeMap::new(),
        controllers,
        previous: current.clone(),
        ui_pose: None,
        ui_animation: None,
        view_context: None,
        rest: current.clone(),
        rest_completed_tick: 0,
        rest_reset_generation: 0,
        rest_reset_pending: false,
        current,
        reset_generation: 0,
        reset_pending: false,
        lifetime_epoch: completed_tick,
        animation_epoch: completed_tick,
        completed_tick,
        fallback: rig.fallback,
        history: VecDeque::with_capacity(MAX_ACTOR_ACTION_HISTORY),
        equipped_main: None,
        equipped_off: None,
        // Vanilla starts both offhand observations at zero.
        off_hand_animation: [ItemAnimationState {
            attack_time: 0.0,
            arm_height: 0.0,
        }; 2],
        skin: None,
        skin_layers: Vec::new(),
        variables,
        samples_render_frames: samples_camera_poses
            || super::render_frame::needs_frame_sampling(assets, rig_binding),
        samples_camera_poses,
        render_frame: None,
        clip_clocks: BTreeMap::new(),
        initialized: false,
        culled: false,
        motion,
    })
}

/// Adds one runtime state per controller reachable from a rig root, each once.
pub(super) fn collect_controllers(
    assets: &RuntimeEntityAssets,
    controller: usize,
    depth: usize,
    output: &mut Vec<ControllerState>,
) -> Option<()> {
    if depth >= assets::MAX_ENTITY_CONTROLLER_NESTING {
        return None;
    }
    if output
        .iter()
        .any(|runtime| runtime.controller == controller)
    {
        return Some(());
    }
    let compiled = assets.controllers().get(controller)?;
    output.push(ControllerState {
        controller,
        state: compiled.initial_state,
        active: false,
        entered_tick: 0,
    });
    let states = assets.controller_states().get(
        compiled.first_state as usize
            ..compiled.first_state as usize + compiled.state_count as usize,
    )?;
    for state in states {
        let animations = assets.controller_animations().get(
            state.first_animation as usize
                ..state.first_animation as usize + state.animation_count as usize,
        )?;
        for animation in animations {
            if let EntityControllerAnimationTarget::Controller(nested) = animation.target {
                collect_controllers(assets, nested as usize, depth + 1, output)?;
            }
        }
    }
    Some(())
}

pub(super) fn resolve_bones(
    assets: &RuntimeEntityAssets,
    geometry_index: usize,
) -> Option<(Vec<RuntimeBone>, Vec<Box<str>>)> {
    let parents = assets.geometry_parents();
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
                if merged.len() >= MAX_RUNTIME_BONES_PER_RIG {
                    return None;
                }
                merged.push(bone_metadata(child));
            }
        }
    }
    skeleton(&merged)
}

/// Runtime bones and lowercase names of a merged bone list; parents must resolve by name.
pub(super) fn skeleton(merged: &[EntityGeometryBone]) -> Option<(Vec<RuntimeBone>, Vec<Box<str>>)> {
    if merged.len() > MAX_RUNTIME_BONES_PER_RIG {
        return None;
    }
    let names = merged
        .iter()
        .map(|bone| bone.name.to_ascii_lowercase().into_boxed_str())
        .collect();
    let bones = merged
        .iter()
        .map(|bone| {
            let parent = bone.parent.as_ref().map(|name| {
                merged
                    .iter()
                    .position(|candidate| candidate.name.eq_ignore_ascii_case(name))
            });
            Some(RuntimeBone {
                parent: match parent {
                    Some(Some(index)) => Some(index),
                    Some(None) => return None,
                    None => None,
                },
                pivot: mirror_x(scalars(bone.pivot.as_ref())),
                rotation: scalars(bone.rotation.as_ref()),
                has_binding_expression: bone.binding.is_some(),
                attachable_root: AttachableRootFrame::Actor,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some((bones, names))
}

// A skeleton consumes only bone defaults. Never copy or accumulate mesh payloads while
// following an inheritance chain; mesh resolution has its own cumulative cube budget.
fn bone_metadata(bone: &EntityGeometryBone) -> EntityGeometryBone {
    EntityGeometryBone {
        name: bone.name.clone(),
        parent: bone.parent.clone(),
        pivot: bone.pivot,
        rotation: bone.rotation,
        bind_pose_rotation: bone.bind_pose_rotation,
        mirror: bone.mirror,
        inflate: bone.inflate,
        never_render: bone.never_render,
        reset: bone.reset,
        binding: bone.binding.clone(),
        texture_meshes: Box::default(),
        cubes: Box::default(),
    }
}

fn overlay_bone(base: &mut EntityGeometryBone, child: &EntityGeometryBone) {
    if child.binding.is_some() {
        base.binding.clone_from(&child.binding);
    }
    if child.parent.is_some() {
        base.parent.clone_from(&child.parent);
    }
    if child.pivot.is_some() {
        base.pivot = child.pivot;
    }
    if child.rotation.is_some() {
        base.rotation = child.rotation;
    }
    if child.bind_pose_rotation.is_some() {
        base.bind_pose_rotation = child.bind_pose_rotation;
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
}

/// Maps authored geometry coordinates into the rig frame, whose X axis is mirrored.
fn mirror_x(point: [f32; 3]) -> [f32; 3] {
    [-point[0], point[1], point[2]]
}

fn scalars(values: Option<&[assets::EntityGeometryScalar; 3]>) -> [f32; 3] {
    values.map_or([0.0; 3], |values| values.map(|value| value.get()))
}

/// Re-evaluates a multi-geometry rig's candidate conditions and, when another candidate now
/// holds (a baby grows up, a sheep is sheared), swaps the rig's bones and controllers to it.
pub(super) fn reselect_geometry(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
    state: &mut ActorRigState,
    actor: &ActorSnapshot,
    context: &ActorTickContext,
    budget: &mut EvalBudget<'_>,
) {
    let Some(rig) = assets.rig_bindings().get(state.rig_binding) else {
        return;
    };
    if rig.geometry_count < 2 {
        return;
    }
    let first = rig.first_geometry as usize;
    let Some(candidates) = assets
        .rig_geometries()
        .get(first..first + usize::from(rig.geometry_count))
    else {
        return;
    };
    let Some(input) = state.history.back().copied() else {
        return;
    };
    let evaluator = Evaluator {
        assets,
        layout,
        actor,
        input: &input,
        context,
        anim_tick: 0,
        anim_time: None,
        life_tick: 0,
        finished: (false, false),
        bones: &state.bones,
        bone_names: &state.bone_names,
    };
    let mut variables = state.variables.clone();
    let mut selected = first;
    for (offset, candidate) in candidates.iter().enumerate().skip(1) {
        let Some(condition) = candidate.condition else {
            return;
        };
        match evaluator.run(condition as usize, &mut variables, 0.0, budget) {
            Ok(value) if value.truthy() => {
                selected = first + offset;
                break;
            }
            Ok(_) => {}
            Err(_) => return,
        }
    }
    if selected == state.geometry_binding {
        return;
    }
    let candidate = &assets.rig_geometries()[selected];
    if candidate.animation_count as usize + candidate.controller_count as usize
        > MAX_RUNTIME_BINDINGS_PER_RIG
    {
        return;
    }
    let Some((bones, bone_names)) = resolve_bones(assets, candidate.geometry as usize) else {
        return;
    };
    let Some(pose) = compose_pose(&bones, &[]) else {
        return;
    };
    let controller_first = candidate.first_controller as usize;
    let Some(bindings) = assets
        .rig_controllers()
        .get(controller_first..controller_first + usize::from(candidate.controller_count))
    else {
        return;
    };
    let mut controllers = Vec::new();
    for binding in bindings {
        if collect_controllers(assets, binding.controller as usize, 0, &mut controllers).is_none() {
            return;
        }
    }
    let rig_id = if state.pack {
        assets::PACK_RIG_ID_BASE.checked_add(selected as u32)
    } else {
        Some(selected as u32)
    };
    let Some(rig_id) = rig_id else {
        return;
    };
    state.rig = EntityRigId(rig_id);
    state.samples_camera_poses = super::render_frame::camera::needs_camera_sampling(
        assets,
        state.rig_binding,
        selected,
        &controllers,
    );
    state.samples_render_frames = state.samples_camera_poses
        || super::render_frame::needs_frame_sampling(assets, state.rig_binding);
    state.geometry_binding = selected;
    state.bones = bones;
    state.bone_names = bone_names;
    state.controllers = controllers;
    state.ui_pose = None;
    state.ui_animation = None;
    state.previous = pose.clone();
    state.rest = pose.clone();
    state.current = pose;
    state.reset_pending = true;
    state.rest_reset_pending = true;
}
