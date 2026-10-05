//! Animated item models use the same compiled scripts, controllers and pose VM as actors.
use super::*;

const MAX_ATTACHABLE_STATES: usize = 256;

/// Native item-render inputs; duration values are ticks, not the actor VM's seconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct AttachableAnimationInput<'a> {
    pub first_person: bool,
    pub off_hand: bool,
    pub is_paperdoll: bool,
    pub frame_alpha: f32,
    pub animation_frame: u32,
    /// Owner's elapsed main-hand use ticks, also visible to offhand attachables.
    pub use_elapsed_ticks: Option<u32>,
    pub max_use_ticks: u32,
    pub hand_charged: bool,
    /// Both owner hands remain visible to item-name queries while one attachable is rendered.
    pub owner_main_hand: Option<&'a str>,
    pub owner_off_hand: Option<&'a str>,
    /// Additional owner variables copied before the item's scripts run.
    pub owner_variables: &'a [(&'a str, f32)],
}

impl AttachableAnimationInput<'_> {
    /// Selects the rendered hand from the owner's first-person use frame.
    pub fn for_hand(self, off_hand: bool) -> Self {
        if off_hand {
            Self {
                off_hand,
                animation_frame: 0,
                hand_charged: false,
                ..self
            }
        } else {
            self
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AttachableQueryContext {
    pub first_person: bool,
    pub off_hand: bool,
    pub is_paperdoll: bool,
    pub frame_alpha: f32,
    pub animation_frame: u32,
    pub use_elapsed_ticks: Option<u32>,
    pub max_use_ticks: u32,
    pub owner_life_tick: u64,
}

/// Evaluated geometry and layers in the item's own model frame, before the owner bone.
#[derive(Clone, Copy, Debug)]
pub struct AttachableRigSnapshot<'a> {
    pub geometry: u32,
    pub pose: &'a [BoneTransform],
    pub bone_names: &'a [Box<str>],
    pub render: &'a [RenderTextureLayer],
    pub scale: f32,
    pub axis_scale: [f32; 3],
}

#[derive(Debug)]
struct AttachableState {
    identifier: Arc<str>,
    rig: ActorRigState,
    last_used: u64,
}

/// Retained script/controller state per owner, hand and render perspective.
#[derive(Debug)]
pub struct AttachablesRuntime {
    assets: Arc<RuntimeEntityAssets>,
    layout: VariableLayout,
    states: BTreeMap<(ActorLifetimeId, bool, bool), AttachableState>,
    evaluations: u64,
}

impl AttachablesRuntime {
    pub fn new(assets: Arc<RuntimeEntityAssets>) -> Self {
        Self {
            layout: VariableLayout::new(&assets),
            assets,
            states: BTreeMap::new(),
            evaluations: 0,
        }
    }

    /// Discards owner lifetimes when a session ends or its active asset catalog changes.
    pub fn clear(&mut self) {
        self.states.clear();
    }

    /// Runs authored item scripts with owner queries and native render-time item inputs.
    /// The returned pose has already sampled `frame_alpha`; do not interpolate it again.
    pub fn evaluate(
        &mut self,
        identifier: &str,
        owner: &ActorSnapshot,
        owner_rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
    ) -> Option<AttachableRigSnapshot<'_>> {
        let key = (owner_rig.actor, input.off_hand, input.first_person);
        // Ended sessions and a reused runtime ID's previous owner carry no script state.
        self.states.retain(|(actor, _, _), _| {
            actor.session_id == owner_rig.actor.session_id
                && (actor.runtime_id != owner_rig.actor.runtime_id || *actor == owner_rig.actor)
        });
        let binding = self.assets.attachable_rig_binding(identifier)?;
        self.evaluations += 1;
        // Departed owners are never announced here; the least recently drawn state makes room.
        if !self.states.contains_key(&key)
            && self.states.len() >= MAX_ATTACHABLE_STATES
            && let Some(stale) = self
                .states
                .iter()
                .min_by_key(|(_, state)| state.last_used)
                .map(|(key, _)| *key)
        {
            self.states.remove(&stale);
        }
        if self
            .states
            .get(&key)
            .is_none_or(|state| state.identifier.as_ref() != identifier)
        {
            let rig = resolve_binding(
                &self.assets,
                &self.layout,
                owner,
                owner_rig.completed_tick,
                binding,
            )?;
            self.states.insert(
                key,
                AttachableState {
                    identifier: Arc::from(identifier),
                    rig,
                    last_used: 0,
                },
            );
        }
        let state = self.states.get_mut(&key)?;
        state.last_used = self.evaluations;
        let state = &mut state.rig;
        let frame_alpha = if input.frame_alpha.is_finite() {
            input.frame_alpha.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let query_context = AttachableQueryContext {
            first_person: input.first_person,
            off_hand: input.off_hand,
            is_paperdoll: input.is_paperdoll,
            frame_alpha,
            animation_frame: input.animation_frame,
            use_elapsed_ticks: input.use_elapsed_ticks,
            max_use_ticks: input.max_use_ticks,
            owner_life_tick: owner_rig.animation_variables.life_tick(),
        };
        let mut context = ActorTickContext {
            is_local_first_person: input.first_person,
            hand_charged: input.hand_charged,
            main_hand_max_use_ticks: input.max_use_ticks,
            attachable: Some(query_context),
            main_hand: input.owner_main_hand.map(Arc::from),
            off_hand: input.owner_off_hand.map(Arc::from),
            ..ActorTickContext::default()
        };
        if input.off_hand {
            context
                .off_hand
                .get_or_insert_with(|| Arc::from(identifier));
        } else {
            context
                .main_hand
                .get_or_insert_with(|| Arc::from(identifier));
        }
        let item =
            owner_rig.item_animation[0].interpolate(owner_rig.item_animation[1], frame_alpha);
        let offhand = owner_rig.off_hand_animation[0]
            .interpolate(owner_rig.off_hand_animation[1], frame_alpha);
        state.history.clear();
        state.history.push_back(ActorTickInput {
            position: owner.position,
            velocity: owner.velocity,
            on_ground: owner.on_ground.unwrap_or(false),
            body_yaw: owner_rig.body_yaw,
            head_yaw: owner.head_yaw,
            pitch: owner.pitch,
            item_use_ticks: input.use_elapsed_ticks.unwrap_or(0),
            attack_time: item.attack_time,
            arm_height: item.arm_height,
            off_hand_arm_height: offhand.arm_height,
            ..ActorTickInput::default()
        });
        let mut world_left = MAX_MOLANG_OPS_PER_ACTOR_TICK;
        let mut budget = EvalBudget {
            actor_left: MAX_MOLANG_OPS_PER_ACTOR_TICK,
            world_left: &mut world_left,
            work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
            transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
            used: 0,
            stack: Vec::new(),
        };
        render::cache_layer_skeletons(&self.assets, state);
        geometry::reselect_geometry(
            &self.assets,
            &self.layout,
            state,
            owner,
            &context,
            &mut budget,
        );
        render::bind_attachable_roots(state, owner_rig.bone_names);
        let evaluated = evaluate_state(
            &self.assets,
            &self.layout,
            state,
            owner,
            &context,
            owner_rig.completed_tick,
            &mut budget,
            Some(tick::EvaluationInheritance {
                variables: owner_rig.animation_variables,
                overrides: input.owner_variables,
            }),
        )
        .ok()?;
        state.variables = evaluated.variables;
        state.controllers = evaluated.controllers;
        state.clip_clocks = evaluated.clip_clocks;
        state.current = evaluated.pose;
        state.scale = evaluated.scale;
        state.render = evaluated.render?;
        state.initialized = true;
        state.reset_pending = false;
        state.completed_tick = owner_rig.completed_tick;
        let geometry = self
            .assets
            .rig_geometries()
            .get(state.geometry_binding)?
            .geometry;
        Some(AttachableRigSnapshot {
            geometry,
            pose: &state.current,
            bone_names: &state.bone_names,
            render: &state.render,
            scale: state
                .scale
                .map_or(self.assets.rig_bindings()[binding].scale.get(), |s| s[0]),
            axis_scale: state.scale.map_or([1.0; 3], |s| [s[1], s[2], s[3]]),
        })
    }
}

/// Native setupAttachableNoChecks distinguishes expression
/// bindings from owner-name matches. Only the latter clear the authored default TRS;
/// applyAnimations restores the former's ModelPart defaults afterward.
pub(super) fn bind_roots(bones: &mut [RuntimeBone], names: &[Box<str>], owner_names: &[Box<str>]) {
    for (bone, name) in bones.iter_mut().zip(names) {
        bone.attachable_root = if bone.parent.is_some() {
            AttachableRootFrame::Actor
        } else if bone.has_binding_expression {
            AttachableRootFrame::BindingExpression
        } else if owner_names
            .iter()
            .any(|owner| owner.eq_ignore_ascii_case(name))
        {
            AttachableRootFrame::MatchingOwnerName
        } else {
            AttachableRootFrame::Actor
        };
    }
}

#[cfg(test)]
pub(in crate::actor_animation) mod tests;
