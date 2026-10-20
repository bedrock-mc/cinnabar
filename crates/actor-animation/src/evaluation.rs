use assets::entity::EngineVariable as V;
use assets::{MAX_MOLANG_LOOP_DEPTH, MAX_MOLANG_LOOP_ITERATIONS, MolangSymbolKind, molang_call};

use {super::*, simulation_clock::TICK_DURATION as ACTOR_TICK_DURATION};

pub(crate) mod frame_cache;
mod input_reads;
mod observed;
#[cfg(test)]
pub(super) mod selection_work;
mod vm;
pub(super) use observed::ObservedInputs;
mod owner_variables;
mod presentation;
pub(super) mod variables;

#[cfg(test)]
mod invalid_input_tests;

#[cfg(test)]
thread_local! {
    static QUERY_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static EXPRESSION_WORK: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Takes bytecode evaluations by expression on the current test thread.
#[cfg(test)]
pub(super) fn take_expression_work() -> Vec<usize> {
    EXPRESSION_WORK.with(|work| std::mem::take(&mut *work.borrow_mut()))
}

/// Counts an expression whose inputs changed, including a direct single-read program.
#[cfg(test)]
pub(super) fn record_expression_work(index: usize) {
    EXPRESSION_WORK.with(|work| {
        let mut work = work.borrow_mut();
        let size = work.len().max(index + 1);
        work.resize(size, 0);
        work[index] += 1;
    });
}

/// Takes query evaluations performed on this test thread since its last observation.
#[cfg(test)]
pub(super) fn take_query_work() -> usize {
    QUERY_WORK.with(|work| work.replace(0))
}

/// Runtime values retain the attachable's owning actor reference without numeric coercion.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum MolangValue {
    Number(f32),
    String(Arc<str>),
    ActorReference(u64),
}

impl MolangValue {
    /// Numeric reading; a string has no numeric value and reads 0.0.
    pub(super) fn number(&self) -> f32 {
        match self {
            Self::Number(value) => *value,
            Self::String(_) | Self::ActorReference(_) => 0.0,
        }
    }

    /// Molang truthiness: non-zero numbers (NaN included) and strings are true.
    pub(super) fn truthy(&self) -> bool {
        match self {
            Self::Number(value) => *value != 0.0,
            Self::String(_) | Self::ActorReference(_) => true,
        }
    }
}

/// Dense variable and temporary slots of one carrier's Molang symbol table.
#[derive(Debug, Default)]
pub(super) struct VariableLayout {
    variable_base: usize,
    variable_count: usize,
    temp_base: usize,
    temp_count: usize,
    /// Shared copies of the symbol table's string literals, from `string_base` on.
    string_base: usize,
    strings: Box<[Arc<str>]>,
    clip_reads: Vec<Box<[u32]>>,
    clip_inputs: Vec<PresentationReads>,
    controller_inputs: Vec<PresentationReads>,
    presentation_symbols: Vec<PresentationReads>,
    expression_inputs: Vec<PresentationReads>,
    pub(super) engine: EngineSlots,
    /// Immutable analysis cached only for this finalized layout and its exact catalog.
    pub(super) plans: rig_plans::RigPlans,
}

/// Immutable camera and attack inputs used to admit scratch presentation evaluation.
#[derive(Clone, Copy, Default, Debug, Eq, PartialEq)]
pub(super) struct PresentationReads {
    pub camera: bool,
    pub swing: bool,
}

/// Slots of variables the client, not the pack, assigns.
#[derive(Debug, Default)]
pub(super) struct EngineSlots {
    /// Assigned once, before the pack's `initialize` script runs.
    pub(super) seeded: Vec<(usize, f32)>,
    pub(super) attack_time: Option<usize>,
    pub(super) gliding_speed_value: Option<usize>,
    pub(super) is_holding_right: Option<usize>,
    pub(super) is_holding_left: Option<usize>,
    pub(super) is_sneaking: Option<usize>,
    pub(super) chest_layer_visible: Option<usize>,
    pub(super) is_blocking: Option<usize>,
    pub(super) damage_nearby_mobs: Option<usize>,
    /// The eat/drink arm raise and bob the pack's `use_item_progress` pose reads.
    pub(super) use_item_startup_progress: Option<usize>,
    pub(super) use_item_interval_progress: Option<usize>,
    /// Refreshed per tick so the root controller tracks live perspective, not only its seed.
    pub(super) is_first_person: Option<usize>,
    pub(super) context_first_person: Option<usize>,
    pub(super) context_paperdoll: Option<usize>,
    pub(super) context_item_slot: Option<usize>,
    pub(super) context_owning_entity: Option<usize>,
    /// Look pitch feeding `variable.map_angle`; refreshed per tick, unlike the seed.
    pub(super) player_x_rotation: Option<usize>,
    /// View-bobbing gate the first-person walk/breathing animations weigh against.
    pub(super) bob_animation: Option<usize>,
    /// The first-person attack swing's factor, which the pinned pack reads but never assigns,
    /// and the pack's own older name for it, which its `pre_animation` still computes.
    pub(super) first_person_item_rotation_factor: Option<usize>,
    pub(super) first_person_rotation_factor: Option<usize>,
    /// Equip progress that lowers the first-person arm while the held item swaps.
    pub(super) player_arm_height: Option<usize>,
    pub(super) context_player_offhand_arm_height: Option<usize>,
    pub(super) swim_amount: Option<usize>,
    pub(super) left_arm_swim_amount: Option<usize>,
    pub(super) right_arm_swim_amount: Option<usize>,
    pub(super) has_target: Option<usize>,
    pub(super) fish_animation_amount: Option<usize>,
    pub(super) fish_animation_amount_previous: Option<usize>,
    pub(super) tropical_fish_base: Option<usize>,
    pub(super) tropical_fish_pattern: Option<usize>,
    pub(super) horse_stand_anim: Option<usize>,
    pub(super) horse_shake_tail: Option<usize>,
    pub(super) horse_open_mouth: Option<usize>,
    /// Dense slots for each requested dragon historical frame's yaw or height.
    pub(super) dragon_history: Vec<(usize, usize, usize)>,
    pub(super) spear: super::spear::Slots,
}

// Client-owned variables seeded on construction and needing independent measurement; remote third-person actors keep these values because
// only first-person, HUD, and paper-doll renderers change them.
const SEEDED_VARIABLES: [(&str, f32); 20] = [
    (V::AnimationFrames128x128.name(), 1.0),
    (V::AnimationFrames32x32.name(), 1.0),
    (V::AnimationFramesFace.name(), 1.0),
    (V::AttackTime.name(), 0.0),
    (V::ChargeAmount.name(), 0.0),
    (V::GlidingSpeedValue.name(), 1.0),
    (V::HasTarget.name(), 0.0),
    (V::IsFirstPerson.name(), 0.0),
    (V::IsHorizontalSplitscreen.name(), 0.0),
    (V::IsPaperdoll.name(), 0.0),
    (V::IsUsingVr.name(), 0.0),
    (V::IsVerticalSplitscreen.name(), 0.0),
    (V::LeftArmSwimAmount.name(), 0.0),
    (V::MapFaceIcon.name(), 0.0),
    (V::PlayerXRotation.name(), 0.0),
    (V::RightArmSwimAmount.name(), 0.0),
    (V::ShortArmOffsetLeft.name(), 0.0),
    (V::ShortArmOffsetRight.name(), 0.0),
    (V::SwimAmount.name(), 0.0),
    (V::UseBlinkingAnimation.name(), 0.0),
];

impl VariableLayout {
    /// Resolves a named engine variable in this carrier's symbol table.
    pub(super) fn slot(&self, assets: &RuntimeEntityAssets, name: &str) -> Option<usize> {
        assets.molang_symbols()[self.variable_base..self.variable_base + self.variable_count]
            .binary_search_by(|symbol| symbol.identifier.as_ref().cmp(name))
            .ok()
    }

    /// Finalizes a catalog layout and binds its cache to that exact owning allocation.
    pub(super) fn new(assets: &Arc<RuntimeEntityAssets>) -> Self {
        let mut layout = Self::from_symbols(assets.molang_symbols());
        layout.bind_input_reads(assets);
        layout.plans = rig_plans::RigPlans::new(assets);
        layout
    }

    pub(super) fn from_symbols(symbols: &[assets::MolangSymbol]) -> Self {
        let range = |kind: MolangSymbolKind| {
            let start = symbols.partition_point(|symbol| symbol.kind < kind);
            let end = symbols.partition_point(|symbol| symbol.kind <= kind);
            (start, end - start)
        };
        let (variable_base, variable_count) = range(MolangSymbolKind::Variable);
        let (temp_base, temp_count) = range(MolangSymbolKind::Temporary);
        let (string_base, string_count) = range(MolangSymbolKind::String);
        let slot = |name: &str| {
            symbols[variable_base..variable_base + variable_count]
                .binary_search_by(|symbol| symbol.identifier.as_ref().cmp(name))
                .ok()
        };
        Self {
            plans: Default::default(),
            variable_base,
            variable_count,
            temp_base,
            temp_count,
            string_base,
            strings: symbols[string_base..string_base + string_count]
                .iter()
                .map(|symbol| Arc::from(symbol.identifier.as_ref()))
                .collect(),
            clip_reads: Vec::new(),
            clip_inputs: Vec::new(),
            controller_inputs: Vec::new(),
            presentation_symbols: Vec::new(),
            expression_inputs: Vec::new(),
            engine: EngineSlots {
                seeded: SEEDED_VARIABLES
                    .iter()
                    .filter_map(|(name, value)| slot(name).map(|slot| (slot, *value)))
                    .collect(),
                attack_time: slot(V::AttackTime.name()),
                spear: super::spear::Slots::new(slot),
                gliding_speed_value: slot(V::GlidingSpeedValue.name()),
                is_holding_right: slot(V::IsHoldingRight.name()),
                is_holding_left: slot(V::IsHoldingLeft.name()),
                is_sneaking: slot(V::IsSneaking.name()),
                chest_layer_visible: slot(V::ChestLayerVisible.name()),
                is_blocking: slot(V::IsBlocking.name()),
                damage_nearby_mobs: slot(V::DamageNearbyMobs.name()),
                use_item_startup_progress: slot(V::UseItemStartupProgress.name()),
                use_item_interval_progress: slot(V::UseItemIntervalProgress.name()),
                is_first_person: slot(V::IsFirstPerson.name()),
                context_first_person: slot("context.is_first_person"),
                context_paperdoll: slot("context.is_paperdoll"),
                context_item_slot: slot("context.item_slot"),
                context_owning_entity: slot("context.owning_entity"),
                player_x_rotation: slot(V::PlayerXRotation.name()),
                bob_animation: slot(V::BobAnimation.name()),
                first_person_item_rotation_factor: slot(V::FirstPersonItemRotationFactor.name()),
                first_person_rotation_factor: slot(V::FirstPersonRotationFactor.name()),
                player_arm_height: slot(V::PlayerArmHeight.name()),
                context_player_offhand_arm_height: slot("context.player_offhand_arm_height"),
                swim_amount: slot(V::SwimAmount.name()),
                left_arm_swim_amount: slot(V::LeftArmSwimAmount.name()),
                right_arm_swim_amount: slot(V::RightArmSwimAmount.name()),
                has_target: slot(V::HasTarget.name()),
                fish_animation_amount: slot(V::Animationamount.name()),
                fish_animation_amount_previous: slot(V::Animationamountprev.name()),
                tropical_fish_base: slot(V::TropicalfishBase.name()),
                tropical_fish_pattern: slot(V::TropicalfishPattern.name()),
                horse_stand_anim: slot(V::StandAnim.name()),
                horse_shake_tail: slot(V::ShakeTail.name()),
                horse_open_mouth: slot(V::OpenMouth.name()),
                dragon_history: symbols[variable_base..variable_base + variable_count]
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, symbol)| {
                        let name = symbol
                            .identifier
                            .strip_prefix("variable.historical_frame_")?;
                        let (offset, component) = name.split_once('.')?;
                        let offset = offset.parse::<usize>().ok()?;
                        let axis = match component {
                            "rot_y" => 0,
                            "pos_y" => 1,
                            _ => return None,
                        };
                        Some((slot, offset, axis))
                    })
                    .collect(),
            },
        }
    }

    pub(super) fn fresh(&self, seed: u64) -> MolangVariables {
        MolangVariables::fresh(self, seed)
    }

    pub(super) fn named_slot(&self, assets: &RuntimeEntityAssets, name: &str) -> Option<usize> {
        assets.molang_symbols()[self.variable_base..self.variable_base + self.variable_count]
            .binary_search_by(|symbol| symbol.identifier.as_ref().cmp(name))
            .ok()
    }

    /// The shared copy of string literal `symbol` when it reads `identifier` in this layout's
    /// symbol table; `None` for any other symbol, so callers allocate their own copy.
    fn interned_string(&self, symbol: u32, identifier: &str) -> Option<&Arc<str>> {
        (symbol as usize)
            .checked_sub(self.string_base)
            .and_then(|index| self.strings.get(index))
            .filter(|interned| ***interned == *identifier)
    }
}

/// Completed owner query clocks and borrowed script values for equipment and particles.
#[derive(Clone, Copy, Debug, Default)]
pub struct ActorAnimationVariables<'a> {
    assets: Option<&'a RuntimeEntityAssets>,
    variables: Option<&'a MolangVariables>,
    life_tick: u64,
    anim_tick: u64,
    input: Option<ActorTickInput>,
    item_context: Option<&'a ActorTickContext>,
    complete_spear: bool,
}

#[derive(Clone, Copy, Debug)]
enum Place {
    Variable(usize),
    Temporary(usize),
}

/// Completed controller effects retain authored slots and their random draw span.
#[derive(Clone, Debug, Default)]
pub(super) struct MolangEffects {
    writes: Vec<(Place, Option<MolangValue>)>,
    random_draws: u64,
    random_start: u64,
    random_end: u64,
}

pub(super) struct EffectsCapture {
    enabled: bool,
    writes: Vec<u64>,
    random_draws: u64,
    random: u64,
}

impl MolangEffects {
    pub(super) fn is_empty(&self) -> bool {
        self.writes.is_empty() && self.random_draws == 0
    }

    pub(super) fn apply(&self, variables: &mut MolangVariables) -> Result<(), EvalError> {
        for (place, value) in &self.writes {
            if let Some(value) = value {
                variables.store(*place, value.clone())?;
            } else {
                variables.unset(*place)?;
            }
        }
        variables.advance_random(self.random_start, self.random_end, self.random_draws);
        Ok(())
    }
}

impl VariableLayout {
    /// Reads an inherited persistent input; expression temporaries start unassigned.
    pub(crate) fn input_variable(
        &self,
        variables: &MolangVariables,
        symbol: u32,
    ) -> Option<MolangValue> {
        match self.place(symbol) {
            Some(Place::Variable(slot)) => variables.variable(slot),
            _ => None,
        }
    }

    fn place(&self, symbol: u32) -> Option<Place> {
        let symbol = symbol as usize;
        if let Some(slot) = symbol.checked_sub(self.variable_base)
            && slot < self.variable_count
        {
            return Some(Place::Variable(slot));
        }
        let slot = symbol.checked_sub(self.temp_base)?;
        (slot < self.temp_count).then_some(Place::Temporary(slot))
    }
}

/// Read-only inputs shared by every expression an actor evaluates in one tick.
#[derive(Clone, Copy)]
pub(super) struct Evaluator<'a> {
    pub(super) assets: &'a RuntimeEntityAssets,
    /// Session-pack animations keep their authored attack channels in every pose pass.
    pub(super) pack: bool,
    pub(super) layout: &'a VariableLayout,
    pub(super) program: Option<&'a assets::MolangProgram>,
    pub(super) actor: &'a dyn AnimatedActor,
    pub(super) input: &'a ActorTickInput,
    pub(super) context: &'a ActorTickContext,
    pub(super) anim_tick: u64,
    /// The clip clock while its time expression or bone channels are evaluated.
    pub(super) anim_time: Option<f32>,
    pub(super) swell_amount: Option<f32>,
    /// The presentation fraction is independent of retained ordinary query inputs.
    pub(super) presentation_alpha: Option<f32>,
    pub(super) query_history: Option<&'a [(u32, MolangValue)]>,
    pub(super) life_tick: u64,
    /// Whether all and any animations of the controller state being left have finished.
    pub(super) finished: (bool, bool),
    /// Seconds since the controller state being evaluated was entered.
    pub(super) state_time: f32,
    /// The posed skeleton's bones and lowercase names, for bone queries.
    pub(super) bones: &'a [RuntimeBone],
    pub(super) bone_names: &'a [Box<str>],
}

impl Evaluator<'_> {
    /// Sets the elapsed time seen by expressions in one controller state.
    pub(super) fn for_controller_state(self, entered_tick: u64) -> Self {
        Self {
            state_time: self.anim_tick.saturating_sub(entered_tick) as f32
                * ACTOR_TICK_DURATION.as_secs_f32(),
            ..self
        }
    }

    fn expressions(&self) -> &[assets::CompiledMolangExpression] {
        self.program.map_or_else(
            || self.assets.molang_expressions(),
            |program| &program.expressions,
        )
    }
    fn ops(&self) -> &[MolangOp] {
        self.program
            .map_or_else(|| self.assets.molang_ops(), |program| &program.ops)
    }
    fn symbols(&self) -> &[assets::MolangSymbol] {
        self.program
            .map_or_else(|| self.assets.molang_symbols(), |program| program.symbols())
    }
    fn collections(&self) -> &[assets::MolangCollection] {
        self.program.map_or_else(
            || self.assets.molang_collections(),
            |program| &program.collections,
        )
    }
    fn collection_items(&self) -> &[assets::MolangCollectionItem] {
        self.program.map_or_else(
            || self.assets.molang_collection_items(),
            |program| &program.collection_items,
        )
    }
    pub(super) fn number(
        &self,
        expression: usize,
        variables: &mut MolangVariables,
        this: f32,
        budget: &mut EvalBudget<'_>,
    ) -> Result<f32, EvalError> {
        Ok(self.run(expression, variables, this, budget)?.number())
    }

    /// Evaluates one compiled program; `this` is the channel value earlier animations built.
    pub(super) fn run(
        &self,
        expression_index: usize,
        variables: &mut MolangVariables,
        this: f32,
        budget: &mut EvalBudget<'_>,
    ) -> Result<MolangValue, EvalError> {
        if self.program.is_none()
            && crate::render_frame::cache::enabled()
            && let Some(mut cache) = variables
                .frame
                .as_mut()
                .and_then(|frame| frame.cache.take())
        {
            let result = cache.run(self, expression_index, variables, this, budget);
            variables.frame.as_mut().unwrap().cache = Some(cache);
            result
        } else {
            self.run_observed(expression_index, variables, this, budget, None)
        }
    }

    /// Records selection reads when given a recorder; its caller supplies clean temporaries and
    /// restores recorded writes after every result. Ordinary evaluations clear their own temps.
    pub(super) fn run_observed(
        &self,
        expression_index: usize,
        variables: &mut MolangVariables,
        this: f32,
        budget: &mut EvalBudget<'_>,
        reads: Option<&mut ObservedInputs>,
    ) -> Result<MolangValue, EvalError> {
        #[cfg(test)]
        record_expression_work(expression_index);
        let mut stack = std::mem::take(&mut budget.stack.0);
        stack.clear();
        let result = self.run_on(&mut stack, expression_index, variables, this, budget, reads);
        budget.stack.0 = stack;
        result
    }

    /// The literal's value, sharing the layout's interned copy instead of allocating one.
    fn string(&self, symbol: u32) -> Result<Arc<str>, EvalError> {
        let identifier = self
            .symbols()
            .get(symbol as usize)
            .filter(|symbol| symbol.kind == MolangSymbolKind::String)
            .map(|symbol| symbol.identifier.as_ref())
            .ok_or(EvalError::Invalid)?;
        Ok(self
            .layout
            .interned_string(symbol, identifier)
            .map_or_else(|| Arc::from(identifier), Arc::clone))
    }

    pub(super) fn query(&self, symbol: u32, arguments: &[MolangValue]) -> MolangValue {
        #[cfg(test)]
        QUERY_WORK.with(|work| work.set(work.get() + 1));
        let binding = self.program.map_or_else(
            || self.assets.molang_query_binding(symbol),
            |program| program.query_binding(symbol),
        );
        if let Some(alpha) = self.presentation_alpha
            && binding == Some(assets::MolangQuery::FrameAlpha)
        {
            return MolangValue::Number(alpha);
        }
        if self.program.is_none()
            && arguments.is_empty()
            && let Some(history) = self.query_history
            && let Ok(slot) = history.binary_search_by_key(&symbol, |(symbol, _)| *symbol)
        {
            return history[slot].1.clone();
        }
        let Some(binding) = binding else {
            return MolangValue::Number(0.0);
        };
        let inputs = query::QueryInputs {
            actor: self.actor,
            input: self.input,
            context: self.context,
            anim_tick: self.anim_tick,
            anim_time: self.anim_time,
            swell_amount: self.swell_amount,
            life_tick: self.life_tick,
            finished: self.finished,
            state_time: self.state_time,
            bones: self.bones,
            bone_names: self.bone_names,
        };
        query::query(&inputs, binding, arguments)
    }

    /// Selects a collection item; indices wrap past the end and clamp below zero.
    fn collection(&self, collection: u32, index: f32) -> Result<f32, EvalError> {
        let collection = self
            .collections()
            .get(collection as usize)
            .ok_or(EvalError::Invalid)?;
        let count = usize::from(collection.item_count);
        if count == 0 {
            return Err(EvalError::Invalid);
        }
        let index = if index.is_nan() || index <= 0.0 {
            0
        } else {
            (index as usize) % count
        };
        self.collection_items()
            .get(collection.first_item as usize + index)
            .map(|item| item.value.get())
            .ok_or(EvalError::Invalid)
    }
}

/// Iterations a `loop` count runs: its ceiling, bounded; `None` skips the body.
pub(super) fn loop_iterations(count: f32) -> Option<u32> {
    (count > 0.0).then(|| count.ceil().min(MAX_MOLANG_LOOP_ITERATIONS as f32) as u32)
}

fn arithmetic(op: &MolangOp, left: f32, right: f32) -> f32 {
    let truth = |value: bool| if value { 1.0 } else { 0.0 };
    match op {
        MolangOp::Add => left + right,
        MolangOp::Subtract => left - right,
        MolangOp::Multiply => left * right,
        // A divisor within float epsilon of zero yields zero; the threshold needs independent
        // measurement.
        MolangOp::Divide if right.abs() < f32::EPSILON => 0.0,
        MolangOp::Divide => left / right,
        MolangOp::Less => truth(left < right),
        MolangOp::LessEqual => truth(left <= right),
        MolangOp::Greater => truth(left > right),
        _ => truth(left >= right),
    }
}

pub(super) fn pop(stack: &mut Vec<MolangValue>) -> Result<MolangValue, EvalError> {
    stack.pop().ok_or(EvalError::Invalid)
}

fn bool_value(value: bool) -> MolangValue {
    MolangValue::Number(if value { 1.0 } else { 0.0 })
}
