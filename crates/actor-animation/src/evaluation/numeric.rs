//! Typed lanes for pure numeric expressions. Budget tails resume the original bytecode.

use super::*;
use assets::{MolangFunction, MolangQuery, MolangQueryOutput};

mod compile;
#[cfg(test)]
mod tests;

pub(super) use compile::compile;

/// A value source bound once, without a runtime symbol or stack lookup.
#[derive(Clone, Copy, Debug)]
enum Source {
    Constant(f32),
    This,
    Variable(usize),
    Query(u32, MolangQuery),
    Lane(u8),
}

/// Single-input transforms can consume a load directly in the same instruction.
#[derive(Clone, Copy, Debug)]
enum Unary {
    Negate,
    Not,
    Truthy,
    Math(MolangFunction),
}

/// Operations address lanes directly; fusion preserves the original f32 operations.
#[derive(Clone, Copy, Debug)]
enum Op {
    Load(Source),
    Unary(Unary, Source),
    Binary(MolangOp, Source),
    Call(MolangFunction),
    Pop,
    Jump(usize),
    Branch { target: usize, truthy: bool },
    Return,
}

/// A fused instruction retains its exact bytecode entry and pre-instruction stack depth.
#[derive(Debug)]
struct Instruction {
    pc: u16,
    height: u8,
    lane: u8,
    cost: u8,
    op: Op,
}

/// Immutable numeric instructions and the dynamic types that must remain numeric.
#[derive(Debug)]
pub(super) struct Program {
    instructions: Box<[Instruction]>,
    variables: Box<[usize]>,
    queries: Box<[u32]>,
    final_height: u8,
}

/// A budget tail continues in the generic VM at the original semantic instruction.
pub(super) enum Outcome {
    Complete(f32),
    Resume(usize),
}

impl Program {
    /// Rejects dynamic strings/references before any query, budget charge or arithmetic runs.
    pub(super) fn accepts(&self, evaluator: &Evaluator<'_>, variables: &MolangVariables) -> bool {
        self.variables
            .iter()
            .all(|slot| variables.numeric_variable(*slot).is_some())
            && evaluator.query_history.is_none_or(|history| {
                self.queries.iter().all(|symbol| {
                    history
                        .binary_search_by_key(symbol, |(symbol, _)| *symbol)
                        .ok()
                        .is_none_or(|slot| matches!(history[slot].1, MolangValue::Number(_)))
                })
            })
    }

    /// Runs bound numeric operations, materializing only a budget tail for the generic VM.
    pub(super) fn run(
        &self,
        evaluator: &Evaluator<'_>,
        variables: &MolangVariables,
        this: f32,
        budget: &mut EvalBudget<'_>,
        stack: &mut Vec<MolangValue>,
    ) -> Outcome {
        let mut lanes = [0.0; assets::MAX_MOLANG_STACK_DEPTH as usize];
        let mut pc = 0;
        while let Some(instruction) = self.instructions.get(pc) {
            let cost = usize::from(instruction.cost);
            if budget.actor_left < cost || *budget.world_left < cost {
                stack.extend(
                    lanes[..usize::from(instruction.height)]
                        .iter()
                        .copied()
                        .map(MolangValue::Number),
                );
                return Outcome::Resume(usize::from(instruction.pc));
            }
            budget.actor_left -= cost;
            *budget.world_left -= cost;
            budget.used += cost;
            let lane = usize::from(instruction.lane);
            let source = |source| match source {
                Source::Constant(value) => value,
                Source::This => this,
                Source::Variable(slot) => variables.numeric_variable(slot).unwrap(),
                Source::Query(symbol, binding) => {
                    evaluator.bound_query(symbol, Some(binding), &[]).number()
                }
                Source::Lane(index) => lanes[usize::from(index)],
            };
            pc += 1;
            match instruction.op {
                Op::Load(input) => lanes[lane] = source(input),
                Op::Unary(operation, input) => {
                    let value = source(input);
                    lanes[lane] = match operation {
                        Unary::Negate => -value,
                        Unary::Not => (value == 0.0) as u8 as f32,
                        Unary::Truthy => (value != 0.0) as u8 as f32,
                        Unary::Math(function) => pure_call(function, &[value]),
                    };
                }
                Op::Binary(operation, right) => {
                    let right = source(right);
                    let left = lanes[lane];
                    lanes[lane] = match operation {
                        MolangOp::Equal => (left == right) as u8 as f32,
                        MolangOp::NotEqual => (left != right) as u8 as f32,
                        _ => arithmetic(&operation, left, right),
                    };
                }
                Op::Call(function) => {
                    lanes[lane] = pure_call(function, &lanes[lane..lane + function.arity()]);
                }
                Op::Pop => {}
                Op::Jump(target) => pc = target,
                Op::Branch { target, truthy } => {
                    if (lanes[lane] != 0.0) == truthy {
                        pc = target;
                    }
                }
                Op::Return => return Outcome::Complete(lanes[lane]),
            }
        }
        debug_assert_eq!(self.final_height, 1);
        Outcome::Complete(lanes[0])
    }
}

/// Uses the shared math implementation; compiled numeric programs exclude random calls.
fn pure_call(function: MolangFunction, arguments: &[f32]) -> f32 {
    molang_call(function, arguments, &mut || {
        unreachable!("random calls use the generic evaluator")
    })
}
