//! Conservative admission and load fusion for the numeric lane interpreter.

use super::*;

/// Binds eligible carrier expressions once; independently admitted programs use the generic VM.
pub(in crate::evaluation) fn compile(
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
) -> Vec<Option<Program>> {
    assets
        .molang_expressions()
        .iter()
        .map(|expression| {
            let start = expression.first_op as usize;
            let ops = assets
                .molang_ops()
                .get(start..start.checked_add(usize::from(expression.op_count))?)?;
            compile_expression(ops, assets, layout)
        })
        .collect()
}

/// Maps a safe load to a numeric source, retaining the query identity and variable slot.
fn load(op: MolangOp, assets: &RuntimeEntityAssets, layout: &VariableLayout) -> Option<Source> {
    match op {
        MolangOp::Push(value) => Some(Source::Constant(value.get())),
        MolangOp::LoadThis => Some(Source::This),
        MolangOp::LoadVariable(symbol) => match layout.place(symbol)? {
            Place::Variable(slot) => Some(Source::Variable(slot)),
            Place::Temporary(_) => None,
        },
        MolangOp::LoadQuery(symbol)
        | MolangOp::CallQuery(assets::MolangCall {
            symbol,
            arguments: 0,
        }) => {
            let binding = assets.molang_query_binding(symbol)?;
            (binding.descriptor().output == MolangQueryOutput::Number)
                .then_some(Source::Query(symbol, binding))
        }
        _ => None,
    }
}

/// Recognizes pure single-input operations that may fuse with their preceding load.
fn unary(op: MolangOp) -> Option<Unary> {
    match op {
        MolangOp::Negate => Some(Unary::Negate),
        MolangOp::Not => Some(Unary::Not),
        MolangOp::Truthy => Some(Unary::Truthy),
        MolangOp::Call(function) if function.arity() == 1 && !function.is_random() => {
            Some(Unary::Math(function))
        }
        _ => None,
    }
}

/// Recognizes binary numeric operations without changing their shared arithmetic semantics.
fn binary(op: MolangOp) -> bool {
    matches!(
        op,
        MolangOp::Add
            | MolangOp::Subtract
            | MolangOp::Multiply
            | MolangOp::Divide
            | MolangOp::Less
            | MolangOp::LessEqual
            | MolangOp::Greater
            | MolangOp::GreaterEqual
            | MolangOp::Equal
            | MolangOp::NotEqual
    )
}

/// Admits only forward numeric control flow, then fuses loads within basic blocks.
fn compile_expression(
    ops: &[MolangOp],
    assets: &RuntimeEntityAssets,
    layout: &VariableLayout,
) -> Option<Program> {
    let mut targets = vec![false; ops.len() + 1];
    let mut heights = vec![None; ops.len() + 1];
    heights[0] = Some(0u8);
    // Inspect even unreachable instructions: an effect or mixed-type boundary rejects the plan.
    for (pc, &op) in ops.iter().enumerate() {
        let supported = load(op, assets, layout).is_some()
            || unary(op).is_some()
            || binary(op)
            || matches!(op, MolangOp::Call(function) if !function.is_random())
            || matches!(
                op,
                MolangOp::Pop
                    | MolangOp::Jump(_)
                    | MolangOp::JumpIfFalse(_)
                    | MolangOp::JumpIfTrue(_)
                    | MolangOp::Return
            );
        if !supported {
            return None;
        }
        let branch = match op {
            MolangOp::Jump(target)
            | MolangOp::JumpIfFalse(target)
            | MolangOp::JumpIfTrue(target) => Some(usize::from(target)),
            _ => None,
        };
        if let Some(target) = branch {
            if target <= pc || target > ops.len() {
                return None;
            }
            targets[target] = true;
        }
        let Some(height) = heights[pc] else {
            continue;
        };
        let (pops, pushes) = if load(op, assets, layout).is_some() {
            (0, 1)
        } else if unary(op).is_some() {
            (1, 1)
        } else if binary(op) {
            (2, 1)
        } else {
            match op {
                MolangOp::Call(function) => (function.arity() as u8, 1),
                MolangOp::Jump(_) => (0, 0),
                _ => (1, 0),
            }
        };
        let next = height.checked_sub(pops)?.checked_add(pushes)?;
        if next > assets::MAX_MOLANG_STACK_DEPTH {
            return None;
        }
        if let Some(target) = branch {
            set_height(&mut heights[target], next)?;
        }
        if !matches!(op, MolangOp::Jump(_) | MolangOp::Return) {
            set_height(&mut heights[pc + 1], next)?;
        }
    }
    if heights[ops.len()].is_some_and(|height| height != 1) {
        return None;
    }
    let mut instructions = Vec::new();
    let mut offsets = vec![0; ops.len() + 1];
    let mut variables = Vec::new();
    let mut queries = Vec::new();
    let mut pc = 0;
    while let Some(&op) = ops.get(pc) {
        offsets[pc] = instructions.len();
        let Some(height) = heights[pc] else {
            pc += 1;
            continue;
        };
        let mut cost = 1;
        let (lane, operation) = if let Some(source) = load(op, assets, layout) {
            match source {
                Source::Variable(slot) => variables.push(slot),
                Source::Query(symbol, _) => queries.push(symbol),
                _ => {}
            }
            match ops.get(pc + 1).copied().filter(|_| !targets[pc + 1]) {
                Some(next) if unary(next).is_some() => {
                    cost = 2;
                    (height, Op::Unary(unary(next)?, source))
                }
                Some(next) if binary(next) => {
                    cost = 2;
                    (height.checked_sub(1)?, Op::Binary(next, source))
                }
                _ => (height, Op::Load(source)),
            }
        } else if let Some(operation) = unary(op) {
            let lane = height.checked_sub(1)?;
            (lane, Op::Unary(operation, Source::Lane(lane)))
        } else if binary(op) {
            let lane = height.checked_sub(2)?;
            (lane, Op::Binary(op, Source::Lane(lane + 1)))
        } else {
            match op {
                MolangOp::Call(function) => (
                    height.checked_sub(function.arity() as u8)?,
                    Op::Call(function),
                ),
                MolangOp::Jump(target) => (0, Op::Jump(usize::from(target))),
                MolangOp::JumpIfFalse(target) | MolangOp::JumpIfTrue(target) => (
                    height.checked_sub(1)?,
                    Op::Branch {
                        target: usize::from(target),
                        truthy: matches!(op, MolangOp::JumpIfTrue(_)),
                    },
                ),
                MolangOp::Return => (height.checked_sub(1)?, Op::Return),
                MolangOp::Pop => (height.checked_sub(1)?, Op::Pop),
                _ => return None,
            }
        };
        instructions.push(Instruction {
            pc: pc as u16,
            height,
            lane,
            cost,
            op: operation,
        });
        pc += usize::from(cost);
    }
    offsets[ops.len()] = instructions.len();
    for instruction in &mut instructions {
        match &mut instruction.op {
            Op::Jump(target) | Op::Branch { target, .. } => *target = offsets[*target],
            _ => {}
        }
    }
    variables.sort_unstable();
    variables.dedup();
    queries.sort_unstable();
    queries.dedup();
    Some(Program {
        instructions: instructions.into(),
        variables: variables.into(),
        queries: queries.into(),
        final_height: heights[ops.len()].unwrap_or(0),
    })
}

/// Requires both incoming paths to address the same numeric lanes at their join.
fn set_height(slot: &mut Option<u8>, height: u8) -> Option<()> {
    if slot.is_some_and(|previous| previous != height) {
        return None;
    }
    *slot = Some(height);
    Some(())
}
