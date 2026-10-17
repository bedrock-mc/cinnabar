//! Owner conditions select an item definition before its animation state is created.
use super::*;
use crate::evaluation::{Evaluator, ObservedInputs};

type SelectionKey = (AttachableKey, u32);

/// Common owner inputs prepared lazily for one equipment search, never retained across calls.
struct PreparedOwner {
    context: ActorTickContext,
    input: ActorTickInput,
    frame_alpha: f32,
}

/// Bounded predicate results whose observed owner inputs are still identical.
#[derive(Debug, Default)]
pub(super) struct SelectionCache {
    entries: BTreeMap<SelectionKey, (ObservedInputs, bool)>,
    weight: usize,
}

impl SelectionCache {
    /// Replaces an entry and evicts entries before retained reads exceed one world budget.
    fn insert(&mut self, key: SelectionKey, reads: ObservedInputs, result: bool) {
        if let Some((old, _)) = self.entries.remove(&key) {
            self.weight -= old.weight();
        }
        let weight = reads.weight();
        if weight > MAX_MOLANG_OPS_PER_WORLD_TICK {
            return;
        }
        while self.weight + weight > MAX_MOLANG_OPS_PER_WORLD_TICK {
            let Some((_, (old, _))) = self.entries.pop_first() else {
                break;
            };
            self.weight -= old.weight();
        }
        self.weight += weight;
        self.entries.insert(key, (reads, result));
    }
}

impl AttachablesRuntime {
    /// Tries owner-admitted item mappings before the ordinary definition within one budget.
    /// `drawable` rejects resources the caller cannot render without changing owner evaluation.
    #[allow(clippy::too_many_arguments)]
    pub fn select_equipment_binding(
        &mut self,
        catalog: &assets::RuntimeEquipmentCatalog,
        identifier: &str,
        owner: &dyn AnimatedActor,
        rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
        operations_left: &mut usize,
        drawable: impl Fn(&assets::EquipmentBinding) -> bool,
    ) -> Option<usize> {
        let start = catalog
            .bindings()
            .partition_point(|binding| binding.identifier.as_ref() < identifier);
        let candidates = catalog.bindings_for(identifier).iter().enumerate().rev();
        let mut prepared = None;
        candidates
            .clone()
            .filter(|(_, binding)| binding.attachable.is_some())
            .chain(candidates.filter(|(_, binding)| binding.attachable.is_none()))
            .find_map(|(offset, binding)| {
                let matches = self
                    .matches_prepared_owner(
                        binding.attachable_identifier(),
                        owner,
                        rig,
                        input,
                        operations_left,
                        &mut prepared,
                    )
                    .unwrap_or(binding.attachable.is_none());
                (matches && drawable(binding)).then_some(start + offset)
            })
    }

    /// Tests an owner condition within the caller's shared operation budget; absent rigs stay unresolved.
    pub fn matches_owner(
        &mut self,
        identifier: &str,
        owner: &dyn AnimatedActor,
        rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
        operations_left: &mut usize,
    ) -> Option<bool> {
        self.matches_prepared_owner(identifier, owner, rig, input, operations_left, &mut None)
    }

    /// Shares the owner prefix while each predicate restores its own writes before returning.
    #[allow(clippy::too_many_arguments)]
    fn matches_prepared_owner(
        &mut self,
        identifier: &str,
        owner: &dyn AnimatedActor,
        rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
        operations_left: &mut usize,
        prepared: &mut Option<PreparedOwner>,
    ) -> Option<bool> {
        let binding = self.assets.attachable_rig_binding(identifier)?;
        let Some(condition) = self.assets.rig_bindings()[binding].item_condition else {
            return Some(true);
        };
        if *operations_left == 0 {
            return Some(false);
        }
        let prepared = prepared.get_or_insert_with(|| self.prepare_owner(owner, rig, input));
        let evaluator = Evaluator {
            assets: &self.assets,
            pack: true,
            layout: &self.layout,
            program: None,
            actor: owner,
            input: &prepared.input,
            context: &prepared.context,
            anim_tick: rig.animation_variables.anim_tick(),
            anim_time: None,
            swell_amount: None,
            presentation_alpha: Some(prepared.frame_alpha),
            query_history: None,
            life_tick: rig.animation_variables.life_tick(),
            finished: (false, false),
            state_time: 0.0,
            bones: &[],
            bone_names: &[],
        };
        let key = (
            (
                rig.actor,
                input.off_hand,
                input.first_person,
                input.worn,
                input.worn_slot,
            ),
            condition,
        );
        if self
            .selections
            .entries
            .first_key_value()
            .is_some_and(|((actor, _), _)| actor.0.session_id != rig.actor.session_id)
        {
            self.selections = SelectionCache::default();
        }
        if let Some((reads, result)) = self.selections.entries.get(&key) {
            match reads.matches(&evaluator, &self.selection_variables, operations_left) {
                Some(true) => return Some(*result),
                None => return Some(false),
                Some(false) => {}
            }
        }
        if *operations_left == 0 {
            return Some(false);
        }
        let mut reads = ObservedInputs::new(&self.selection_variables);
        let mut budget = EvalBudget {
            actor_left: *operations_left,
            world_left: operations_left,
            work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
            transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
            used: 0,
            stack: OperandStack(std::mem::take(&mut self.selection_stack)),
            static_draw: None,
        };
        let result = evaluator.run_observed(
            condition as usize,
            &mut self.selection_variables,
            0.0,
            &mut budget,
            Some(&mut reads),
        );
        self.selection_stack = std::mem::take(&mut budget.stack.0);
        reads.restore(&mut self.selection_variables);
        let matches = result.as_ref().is_ok_and(|value| value.truthy());
        if result.is_ok() {
            self.selections.insert(key, reads, matches);
        }
        Some(matches)
    }

    /// Resets the full layout and inherits immutable owner values once before conditional search.
    fn prepare_owner(
        &mut self,
        owner: &dyn AnimatedActor,
        rig: &ActorRigSnapshot<'_>,
        input: AttachableAnimationInput<'_>,
    ) -> PreparedOwner {
        let frame_alpha = sample_alpha(input);
        let context = owner_context(rig, input);
        let tick_input = owner_frame(owner, rig, input);
        self.selection_variables
            .reset(owner.runtime_id() ^ owner.spawn_revision().rotate_left(32));
        tick::apply_engine_variables(
            &self.layout.engine,
            &mut self.selection_variables,
            owner,
            &context,
            &tick_input,
            &motion::MotionState::default(),
        );
        rig.animation_variables
            .copy_to(&self.assets, &self.layout, &mut self.selection_variables);
        rig.animation_variables.sample_spear_to(
            &self.layout.engine.spear,
            &mut self.selection_variables,
            owner,
            &tick_input,
            frame_alpha,
        );
        for (name, value) in input.owner_variables {
            self.selection_variables
                .set(self.layout.named_slot(&self.assets, name), *value);
        }
        apply_context_variables(
            &self.layout.engine,
            &mut self.selection_variables,
            owner,
            &context,
            &tick_input,
        );
        PreparedOwner {
            input: tick::evaluation_input(tick_input, &context),
            context,
            frame_alpha,
        }
    }
}

/// Builds the equipment owner's complete query context for selection and animation.
pub(super) fn owner_context(
    owner_rig: &ActorRigSnapshot<'_>,
    input: AttachableAnimationInput<'_>,
) -> ActorTickContext {
    let frame_alpha = sample_alpha(input);
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
    let (main_hand_kinetic, main_hand_swing_seconds) = owner_rig.animation_variables.item_timings();
    let owner_context = owner_rig.animation_variables.owner_context();
    ActorTickContext {
        // Attachables use render time, independently of the owner's tick batching.
        animation_elapsed_ticks: None,
        main_hand_kinetic,
        main_hand_swing_seconds,
        main_hand_is_spear: owner_rig.animation_variables.is_spear(),
        is_local_first_person: input.first_person,
        is_in_ui: input.is_in_ui || owner_context.is_some_and(|owner| owner.is_in_ui),
        is_local_player: input.is_local_player
            || input.first_person
            || owner_context.is_some_and(|owner| owner.is_local_player),
        hand_charged: input.hand_charged,
        main_hand_max_use_ticks: input.max_use_ticks,
        attachable: Some(query_context),
        main_hand: input.owner_main_hand.map(Arc::from),
        off_hand: input.owner_off_hand.map(Arc::from),
        ..owner_context.cloned().unwrap_or_default()
    }
}

/// Samples the owner frame shared by predicates and attachable evaluation, including Java swing.
pub(super) fn owner_frame(
    owner: &dyn AnimatedActor,
    rig: &ActorRigSnapshot<'_>,
    input: AttachableAnimationInput<'_>,
) -> ActorTickInput {
    let alpha = sample_alpha(input);
    let item = rig.item_animation[0].interpolate(rig.item_animation[1], alpha);
    let swing = rig.item_animation[0].interpolate(
        rig.item_animation[1],
        rig.java.local_swing_alpha.unwrap_or(alpha),
    );
    let offhand = rig.off_hand_animation[0].interpolate(rig.off_hand_animation[1], alpha);
    ActorTickInput {
        position: owner.position(),
        velocity: owner.velocity(),
        on_ground: owner.on_ground(),
        body_yaw: rig.body_yaw,
        head_yaw: owner.head_yaw(),
        pitch: owner.pitch(),
        item_use_ticks: input.use_elapsed_ticks.unwrap_or(0),
        attack_time: swing.attack_time,
        arm_height: item.arm_height,
        off_hand_arm_height: offhand.arm_height,
        ..rig.animation_variables.input().unwrap_or_default()
    }
}

/// Normalizes the presentation fraction before every owner-frame consumer samples it.
pub(super) fn sample_alpha(input: AttachableAnimationInput<'_>) -> f32 {
    if input.frame_alpha.is_finite() {
        input.frame_alpha.clamp(0.0, 1.0)
    } else {
        0.0
    }
}
