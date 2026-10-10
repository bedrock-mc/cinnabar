//! Animated item models use the same compiled scripts, controllers and pose VM as actors.
use super::*;

mod preview;
pub(super) mod static_draw;
use preview::Preview;
type AttachableKey = (ActorLifetimeId, bool, bool, bool, u8);

// Every hand, perspective, worn-state and worn-slot key fits for all admitted owners.
const STATES_PER_OWNER: usize = 1 << 5;
const MAX_ATTACHABLE_STATES: usize = crate::actor_store::MAX_TRACKED_ACTORS * STATES_PER_OWNER;

/// Native item-render inputs; duration values are ticks, not the actor VM's seconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct AttachableAnimationInput<'a> {
    pub first_person: bool,
    /// Whether the owner belongs to the current game window.
    pub is_local_player: bool,
    pub off_hand: bool,
    /// A worn model has independent controller state from either held item.
    pub worn: bool,
    /// Armour slot index of a worn model, so each worn piece keeps its own state.
    pub worn_slot: u8,
    pub is_paperdoll: bool,
    pub frame_alpha: f32,
    /// Elapsed render time for clip application; absent inputs use the actor timestep.
    pub delta_seconds: Option<f32>,
    /// Animation frame of the owner's main-hand item, visible to either rendered hand.
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
    pub worn: bool,
    pub worn_slot: u8,
    pub is_paperdoll: bool,
    pub frame_alpha: f32,
    pub delta_seconds: Option<f32>,
    pub animation_frame: u32,
    pub use_elapsed_ticks: Option<u32>,
    pub max_use_ticks: u32,
    pub owner_life_tick: u64,
}

impl AttachableQueryContext {
    /// Returns the model binding name assigned to the rendered equipment slot.
    pub(super) fn item_slot(self) -> &'static str {
        if self.worn {
            match self.worn_slot {
                0 => "head",
                1 => "torso",
                4 => "body",
                _ => "",
            }
        } else if self.off_hand {
            "off_hand"
        } else {
            "main_hand"
        }
    }
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
    bones: &'a [RuntimeBone],
    layer_skeletons: &'a BTreeMap<u32, Option<Arc<render::LayerSkeleton>>>,
}

/// The skeletal frame inherited by an attachable root and all its descendants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AttachableBoneParent<'a> {
    Actor,
    OwnerNamed(&'a str),
    BindingExpression,
}

impl<'a> AttachableRigSnapshot<'a> {
    /// Resolves the selected geometry's root without allocating or copying its pose.
    pub fn bone_parent(&self, geometry: u32, index: usize) -> Option<AttachableBoneParent<'a>> {
        let (bones, names) = if geometry == self.geometry {
            (self.bones, self.bone_names)
        } else {
            let skeleton = self.layer_skeletons.get(&geometry)?.as_ref()?;
            (skeleton.bones.as_slice(), skeleton.names.as_slice())
        };
        let mut root = index;
        for _ in 0..bones.len() {
            let bone = bones.get(root)?;
            if let Some(parent) = bone.parent {
                root = parent;
                continue;
            }
            return Some(match bone.attachable_root {
                AttachableRootFrame::Actor => AttachableBoneParent::Actor,
                AttachableRootFrame::MatchingOwnerName => {
                    AttachableBoneParent::OwnerNamed(names.get(root)?)
                }
                AttachableRootFrame::BindingExpression => AttachableBoneParent::BindingExpression,
            });
        }
        None
    }
}

#[derive(Debug)]
struct AttachableState {
    identifier: Arc<str>,
    rig: ActorRigState,
    last_used: u64,
    preview: Option<Preview>,
    static_draw: Option<static_draw::StaticDraw>,
}

/// Retained script/controller state per owner, hand and render perspective.
#[derive(Debug)]
pub struct AttachablesRuntime {
    assets: Arc<RuntimeEntityAssets>,
    layout: VariableLayout,
    states: BTreeMap<AttachableKey, AttachableState>,
    evaluations: u64,
    previewing: bool,
    previewed: Vec<AttachableKey>,
    #[cfg(test)]
    commits: u64,
}

impl AttachablesRuntime {
    pub fn new(assets: Arc<RuntimeEntityAssets>) -> Self {
        Self {
            layout: VariableLayout::new(&assets),
            assets,
            states: BTreeMap::new(),
            evaluations: 0,
            previewing: false,
            previewed: Vec::new(),
            #[cfg(test)]
            commits: 0,
        }
    }

    /// Discards owner lifetimes when a session ends or its active asset catalog changes.
    pub fn clear(&mut self) {
        self.states.clear();
        self.previewed.clear();
        self.previewing = false;
    }

    /// Starts exact readiness draws while keeping their authored VM results uncommitted.
    pub fn begin_preview(&mut self) {
        self.finish_preview(false);
        self.previewing = true;
    }

    /// Leaves preview evaluation mode while retaining the bounded hand results for selection.
    pub fn end_preview(&mut self) {
        self.previewing = false;
    }

    /// Commits reused readiness results once, or restores discarded geometry before final drawing.
    pub fn finish_preview(&mut self, commit: bool) {
        self.previewing = false;
        for key in self.previewed.drain(..) {
            if let Some(state) = self.states.get_mut(&key) {
                if commit {
                    let committed = state.commit_preview();
                    #[cfg(test)]
                    {
                        self.commits += u64::from(committed);
                    }
                    #[cfg(not(test))]
                    let _ = committed;
                } else {
                    state.discard_preview();
                }
            }
        }
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
        let key = (
            owner_rig.actor,
            input.off_hand,
            input.first_person,
            input.worn,
            if input.worn { input.worn_slot } else { 0 },
        );
        if self
            .states
            .first_key_value()
            .is_some_and(|((actor, ..), _)| actor.session_id != owner_rig.actor.session_id)
        {
            self.states.clear();
            self.previewed.clear();
        }
        if !self.states.contains_key(&key) {
            let first = ActorLifetimeId {
                spawn_revision: 0,
                ..owner_rig.actor
            };
            let last = ActorLifetimeId {
                spawn_revision: u64::MAX,
                ..owner_rig.actor
            };
            let mut stale = [None; STATES_PER_OWNER];
            for (slot, (&key, _)) in stale.iter_mut().zip(
                self.states
                    .range((first, false, false, false, 0)..=(last, true, true, true, u8::MAX)),
            ) {
                if key.0 != owner_rig.actor {
                    *slot = Some(key);
                }
            }
            for key in stale.into_iter().flatten() {
                self.states.remove(&key);
            }
        }
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
                    preview: None,
                    static_draw: None,
                },
            );
        }
        let entry = self.states.get_mut(&key)?;
        entry.discard_preview();
        entry.last_used = self.evaluations;
        let static_input = static_draw::Inputs::new(owner, owner_rig, input);
        if !self.previewing
            && entry.static_draw.as_ref().is_some_and(|cached| {
                cached.input == static_input && cached.owner_names == owner_rig.bone_names
            })
        {
            entry.rig.completed_tick = owner_rig.completed_tick;
            return entry.snapshot(&self.assets, binding);
        }
        entry.static_draw = None;
        let state = &mut entry.rig;
        let frame_alpha = if input.frame_alpha.is_finite() {
            input.frame_alpha.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let query_context = AttachableQueryContext {
            first_person: input.first_person,
            off_hand: input.off_hand,
            worn: input.worn,
            worn_slot: input.worn_slot,
            is_paperdoll: input.is_paperdoll,
            frame_alpha,
            delta_seconds: input
                .delta_seconds
                .filter(|delta| delta.is_finite() && *delta >= 0.0),
            animation_frame: input.animation_frame,
            use_elapsed_ticks: input.use_elapsed_ticks,
            max_use_ticks: input.max_use_ticks,
            owner_life_tick: owner_rig.animation_variables.life_tick(),
        };
        let (main_hand_kinetic, main_hand_swing_seconds) =
            owner_rig.animation_variables.item_timings();
        let mut context = ActorTickContext {
            main_hand_kinetic,
            main_hand_swing_seconds,
            main_hand_is_spear: owner_rig.animation_variables.is_spear(),
            is_local_first_person: input.first_person,
            is_local_player: input.is_local_player || input.first_person,
            hand_charged: input.hand_charged,
            main_hand_max_use_ticks: input.max_use_ticks,
            attachable: Some(query_context),
            main_hand: input.owner_main_hand.map(Arc::from),
            off_hand: input.owner_off_hand.map(Arc::from),
            ..owner_rig
                .animation_variables
                .owner_context()
                .cloned()
                .unwrap_or_default()
        };
        if !input.worn && input.off_hand {
            context
                .off_hand
                .get_or_insert_with(|| Arc::from(identifier));
        } else if !input.worn {
            context
                .main_hand
                .get_or_insert_with(|| Arc::from(identifier));
        }
        let mut item =
            owner_rig.item_animation[0].interpolate(owner_rig.item_animation[1], frame_alpha);
        item.attack_time = owner_rig.item_animation[0]
            .interpolate(
                owner_rig.item_animation[1],
                owner_rig.java.local_swing_alpha.unwrap_or(frame_alpha),
            )
            .attack_time;
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
            ..owner_rig.animation_variables.input().unwrap_or_default()
        });
        let mut world_left = MAX_MOLANG_OPS_PER_ACTOR_TICK;
        let mut budget = EvalBudget {
            actor_left: MAX_MOLANG_OPS_PER_ACTOR_TICK,
            world_left: &mut world_left,
            work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
            transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
            used: 0,
            stack: Vec::new(),
            static_draw: (!self.previewing)
                .then_some(self.assets.rig_bindings()[binding].geometry_count == 1),
        };
        render::cache_layer_skeletons(&self.assets, state);
        let geometry = if self.previewing {
            geometry::reselect_geometry_preview(
                &self.assets,
                &self.layout,
                state,
                owner,
                &context,
                &mut budget,
            )
        } else {
            geometry::reselect_geometry(
                &self.assets,
                &self.layout,
                state,
                owner,
                &context,
                &mut budget,
            );
            None
        };
        render::bind_attachable_roots(state, owner_rig.bone_names);
        let evaluated = match evaluate_state(
            &self.assets,
            &self.layout,
            state,
            owner,
            &context,
            owner_rig.completed_tick,
            &mut budget,
            true,
            Some(tick::EvaluationInheritance {
                variables: owner_rig.animation_variables,
                overrides: input.owner_variables,
            }),
        ) {
            Ok(evaluated) => evaluated,
            Err(_) => {
                if let Some(geometry) = geometry {
                    geometry.restore(state);
                }
                return None;
            }
        };
        let retain_static = budget.static_draw == Some(true)
            && evaluated.render.is_some()
            && evaluated.clip_clocks.is_empty()
            && static_draw::eligible(state, &evaluated.controllers);
        if self.previewing {
            entry.preview = Some(Preview {
                evaluated,
                tick: owner_rig.completed_tick,
                geometry,
            });
            if !self.previewed.contains(&key) {
                self.previewed.push(key);
            }
        } else {
            let drawable = entry.commit(evaluated, owner_rig.completed_tick);
            #[cfg(test)]
            {
                self.commits += 1;
            }
            if !drawable {
                return None;
            }
            if retain_static {
                entry.static_draw = Some(static_draw::StaticDraw {
                    input: static_input,
                    owner_names: owner_rig.bone_names.to_vec(),
                });
            }
        }
        entry.snapshot(&self.assets, binding)
    }
}

/// Vanilla attachable setup distinguishes expression bindings from owner-name
/// matches. Only the latter clear the authored default TRS; animation restores the
/// former's bone defaults afterward.
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

#[cfg(test)]
#[path = "attachable/preview_tests.rs"]
mod preview_tests;

#[cfg(test)]
#[path = "attachable/owner_reference_tests.rs"]
mod owner_reference_tests;

#[cfg(test)]
#[path = "attachable/owner_context_tests.rs"]
mod owner_context_tests;

#[cfg(test)]
#[path = "attachable/activation_clock_tests.rs"]
mod activation_clock_tests;

#[cfg(test)]
#[path = "attachable/parent_frame_tests.rs"]
mod parent_frame_tests;
