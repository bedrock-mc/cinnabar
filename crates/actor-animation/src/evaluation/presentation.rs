use super::*;
use crate::render_frame::camera;

impl PresentationReads {
    /// Combines inputs conservatively across authored branches and assignments.
    pub(super) fn union(self, other: Self) -> Self {
        Self {
            camera: self.camera || other.camera,
            swing: self.swing || other.swing,
        }
    }
}

impl VariableLayout {
    /// Resolves camera and swing dependencies together, including render-controller writes.
    pub(super) fn bind_presentation_inputs(&mut self, assets: &RuntimeEntityAssets) {
        self.plans = rig_plans::RigPlans::default();
        self.presentation_symbols = (0..assets.molang_symbols().len())
            .map(|symbol| PresentationReads {
                camera: camera::camera_symbol(assets, symbol as u32, false),
                swing: camera::camera_symbol(assets, symbol as u32, true),
            })
            .collect();
        self.expression_inputs =
            vec![PresentationReads::default(); assets.molang_expressions().len()];
        loop {
            let mut changed = false;
            for (index, expression) in assets.molang_expressions().iter().enumerate() {
                let first = expression.first_op as usize;
                let ops = &assets.molang_ops()[first..first + usize::from(expression.op_count)];
                let mut inputs = PresentationReads::default();
                for op in ops {
                    let symbol = match op {
                        MolangOp::LoadVariable(symbol) | MolangOp::LoadQuery(symbol) => *symbol,
                        MolangOp::CallQuery(call) => call.symbol,
                        MolangOp::Coalesce(branch) => branch.symbol,
                        _ => continue,
                    };
                    inputs = inputs.union(self.symbol_inputs(symbol));
                }
                self.expression_inputs[index] = inputs;
                // Each write carries only the reads that reach its value or enclosing branch.
                for (symbol, written) in store_inputs(ops, |symbol| self.symbol_inputs(symbol)) {
                    let previous = self.presentation_symbols[symbol as usize];
                    let next = previous.union(written);
                    changed |= next != previous;
                    self.presentation_symbols[symbol as usize] = next;
                }
            }
            // The native fallback copies the authored factor after pre_animation.
            if let (Some(source), Some(target)) = (
                self.engine.first_person_rotation_factor,
                self.engine.first_person_item_rotation_factor,
            ) {
                let source = self.presentation_symbols[self.variable_base + source];
                let target = &mut self.presentation_symbols[self.variable_base + target];
                let next = target.union(source);
                changed |= next != *target;
                *target = next;
            }
            if !changed {
                break;
            }
        }
    }

    /// Unknown reads conservatively require live camera and swing inputs.
    pub(crate) fn symbol_inputs(&self, symbol: u32) -> PresentationReads {
        self.presentation_symbols
            .get(symbol as usize)
            .copied()
            .unwrap_or(PresentationReads {
                camera: true,
                swing: true,
            })
    }

    /// Shares the assignment closure between pose admission and selected-layer sampling.
    pub(crate) fn expression_inputs(&self, expression: usize) -> PresentationReads {
        self.expression_inputs
            .get(expression)
            .copied()
            .unwrap_or(PresentationReads {
                camera: true,
                swing: true,
            })
    }
}

/// Inputs reaching each variable a program writes, following its stack values, the random
/// stream, and the conditions of the branches, loops and arrows that enclose or precede the write.
fn store_inputs(
    ops: &[MolangOp],
    symbol: impl Fn(u32) -> PresentationReads,
) -> Vec<(u32, PresentationReads)> {
    let end = ops.len();
    let mut control = vec![PresentationReads::default(); end + 1];
    loop {
        let mut stacks: Vec<Option<Vec<PresentationReads>>> = vec![None; end + 1];
        let mut stores = Vec::new();
        let mut regions = Vec::new();
        // The bottom slot threads the random stream: a guarded draw shifts every later draw.
        let mut pending = vec![(0_usize, vec![PresentationReads::default()])];
        while let Some((at, stack)) = pending.pop() {
            let joined = match &stacks[at] {
                Some(existing) if existing.len() != stack.len() => {
                    return conservative_stores(ops, &symbol);
                }
                Some(existing) => {
                    let joined: Vec<_> = existing
                        .iter()
                        .zip(&stack)
                        .map(|(a, b)| a.union(*b))
                        .collect();
                    if &joined == existing {
                        continue;
                    }
                    joined
                }
                None => stack,
            };
            stacks[at] = Some(joined.clone());
            if at == end {
                continue;
            }
            let mut stack = joined;
            let guard = control[at];
            let pop = |stack: &mut Vec<PresentationReads>, count: usize| {
                let keep = stack.len().saturating_sub(count);
                stack
                    .drain(keep..)
                    .fold(PresentationReads::default(), PresentationReads::union)
            };
            let next = at + 1;
            let mut push_to = |target: usize, stack: Vec<PresentationReads>| {
                pending.push((target.min(end), stack));
            };
            match ops[at] {
                MolangOp::Push(_) | MolangOp::PushString(_) | MolangOp::LoadThis => {
                    stack.push(guard);
                    push_to(next, stack);
                }
                MolangOp::LoadQuery(read) | MolangOp::LoadVariable(read) => {
                    stack.push(guard.union(symbol(read)));
                    push_to(next, stack);
                }
                MolangOp::CallQuery(call) => {
                    let arguments = pop(&mut stack, usize::from(call.arguments));
                    stack.push(guard.union(arguments).union(symbol(call.symbol)));
                    push_to(next, stack);
                }
                MolangOp::StoreVariable(written) => {
                    stores.push((written, guard.union(pop(&mut stack, 1))));
                    push_to(next, stack);
                }
                MolangOp::Pop => {
                    pop(&mut stack, 1);
                    push_to(next, stack);
                }
                MolangOp::Coalesce(branch) => {
                    let read = symbol(branch.symbol);
                    regions.push((next, usize::from(branch.target), read));
                    let mut assigned = stack.clone();
                    assigned.push(guard.union(read));
                    push_to(next, stack);
                    push_to(usize::from(branch.target), assigned);
                }
                MolangOp::SelectCollection(_)
                | MolangOp::Negate
                | MolangOp::Not
                | MolangOp::Truthy => {
                    let value = pop(&mut stack, 1);
                    stack.push(guard.union(value));
                    push_to(next, stack);
                }
                MolangOp::Add
                | MolangOp::Subtract
                | MolangOp::Multiply
                | MolangOp::Divide
                | MolangOp::Less
                | MolangOp::LessEqual
                | MolangOp::Greater
                | MolangOp::GreaterEqual
                | MolangOp::Equal
                | MolangOp::NotEqual => {
                    let value = pop(&mut stack, 2);
                    stack.push(guard.union(value));
                    push_to(next, stack);
                }
                MolangOp::Call(function) => {
                    let mut value = guard.union(pop(&mut stack, function.arity()));
                    if function.is_random() {
                        stack[0] = stack[0].union(guard);
                        value = value.union(stack[0]);
                    }
                    stack.push(value);
                    push_to(next, stack);
                }
                MolangOp::Jump(target) | MolangOp::LoopBreak(target) => {
                    push_to(usize::from(target), stack);
                }
                MolangOp::JumpIfFalse(target) | MolangOp::JumpIfTrue(target) => {
                    let condition = pop(&mut stack, 1);
                    let target = usize::from(target);
                    // A skipped else arm also depends on the condition.
                    let until = match target.checked_sub(1).map(|last| ops.get(last)) {
                        Some(Some(MolangOp::Jump(skip))) if usize::from(*skip) > target => {
                            usize::from(*skip)
                        }
                        _ => target,
                    };
                    regions.push((next, until, condition));
                    push_to(next, stack.clone());
                    push_to(target, stack);
                }
                MolangOp::Return => {}
                MolangOp::LoopStart(target)
                | MolangOp::ForEachStart(assets::MolangBranch { target, .. }) => {
                    let count = pop(&mut stack, 1);
                    regions.push((next, usize::from(target), count));
                    push_to(next, stack.clone());
                    push_to(usize::from(target), stack);
                }
                MolangOp::LoopNext(target)
                | MolangOp::ForEachNext(assets::MolangBranch { target, .. }) => {
                    push_to(usize::from(target), stack.clone());
                    push_to(next, stack);
                }
                MolangOp::Arrow(target) => {
                    let owner = pop(&mut stack, 1);
                    regions.push((next, usize::from(target), owner));
                    let mut skipped = stack.clone();
                    skipped.push(guard.union(owner));
                    push_to(next, stack);
                    push_to(usize::from(target), skipped);
                }
            }
        }
        let mut guarded = vec![PresentationReads::default(); end + 1];
        for (from, until, condition) in regions {
            // A guarded early exit decides whether every later write runs at all.
            let until = if ops[from.min(end)..until.min(end)]
                .iter()
                .any(|op| leaves(op, from, until))
            {
                end
            } else {
                until
            };
            for guard in &mut guarded[from.min(end)..until.min(end)] {
                *guard = guard.union(condition);
            }
        }
        if guarded == control {
            return stores;
        }
        control = guarded;
    }
}

/// Malformed programs fall back to tainting every write with every read.
fn conservative_stores(
    ops: &[MolangOp],
    symbol: &impl Fn(u32) -> PresentationReads,
) -> Vec<(u32, PresentationReads)> {
    let reads = ops
        .iter()
        .filter_map(|op| match op {
            MolangOp::LoadVariable(read) | MolangOp::LoadQuery(read) => Some(*read),
            MolangOp::CallQuery(call) => Some(call.symbol),
            MolangOp::Coalesce(branch) => Some(branch.symbol),
            _ => None,
        })
        .fold(PresentationReads::default(), |inputs, read| {
            inputs.union(symbol(read))
        });
    ops.iter()
        .filter_map(|op| match op {
            MolangOp::StoreVariable(written) => Some((*written, reads)),
            _ => None,
        })
        .collect()
}

/// Whether `op` inside `from..until` returns or transfers control outside that region.
fn leaves(op: &MolangOp, from: usize, until: usize) -> bool {
    let target = match *op {
        MolangOp::Return => return true,
        MolangOp::Jump(target)
        | MolangOp::JumpIfFalse(target)
        | MolangOp::JumpIfTrue(target)
        | MolangOp::LoopStart(target)
        | MolangOp::LoopNext(target)
        | MolangOp::LoopBreak(target)
        | MolangOp::Arrow(target) => target,
        MolangOp::Coalesce(branch)
        | MolangOp::ForEachStart(branch)
        | MolangOp::ForEachNext(branch) => branch.target,
        _ => return false,
    };
    let target = usize::from(target);
    target < from || target > until
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAMERA: u32 = 1;

    fn reads(symbol: u32) -> PresentationReads {
        PresentationReads {
            camera: symbol == CAMERA,
            swing: false,
        }
    }

    fn written(ops: &[MolangOp], symbol: u32) -> bool {
        store_inputs(ops, reads)
            .into_iter()
            .filter(|(written, _)| *written == symbol)
            .any(|(_, inputs)| inputs.camera)
    }

    /// Only writes reached by a camera value or a camera-dependent branch inherit it.
    #[test]
    fn writes_inherit_only_their_value_and_branch_inputs() {
        let zero = MolangOp::Push(crate::test_support::scalar(0.0));
        let one = MolangOp::Push(crate::test_support::scalar(1.0));
        let statements = [
            MolangOp::LoadVariable(2),
            MolangOp::StoreVariable(10),
            MolangOp::LoadVariable(CAMERA),
            MolangOp::StoreVariable(11),
            zero,
        ];
        assert!(!written(&statements, 10));
        assert!(written(&statements, 11));
        let block = [
            MolangOp::LoadVariable(CAMERA),
            MolangOp::JumpIfFalse(4),
            one,
            MolangOp::StoreVariable(12),
            zero,
            MolangOp::StoreVariable(13),
            zero,
        ];
        assert!(written(&block, 12));
        assert!(!written(&block, 13));
        let ternary = [
            MolangOp::LoadVariable(CAMERA),
            MolangOp::JumpIfFalse(4),
            one,
            MolangOp::Jump(5),
            zero,
            MolangOp::StoreVariable(14),
            zero,
        ];
        assert!(written(&ternary, 14));
    }

    /// A camera-guarded early return decides whether later writes run.
    #[test]
    fn guarded_exits_taint_every_later_write() {
        let zero = MolangOp::Push(crate::test_support::scalar(0.0));
        let thirty = MolangOp::Push(crate::test_support::scalar(30.0));
        let early = [
            MolangOp::LoadVariable(CAMERA),
            MolangOp::JumpIfFalse(4),
            zero,
            MolangOp::Return,
            thirty,
            MolangOp::StoreVariable(15),
            zero,
        ];
        assert!(written(&early, 15));
    }

    /// A camera-guarded draw shifts the random stream for every later draw.
    #[test]
    fn guarded_random_draws_taint_later_draws() {
        let zero = MolangOp::Push(crate::test_support::scalar(0.0));
        let one = MolangOp::Push(crate::test_support::scalar(1.0));
        let random = MolangOp::Call(assets::MolangFunction::Random);
        let draws = [
            MolangOp::LoadVariable(CAMERA),
            MolangOp::JumpIfFalse(6),
            zero,
            one,
            random,
            MolangOp::Pop,
            zero,
            one,
            random,
            MolangOp::StoreVariable(16),
            zero,
        ];
        assert!(written(&draws, 16));
        let unguarded = [zero, one, random, MolangOp::StoreVariable(17), zero];
        assert!(!written(&unguarded, 17));
    }
}
