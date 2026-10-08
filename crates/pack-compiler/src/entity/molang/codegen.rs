use assets::{AssetError, EntityGeometryScalar, MolangFunction, molang_call};

use super::parser::{Binary, Expr, Program, Slot, Statement, Unary};
use crate::entity::invalid;

/// Ops that still name their symbols; the compiler interns names when it finishes.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum IrOp {
    Push(f32),
    PushString(Box<str>),
    LoadThis,
    LoadQuery(Box<str>),
    CallQuery(Box<str>, u8),
    LoadVariable(Slot, Box<str>),
    StoreVariable(Slot, Box<str>),
    Coalesce(Slot, Box<str>, Label),
    Pop,
    Negate,
    Not,
    Truthy,
    Binary(Binary),
    Call(MolangFunction),
    Jump(Label),
    JumpIfFalse(Label),
    JumpIfTrue(Label),
    Return,
    LoopStart(Label),
    LoopNext(Label),
    LoopBreak(Label),
    ForEachStart(Slot, Box<str>, Label),
    ForEachNext(Slot, Box<str>, Label),
    Arrow(Label),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Label(usize);

impl Label {
    /// Op offset of a resolved label.
    pub(super) const fn offset(self) -> usize {
        self.0
    }
}

#[derive(Default)]
pub(super) struct Codegen {
    ops: Vec<IrOp>,
    labels: Vec<Option<usize>>,
    /// Continue and break labels of each open loop, innermost last.
    loops: Vec<(Label, Label)>,
}

impl Codegen {
    pub(super) fn program(program: &Program) -> Result<Vec<IrOp>, AssetError> {
        let mut codegen = Self::default();
        match program {
            Program::Simple(expression) => codegen.expression(expression)?,
            Program::Complex(statements) => {
                codegen.statements(statements)?;
                codegen.emit(IrOp::Push(0.0));
            }
        }
        codegen.finish()
    }

    fn finish(self) -> Result<Vec<IrOp>, AssetError> {
        let resolve = |label: Label| -> Result<Label, AssetError> {
            self.labels[label.0]
                .map(Label)
                .ok_or_else(|| invalid("unplaced Molang label"))
        };
        self.ops
            .iter()
            .map(|op| {
                Ok(match op {
                    IrOp::Coalesce(slot, name, label) => {
                        IrOp::Coalesce(*slot, name.clone(), resolve(*label)?)
                    }
                    IrOp::Jump(label) => IrOp::Jump(resolve(*label)?),
                    IrOp::JumpIfFalse(label) => IrOp::JumpIfFalse(resolve(*label)?),
                    IrOp::JumpIfTrue(label) => IrOp::JumpIfTrue(resolve(*label)?),
                    IrOp::LoopStart(label) => IrOp::LoopStart(resolve(*label)?),
                    IrOp::LoopNext(label) => IrOp::LoopNext(resolve(*label)?),
                    IrOp::LoopBreak(label) => IrOp::LoopBreak(resolve(*label)?),
                    IrOp::ForEachStart(slot, name, label) => {
                        IrOp::ForEachStart(*slot, name.clone(), resolve(*label)?)
                    }
                    IrOp::ForEachNext(slot, name, label) => {
                        IrOp::ForEachNext(*slot, name.clone(), resolve(*label)?)
                    }
                    IrOp::Arrow(label) => IrOp::Arrow(resolve(*label)?),
                    other => other.clone(),
                })
            })
            .collect()
    }

    fn emit(&mut self, op: IrOp) {
        self.ops.push(op);
    }

    fn label(&mut self) -> Label {
        self.labels.push(None);
        Label(self.labels.len() - 1)
    }

    fn place(&mut self, label: Label) {
        self.labels[label.0] = Some(self.ops.len());
    }

    fn statements(&mut self, statements: &[Statement]) -> Result<(), AssetError> {
        for statement in statements {
            self.statement(statement)?;
        }
        Ok(())
    }

    fn statement(&mut self, statement: &Statement) -> Result<(), AssetError> {
        match statement {
            Statement::Expression(Expr::Branch(condition, yes, no)) => {
                let (otherwise, end) = (self.label(), self.label());
                self.expression(condition)?;
                self.emit(IrOp::JumpIfFalse(otherwise));
                self.statements(yes)?;
                self.emit(IrOp::Jump(end));
                self.place(otherwise);
                if let Some(no) = no {
                    self.statements(no)?;
                }
                self.place(end);
            }
            Statement::Expression(expression) => {
                self.expression(expression)?;
                self.emit(IrOp::Pop);
            }
            Statement::Assign(slot, name, value) => {
                self.expression(value)?;
                self.emit(IrOp::StoreVariable(*slot, name.clone()));
            }
            Statement::Return(value) => {
                match value {
                    Some(value) => self.expression(value)?,
                    None => self.emit(IrOp::Push(0.0)),
                }
                self.emit(IrOp::Return);
            }
            Statement::Break => {
                let &(_, end) = self
                    .loops
                    .last()
                    .ok_or_else(|| invalid("break outside a Molang loop"))?;
                self.emit(IrOp::LoopBreak(end));
            }
            Statement::Continue => {
                let &(next, _) = self
                    .loops
                    .last()
                    .ok_or_else(|| invalid("continue outside a Molang loop"))?;
                self.emit(IrOp::Jump(next));
            }
            Statement::Loop(count, body) => {
                let (body_start, next, end) = (self.label(), self.label(), self.label());
                self.expression(count)?;
                self.emit(IrOp::LoopStart(end));
                self.place(body_start);
                self.loops.push((next, end));
                self.statements(body)?;
                self.loops.pop();
                self.place(next);
                self.emit(IrOp::LoopNext(body_start));
                self.place(end);
            }
            Statement::ForEach(slot, name, array, body) => {
                let (body_start, next, end) = (self.label(), self.label(), self.label());
                self.expression(array)?;
                self.emit(IrOp::ForEachStart(*slot, name.clone(), end));
                self.place(body_start);
                self.loops.push((next, end));
                self.statements(body)?;
                self.loops.pop();
                self.place(next);
                self.emit(IrOp::ForEachNext(*slot, name.clone(), body_start));
                self.place(end);
            }
            Statement::Block(statements) => self.statements(statements)?,
        }
        Ok(())
    }

    fn expression(&mut self, expression: &Expr) -> Result<(), AssetError> {
        if let Some(value) = fold(expression) {
            self.emit(IrOp::Push(value));
            return Ok(());
        }
        match expression {
            Expr::Number(value) => {
                EntityGeometryScalar::new(*value)
                    .ok_or_else(|| invalid("Molang constant exceeds the carrier bound"))?;
                self.emit(IrOp::Push(*value));
            }
            Expr::String(text) => self.emit(IrOp::PushString(text.clone())),
            Expr::This => self.emit(IrOp::LoadThis),
            Expr::Variable(slot, name) => self.emit(IrOp::LoadVariable(*slot, name.clone())),
            Expr::Query(name, None) => self.emit(IrOp::LoadQuery(name.clone())),
            Expr::Query(name, Some(arguments)) => {
                let count = u8::try_from(arguments.len())
                    .ok()
                    .filter(|count| *count <= assets::MAX_MOLANG_QUERY_ARGUMENTS)
                    .ok_or_else(|| invalid("Molang query argument count exceeds bound"))?;
                for argument in arguments {
                    self.expression(argument)?;
                }
                self.emit(IrOp::CallQuery(name.clone(), count));
            }
            Expr::Call(function, arguments) => {
                for argument in arguments {
                    self.expression(argument)?;
                }
                self.emit(IrOp::Call(*function));
            }
            Expr::Unary(operator, value) => {
                self.expression(value)?;
                self.emit(match operator {
                    Unary::Negate => IrOp::Negate,
                    Unary::Not => IrOp::Not,
                });
            }
            Expr::Binary(operator, left, right) => {
                self.expression(left)?;
                self.expression(right)?;
                self.emit(IrOp::Binary(*operator));
            }
            Expr::And(left, right) | Expr::Or(left, right) => {
                let and = matches!(expression, Expr::And(..));
                let (short, end) = (self.label(), self.label());
                self.expression(left)?;
                self.emit(if and {
                    IrOp::JumpIfFalse(short)
                } else {
                    IrOp::JumpIfTrue(short)
                });
                self.expression(right)?;
                self.emit(IrOp::Truthy);
                self.emit(IrOp::Jump(end));
                self.place(short);
                self.emit(IrOp::Push(if and { 0.0 } else { 1.0 }));
                self.place(end);
            }
            Expr::Conditional(condition, yes, no) => {
                let (otherwise, end) = (self.label(), self.label());
                self.expression(condition)?;
                self.emit(IrOp::JumpIfFalse(otherwise));
                self.expression(yes)?;
                self.emit(IrOp::Jump(end));
                self.place(otherwise);
                match no {
                    Some(no) => self.expression(no)?,
                    None => self.emit(IrOp::Push(0.0)),
                }
                self.place(end);
            }
            Expr::Coalesce(slot, name, fallback) => {
                let end = self.label();
                self.emit(IrOp::Coalesce(*slot, name.clone(), end));
                self.expression(fallback)?;
                self.place(end);
            }
            Expr::Branch(..) => {
                return Err(invalid("a Molang statement branch has no value"));
            }
            Expr::Arrow(actor, value) => {
                let end = self.label();
                self.expression(actor)?;
                self.emit(IrOp::Arrow(end));
                self.expression(value)?;
                self.place(end);
            }
        }
        Ok(())
    }
}

/// Folds a pure numeric subtree; randomness, names, and strings stay dynamic.
pub(super) fn fold(expression: &Expr) -> Option<f32> {
    fold_with(expression, &|_| None)
}

/// Folds with every query and variable read through `read`, for authoring-time defaults.
pub(super) fn fold_with(expression: &Expr, read: &dyn Fn(&Expr) -> Option<f32>) -> Option<f32> {
    let value = match expression {
        Expr::Number(value) => *value,
        Expr::Variable(..) | Expr::Query(..) => read(expression)?,
        Expr::String(_) | Expr::This | Expr::Arrow(..) | Expr::Coalesce(..) | Expr::Branch(..) => {
            return None;
        }
        Expr::Unary(operator, value) => {
            let value = fold_with(value, read)?;
            match operator {
                Unary::Negate => -value,
                Unary::Not => truth(value == 0.0),
            }
        }
        Expr::Binary(operator, left, right) => {
            let (left, right) = (fold_with(left, read)?, fold_with(right, read)?);
            binary(*operator, left, right)
        }
        Expr::And(left, right) => {
            truth(fold_with(left, read)? != 0.0 && fold_with(right, read)? != 0.0)
        }
        Expr::Or(left, right) => {
            truth(fold_with(left, read)? != 0.0 || fold_with(right, read)? != 0.0)
        }
        Expr::Conditional(condition, yes, no) => {
            if fold_with(condition, read)? != 0.0 {
                fold_with(yes, read)?
            } else {
                no.as_ref().map_or(Some(0.0), |no| fold_with(no, read))?
            }
        }
        Expr::Call(function, arguments) => {
            if function.is_random() {
                return None;
            }
            let values = arguments
                .iter()
                .map(|argument| fold_with(argument, read))
                .collect::<Option<Vec<_>>>()?;
            molang_call(*function, &values, &mut || 0.0)
        }
    };
    // Only values the carrier can store are folded.
    EntityGeometryScalar::new(value).map(|_| value)
}

pub(super) fn binary(operator: Binary, left: f32, right: f32) -> f32 {
    match operator {
        Binary::Add => left + right,
        Binary::Subtract => left - right,
        Binary::Multiply => left * right,
        // Same near-zero divisor rule as the runtime; needs independent measurement.
        Binary::Divide => {
            if right.abs() < f32::EPSILON {
                0.0
            } else {
                left / right
            }
        }
        Binary::Less => truth(left < right),
        Binary::LessEqual => truth(left <= right),
        Binary::Greater => truth(left > right),
        Binary::GreaterEqual => truth(left >= right),
        Binary::Equal => truth(left == right),
        Binary::NotEqual => truth(left != right),
    }
}

fn truth(value: bool) -> f32 {
    if value { 1.0 } else { 0.0 }
}
