use assets::{MAX_MOLANG_LOOP_DEPTH, MAX_MOLANG_LOOP_ITERATIONS, MolangSymbolKind, molang_call};

use super::*;

/// A Molang runtime value. Actor references and arrays have no producer in this client, so
/// `->` and `for_each` always take their empty path.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum MolangValue {
    Number(f32),
    String(Arc<str>),
}

impl MolangValue {
    /// Numeric reading; a string has no numeric value and reads 0.0.
    pub(super) fn number(&self) -> f32 {
        match self {
            Self::Number(value) => *value,
            Self::String(_) => 0.0,
        }
    }

    /// Molang truthiness: non-zero numbers (NaN included) and strings are true.
    pub(super) fn truthy(&self) -> bool {
        match self {
            Self::Number(value) => *value != 0.0,
            Self::String(_) => true,
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
    pub(super) engine: EngineSlots,
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
    /// Refreshed per tick so the root controller tracks live perspective, not only its seed.
    pub(super) is_first_person: Option<usize>,
    pub(super) context_first_person: Option<usize>,
    pub(super) context_paperdoll: Option<usize>,
    pub(super) context_item_slot: Option<usize>,
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
}

// Client-owned variables seeded on construction and needing independent measurement; remote third-person actors keep these values because
// only first-person, HUD, and paper-doll renderers change them.
const SEEDED_VARIABLES: [(&str, f32); 20] = [
    ("variable.animation_frames_128x128", 1.0),
    ("variable.animation_frames_32x32", 1.0),
    ("variable.animation_frames_face", 1.0),
    ("variable.attack_time", 0.0),
    ("variable.charge_amount", 0.0),
    ("variable.gliding_speed_value", 1.0),
    ("variable.has_target", 0.0),
    ("variable.is_first_person", 0.0),
    ("variable.is_horizontal_splitscreen", 0.0),
    ("variable.is_paperdoll", 0.0),
    ("variable.is_using_vr", 0.0),
    ("variable.is_vertical_splitscreen", 0.0),
    ("variable.left_arm_swim_amount", 0.0),
    ("variable.map_face_icon", 0.0),
    ("variable.player_x_rotation", 0.0),
    ("variable.right_arm_swim_amount", 0.0),
    ("variable.short_arm_offset_left", 0.0),
    ("variable.short_arm_offset_right", 0.0),
    ("variable.swim_amount", 0.0),
    ("variable.use_blinking_animation", 0.0),
];

impl VariableLayout {
    /// Resolves a named engine variable in this carrier's symbol table.
    pub(super) fn slot(&self, assets: &RuntimeEntityAssets, name: &str) -> Option<usize> {
        assets.molang_symbols()[self.variable_base..self.variable_base + self.variable_count]
            .binary_search_by(|symbol| symbol.identifier.as_ref().cmp(name))
            .ok()
    }

    pub(super) fn new(assets: &RuntimeEntityAssets) -> Self {
        let symbols = assets.molang_symbols();
        let range = |kind: MolangSymbolKind| {
            let start = symbols.partition_point(|symbol| symbol.kind < kind);
            let end = symbols.partition_point(|symbol| symbol.kind <= kind);
            (start, end - start)
        };
        let (variable_base, variable_count) = range(MolangSymbolKind::Variable);
        let (temp_base, temp_count) = range(MolangSymbolKind::Temporary);
        let slot = |name: &str| {
            symbols[variable_base..variable_base + variable_count]
                .binary_search_by(|symbol| symbol.identifier.as_ref().cmp(name))
                .ok()
        };
        Self {
            variable_base,
            variable_count,
            temp_base,
            temp_count,
            engine: EngineSlots {
                seeded: SEEDED_VARIABLES
                    .iter()
                    .filter_map(|(name, value)| slot(name).map(|slot| (slot, *value)))
                    .collect(),
                attack_time: slot("variable.attack_time"),
                gliding_speed_value: slot("variable.gliding_speed_value"),
                is_holding_right: slot("variable.is_holding_right"),
                is_holding_left: slot("variable.is_holding_left"),
                is_sneaking: slot("variable.is_sneaking"),
                chest_layer_visible: slot("variable.chest_layer_visible"),
                is_blocking: slot("variable.is_blocking"),
                damage_nearby_mobs: slot("variable.damage_nearby_mobs"),
                is_first_person: slot("variable.is_first_person"),
                context_first_person: slot("context.is_first_person"),
                context_paperdoll: slot("context.is_paperdoll"),
                context_item_slot: slot("context.item_slot"),
                player_x_rotation: slot("variable.player_x_rotation"),
                bob_animation: slot("variable.bob_animation"),
                first_person_item_rotation_factor: slot(
                    "variable.first_person_item_rotation_factor",
                ),
                first_person_rotation_factor: slot("variable.first_person_rotation_factor"),
                player_arm_height: slot("variable.player_arm_height"),
                context_player_offhand_arm_height: slot("context.player_offhand_arm_height"),
                swim_amount: slot("variable.swim_amount"),
                left_arm_swim_amount: slot("variable.left_arm_swim_amount"),
                right_arm_swim_amount: slot("variable.right_arm_swim_amount"),
                has_target: slot("variable.has_target"),
                fish_animation_amount: slot("variable.animationamount"),
                fish_animation_amount_previous: slot("variable.animationamountprev"),
                tropical_fish_base: slot("variable.tropicalfish.base"),
                tropical_fish_pattern: slot("variable.tropicalfish.pattern"),
                horse_stand_anim: slot("variable.stand_anim"),
                horse_shake_tail: slot("variable.shake_tail"),
                horse_open_mouth: slot("variable.open_mouth"),
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
                        (offset < crate::actor_store::dragon_animation::HISTORICAL_VARIABLES)
                            .then_some((slot, offset, axis))
                    })
                    .collect(),
            },
        }
    }

    pub(super) fn fresh(&self, seed: u64) -> MolangVariables {
        MolangVariables {
            values: vec![None; self.variable_count],
            temps: vec![None; self.temp_count],
            random: seed | 1,
        }
    }

    pub(super) fn named_slot(&self, assets: &RuntimeEntityAssets, name: &str) -> Option<usize> {
        assets.molang_symbols()[self.variable_base..self.variable_base + self.variable_count]
            .binary_search_by(|symbol| symbol.identifier.as_ref().cmp(name))
            .ok()
    }
}

/// One actor's Molang variables and random stream; an unassigned variable reads as 0.0.
#[derive(Clone, Debug, Default)]
pub(super) struct MolangVariables {
    values: Vec<Option<MolangValue>>,
    temps: Vec<Option<MolangValue>>,
    random: u64,
}

/// Borrowed owner script values, copied by name when an item uses another asset catalog.
#[derive(Clone, Copy, Debug, Default)]
pub struct ActorAnimationVariables<'a> {
    assets: Option<&'a RuntimeEntityAssets>,
    variables: Option<&'a MolangVariables>,
    life_tick: u64,
    input: Option<ActorTickInput>,
}

impl<'a> ActorAnimationVariables<'a> {
    pub(super) fn new(
        assets: Option<&'a RuntimeEntityAssets>,
        variables: &'a MolangVariables,
        life_tick: u64,
    ) -> Self {
        Self {
            assets,
            variables: Some(variables),
            life_tick,
            input: None,
        }
    }

    /// Equipment observes the same fixed-tick movement and swim blend as its owner.
    pub(super) fn with_input(mut self, input: Option<ActorTickInput>) -> Self {
        self.input = input;
        self
    }

    /// Retains query inputs as well as script variables for worn animation clips.
    pub(super) fn input(self) -> Option<ActorTickInput> {
        self.input
    }

    pub(super) fn life_tick(self) -> u64 {
        self.life_tick
    }

    /// The catalog that owns these retained rig script values.
    pub(super) fn asset_catalog(self) -> Option<&'a RuntimeEntityAssets> {
        self.assets
    }

    pub(super) fn copy_to(
        self,
        assets: &RuntimeEntityAssets,
        layout: &VariableLayout,
        output: &mut MolangVariables,
    ) {
        let (Some(owner_assets), Some(variables)) = (self.assets, self.variables) else {
            return;
        };
        let owner_symbols = owner_assets.molang_symbols();
        let first =
            owner_symbols.partition_point(|symbol| symbol.kind < MolangSymbolKind::Variable);
        for (offset, value) in variables.values.iter().enumerate() {
            let Some(value) = value else {
                continue;
            };
            let Some(symbol) = owner_symbols.get(first + offset) else {
                break;
            };
            if let Some(slot) = layout.named_slot(assets, &symbol.identifier) {
                output.values[slot] = Some(value.clone());
            }
        }
    }
}

enum Place {
    Variable(usize),
    Temporary(usize),
}

impl MolangVariables {
    pub(super) fn set_string(&mut self, slot: Option<usize>, value: &str) {
        if let Some(entry) = slot.and_then(|slot| self.values.get_mut(slot)) {
            *entry = Some(MolangValue::String(Arc::from(value)));
        }
    }

    pub(super) fn set(&mut self, slot: Option<usize>, value: f32) {
        if let Some(entry) = slot.and_then(|slot| self.values.get_mut(slot)) {
            *entry = Some(MolangValue::Number(value));
        }
    }

    pub(super) fn clear(&mut self, slot: Option<usize>) {
        if let Some(entry) = slot.and_then(|slot| self.values.get_mut(slot)) {
            *entry = None;
        }
    }

    pub(super) fn get(&self, slot: Option<usize>) -> Option<f32> {
        self.values
            .get(slot?)
            .and_then(Option::as_ref)
            .map(MolangValue::number)
    }

    pub(super) fn clear_temporaries(&mut self) {
        self.temps.fill(None);
    }

    fn entry(&mut self, place: Place) -> Option<&mut Option<MolangValue>> {
        match place {
            Place::Variable(slot) => self.values.get_mut(slot),
            Place::Temporary(slot) => self.temps.get_mut(slot),
        }
    }

    #[cfg(test)]
    pub(super) fn slots(count: usize) -> Self {
        Self {
            values: vec![None; count],
            temps: Vec::new(),
            random: 1,
        }
    }

    #[cfg(test)]
    pub(super) fn number_at(&self, slot: usize) -> Option<f32> {
        self.values
            .get(slot)
            .and_then(|entry| entry.clone())
            .map(|value| value.number())
    }

    /// Next value in `[0, 1]` from a per-actor xorshift stream.
    fn next_random(&mut self) -> f32 {
        let mut state = self.random;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.random = state;
        (state >> 40) as f32 / (1_u64 << 24) as f32
    }
}

impl VariableLayout {
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
    pub(super) layout: &'a VariableLayout,
    pub(super) actor: &'a ActorSnapshot,
    pub(super) input: &'a ActorTickInput,
    pub(super) context: &'a ActorTickContext,
    pub(super) anim_tick: u64,
    /// The clip clock while its time expression or bone channels are evaluated.
    pub(super) anim_time: Option<f32>,
    pub(super) life_tick: u64,
    /// Whether all and any animations of the controller state being left have finished.
    pub(super) finished: (bool, bool),
    /// The posed skeleton's bones and lowercase names, for bone queries.
    pub(super) bones: &'a [RuntimeBone],
    pub(super) bone_names: &'a [Box<str>],
}

impl Evaluator<'_> {
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
        let mut stack = std::mem::take(&mut budget.stack);
        stack.clear();
        let result = self.run_on(&mut stack, expression_index, variables, this, budget);
        budget.stack = stack;
        result
    }

    fn run_on(
        &self,
        stack: &mut Vec<MolangValue>,
        expression_index: usize,
        variables: &mut MolangVariables,
        this: f32,
        budget: &mut EvalBudget<'_>,
    ) -> Result<MolangValue, EvalError> {
        let expression = self
            .assets
            .molang_expressions()
            .get(expression_index)
            .ok_or(EvalError::Invalid)?;
        let first = expression.first_op as usize;
        let end = first
            .checked_add(expression.op_count as usize)
            .ok_or(EvalError::Invalid)?;
        let ops = self
            .assets
            .molang_ops()
            .get(first..end)
            .ok_or(EvalError::Invalid)?;
        // Temporaries last for one evaluation.
        variables.clear_temporaries();
        stack.reserve(expression.max_stack as usize);
        let mut loops = Vec::new();
        let mut pc = 0;
        while let Some(op) = ops.get(pc) {
            budget.charge()?;
            pc += 1;
            let jump = |target: u16| -> Result<usize, EvalError> {
                let target = target as usize;
                (target <= ops.len())
                    .then_some(target)
                    .ok_or(EvalError::Invalid)
            };
            match *op {
                MolangOp::Push(value) => stack.push(MolangValue::Number(value.get())),
                MolangOp::PushString(symbol) => {
                    stack.push(MolangValue::String(self.string(symbol)?));
                }
                MolangOp::LoadThis => stack.push(MolangValue::Number(this)),
                MolangOp::LoadQuery(symbol) => {
                    stack.push(self.query(symbol, &[]));
                }
                MolangOp::CallQuery(call) => {
                    let start = stack
                        .len()
                        .checked_sub(call.arguments as usize)
                        .ok_or(EvalError::Invalid)?;
                    let value = self.query(call.symbol, &stack[start..]);
                    stack.truncate(start);
                    stack.push(value);
                }
                MolangOp::LoadVariable(symbol) => {
                    let place = self.layout.place(symbol).ok_or(EvalError::Invalid)?;
                    let value = variables
                        .entry(place)
                        .and_then(|entry| entry.clone())
                        .unwrap_or(MolangValue::Number(0.0));
                    stack.push(value);
                }
                MolangOp::StoreVariable(symbol) => {
                    let value = pop(stack)?;
                    let place = self.layout.place(symbol).ok_or(EvalError::Invalid)?;
                    *variables.entry(place).ok_or(EvalError::Invalid)? = Some(value);
                }
                MolangOp::Coalesce(branch) => {
                    let place = self.layout.place(branch.symbol).ok_or(EvalError::Invalid)?;
                    if let Some(value) = variables.entry(place).and_then(|entry| entry.clone()) {
                        stack.push(value);
                        pc = jump(branch.target)?;
                    }
                }
                MolangOp::SelectCollection(collection) => {
                    let index = pop(stack)?.number();
                    stack.push(MolangValue::Number(self.collection(collection, index)?));
                }
                MolangOp::Pop => {
                    pop(stack)?;
                }
                MolangOp::Negate => {
                    let value = pop(stack)?.number();
                    stack.push(MolangValue::Number(-value));
                }
                MolangOp::Not => {
                    let value = match pop(stack)? {
                        MolangValue::Number(value) => value == 0.0,
                        MolangValue::String(_) => false,
                    };
                    stack.push(bool_value(value));
                }
                MolangOp::Truthy => {
                    let value = pop(stack)?.truthy();
                    stack.push(bool_value(value));
                }
                MolangOp::Equal | MolangOp::NotEqual => {
                    let right = pop(stack)?;
                    let left = pop(stack)?;
                    let equal = match (&left, &right) {
                        (MolangValue::Number(left), MolangValue::Number(right)) => left == right,
                        (MolangValue::String(left), MolangValue::String(right)) => left == right,
                        _ => false,
                    };
                    stack.push(bool_value(equal == matches!(op, MolangOp::Equal)));
                }
                MolangOp::Add
                | MolangOp::Subtract
                | MolangOp::Multiply
                | MolangOp::Divide
                | MolangOp::Less
                | MolangOp::LessEqual
                | MolangOp::Greater
                | MolangOp::GreaterEqual => {
                    let right = pop(stack)?.number();
                    let left = pop(stack)?.number();
                    stack.push(MolangValue::Number(arithmetic(op, left, right)));
                }
                MolangOp::Call(function) => {
                    let start = stack
                        .len()
                        .checked_sub(function.arity())
                        .ok_or(EvalError::Invalid)?;
                    let mut arguments = [0.0; 3];
                    let arguments = arguments
                        .get_mut(..function.arity())
                        .ok_or(EvalError::Invalid)?;
                    for (argument, value) in arguments.iter_mut().zip(&stack[start..]) {
                        *argument = value.number();
                    }
                    stack.truncate(start);
                    let value = molang_call(function, arguments, &mut || variables.next_random());
                    stack.push(MolangValue::Number(value));
                }
                MolangOp::Jump(target) => pc = jump(target)?,
                MolangOp::JumpIfFalse(target) => {
                    if !pop(stack)?.truthy() {
                        pc = jump(target)?;
                    }
                }
                MolangOp::JumpIfTrue(target) => {
                    if pop(stack)?.truthy() {
                        pc = jump(target)?;
                    }
                }
                MolangOp::Return => return pop(stack),
                MolangOp::LoopStart(target) => match loop_iterations(pop(stack)?.number()) {
                    Some(_) if loops.len() == MAX_MOLANG_LOOP_DEPTH => {
                        return Err(EvalError::Invalid);
                    }
                    Some(iterations) => loops.push(iterations),
                    None => pc = jump(target)?,
                },
                MolangOp::LoopNext(target)
                | MolangOp::ForEachNext(assets::MolangBranch { target, .. }) => {
                    let remaining = loops.last_mut().ok_or(EvalError::Invalid)?;
                    *remaining = remaining.saturating_sub(1);
                    if *remaining > 0 {
                        pc = jump(target)?;
                    } else {
                        loops.pop();
                    }
                }
                MolangOp::LoopBreak(target) => {
                    loops.pop().ok_or(EvalError::Invalid)?;
                    pc = jump(target)?;
                }
                MolangOp::ForEachStart(branch) => {
                    // No value is an actor array, so the body never runs.
                    pop(stack)?;
                    pc = jump(branch.target)?;
                }
                MolangOp::Arrow(target) => {
                    // No value is an actor reference: the right side is skipped.
                    pop(stack)?;
                    stack.push(MolangValue::Number(0.0));
                    pc = jump(target)?;
                }
            }
        }
        if stack.len() != 1 {
            return Err(EvalError::Invalid);
        }
        pop(stack)
    }

    fn string(&self, symbol: u32) -> Result<Arc<str>, EvalError> {
        self.assets
            .molang_symbols()
            .get(symbol as usize)
            .filter(|symbol| symbol.kind == MolangSymbolKind::String)
            .map(|symbol| Arc::from(symbol.identifier.as_ref()))
            .ok_or(EvalError::Invalid)
    }

    fn query(&self, symbol: u32, arguments: &[MolangValue]) -> MolangValue {
        let Some(symbol) = self.assets.molang_symbols().get(symbol as usize) else {
            return MolangValue::Number(0.0);
        };
        let inputs = query::QueryInputs {
            actor: self.actor,
            input: self.input,
            context: self.context,
            anim_tick: self.anim_tick,
            anim_time: self.anim_time,
            life_tick: self.life_tick,
            finished: self.finished,
            bones: self.bones,
            bone_names: self.bone_names,
        };
        query::query(&inputs, &symbol.identifier, arguments)
    }

    /// Selects a collection item; indices wrap past the end and clamp below zero.
    fn collection(&self, collection: u32, index: f32) -> Result<f32, EvalError> {
        let collection = self
            .assets
            .molang_collections()
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
        self.assets
            .molang_collection_items()
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
