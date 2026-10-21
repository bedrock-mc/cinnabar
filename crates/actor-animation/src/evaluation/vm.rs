//! Bounded execution of the compiled Molang instruction stream.
use super::*;

impl Evaluator<'_> {
    /// Executes bounded bytecode and optionally records reads before scratch writes.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_on(
        &self,
        stack: &mut Vec<MolangValue>,
        expression_index: usize,
        variables: &mut MolangVariables,
        this: f32,
        budget: &mut EvalBudget<'_>,
        mut reads: Option<&mut ObservedInputs>,
    ) -> Result<MolangValue, EvalError> {
        let expression = self
            .expressions()
            .get(expression_index)
            .ok_or(EvalError::Invalid)?;
        let first = expression.first_op as usize;
        let end = first
            .checked_add(expression.op_count as usize)
            .ok_or(EvalError::Invalid)?;
        let ops = self.ops().get(first..end).ok_or(EvalError::Invalid)?;
        if let Some(cacheable) = budget.static_draw.as_mut() {
            *cacheable &= self.program.is_none()
                && super::attachable::static_draw::expression_is_static(
                    self.assets,
                    expression_index,
                    false,
                );
        }
        // Observed owner predicates restore their writes; ordinary evaluations clear temps here.
        if reads.is_none() {
            variables.clear_temporaries();
        }
        let mut loops = Vec::new();
        let mut pc = 0;
        if reads.is_none()
            && self.program.is_none()
            && let Some(Some(program)) = self.layout.numeric.get(expression_index)
            && program.accepts(self, variables)
        {
            match program.run(self, variables, this, budget, stack) {
                numeric::Outcome::Complete(value) => return Ok(MolangValue::Number(value)),
                numeric::Outcome::Resume(offset) => pc = offset,
            }
        }
        stack.reserve(expression.max_stack as usize);
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
                    let value = self.query(symbol, &[]);
                    if let Some(reads) = reads.as_mut() {
                        reads.query(symbol, &[], &value);
                    }
                    stack.push(value);
                }
                MolangOp::CallQuery(call) => {
                    let start = stack
                        .len()
                        .checked_sub(call.arguments as usize)
                        .ok_or(EvalError::Invalid)?;
                    let value = self.query(call.symbol, &stack[start..]);
                    if let Some(reads) = reads.as_mut() {
                        reads.query(call.symbol, &stack[start..], &value);
                    }
                    stack.truncate(start);
                    stack.push(value);
                }
                MolangOp::LoadVariable(symbol) => {
                    let place = self.layout.place(symbol).ok_or(EvalError::Invalid)?;
                    if let Some(reads) = reads.as_mut() {
                        reads.variable(symbol, place, variables);
                    }
                    let value = variables.read(place).unwrap_or(MolangValue::Number(0.0));
                    stack.push(value);
                }
                MolangOp::StoreVariable(symbol) => {
                    let value = pop(stack)?;
                    let place = self.layout.place(symbol).ok_or(EvalError::Invalid)?;
                    if let Some(reads) = reads.as_mut() {
                        reads.written(symbol, place, variables);
                    }
                    variables.store(place, value)?;
                }
                MolangOp::Coalesce(branch) => {
                    let place = self.layout.place(branch.symbol).ok_or(EvalError::Invalid)?;
                    if let Some(reads) = reads.as_mut() {
                        reads.variable(branch.symbol, place, variables);
                    }
                    if let Some(value) = variables.read(place) {
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
                        MolangValue::String(_) | MolangValue::ActorReference(_) => false,
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
                        (MolangValue::ActorReference(left), MolangValue::ActorReference(right)) => {
                            left == right
                        }
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
                    // Attachable queries already read the supplied owning actor snapshot.
                    let reference = pop(stack)?;
                    if !matches!(reference, MolangValue::ActorReference(id) if id == self.actor.runtime_id())
                    {
                        stack.push(MolangValue::Number(0.0));
                        pc = jump(target)?;
                    }
                }
            }
        }
        if stack.len() != 1 {
            return Err(EvalError::Invalid);
        }
        pop(stack)
    }
}
