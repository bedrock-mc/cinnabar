//! Flat postfix code compiled once from the parsed tree. Evaluation is one loop over a small
//! value stack: no recursion, boxed-node pointer chasing or per-evaluation allocation.

use assets::entity::MolangFunction;

use super::{Expr, MAX_TEMPS, Op, Queries, Query, Rng};

/// Programs needing at most this many pending values evaluate on an inline stack.
const INLINE_STACK: usize = 32;

/// A query operand; `Named` indexes the program's name table.
#[derive(Clone, Copy, Debug)]
enum QueryRef {
    IsInWater,
    IsInWaterOrRain,
    IsBaby,
    GroundSpeed,
    FrameAlpha,
    SpellColor(u8),
    Named(u16),
}

/// One postfix instruction. Jump operands are absolute instruction indices.
#[derive(Clone, Copy, Debug)]
enum Inst {
    Num(f32),
    Var(u16),
    Temp(u8),
    Query(QueryRef),
    Neg,
    Not,
    /// Strict arithmetic or comparison of the two top values, cleaned to a finite result.
    Bin(Op),
    /// Pops the `&&` left operand; a zero pushes 0 and jumps past the right operand.
    AndSkip(u16),
    /// Pops the `||` left operand; a non-zero pushes 1 and jumps past the right operand.
    OrSkip(u16),
    /// Replaces the top value with 1 when non-zero, else 0.
    Truthy,
    /// Pops a condition and jumps when it is zero.
    JumpIfZero(u16),
    Jump(u16),
    /// Pushes an assigned variable and jumps past the fallback; unassigned falls through.
    CoalesceVar(u16, u16),
    /// Pushes an assigned temp and jumps past the fallback; unassigned falls through.
    CoalesceTemp(u8, u16),
    /// Pops this many evaluated arguments (at most four) and pushes the cleaned result.
    Call(Option<MolangFunction>, u8),
    AssignVar(u16),
    AssignTemp(u8),
    /// Ends evaluation with the top value.
    Return,
    /// Discards the previous statement's value.
    Pop,
}

/// Compiled statements of one program, evaluated in the tree's exact order.
#[derive(Clone, Debug)]
pub(super) struct Code {
    insts: Box<[Inst]>,
    names: Box<[Box<str>]>,
    /// Most values pending at once during evaluation.
    stack: usize,
}

/// Lowers parsed statements; operands evaluate exactly as the tree would, including
/// short-circuiting, unevaluated call arguments past the fourth, and statement-level returns.
pub(super) fn compile(statements: &[Expr]) -> Code {
    let mut compiler = Compiler::default();
    for (index, statement) in statements.iter().enumerate() {
        if index > 0 {
            compiler.insts.push(Inst::Pop);
            compiler.depth -= 1;
        }
        compiler.expr(statement);
    }
    Code {
        insts: compiler.insts.into_boxed_slice(),
        names: compiler.names.into_boxed_slice(),
        stack: compiler.max,
    }
}

/// Emission state; `depth` tracks values pending at the current instruction, `max` its peak.
#[derive(Default)]
struct Compiler {
    insts: Vec<Inst>,
    names: Vec<Box<str>>,
    depth: usize,
    max: usize,
}

impl Compiler {
    /// Emits an instruction that pushes one value.
    fn leaf(&mut self, inst: Inst) {
        self.insts.push(inst);
        self.depth += 1;
        self.max = self.max.max(self.depth);
    }

    /// Emits a forward jump and returns its index for [`Self::land`].
    fn jump(&mut self, inst: Inst) -> usize {
        self.insts.push(inst);
        self.insts.len() - 1
    }

    /// Points the forward jump at `at` to the next emitted instruction.
    fn land(&mut self, at: usize) {
        let target = self.insts.len() as u16;
        match &mut self.insts[at] {
            Inst::AndSkip(to)
            | Inst::OrSkip(to)
            | Inst::JumpIfZero(to)
            | Inst::Jump(to)
            | Inst::CoalesceVar(_, to)
            | Inst::CoalesceTemp(_, to) => *to = target,
            _ => unreachable!("only jumps are landed"),
        }
    }

    /// Maps a parsed query to its operand, appending named queries to the name table.
    fn query(&mut self, query: &Query) -> QueryRef {
        match query {
            Query::IsInWater => QueryRef::IsInWater,
            Query::IsInWaterOrRain => QueryRef::IsInWaterOrRain,
            Query::IsBaby => QueryRef::IsBaby,
            Query::GroundSpeed => QueryRef::GroundSpeed,
            Query::FrameAlpha => QueryRef::FrameAlpha,
            Query::SpellColor(component) => QueryRef::SpellColor(*component as u8),
            Query::Named(name) => {
                self.names.push(name.clone());
                QueryRef::Named((self.names.len() - 1) as u16)
            }
        }
    }

    /// Emits code that leaves exactly one value on the stack.
    fn expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Num(value) => self.leaf(Inst::Num(*value)),
            Expr::Var(slot) => self.leaf(Inst::Var(*slot)),
            Expr::Temp(slot) => self.leaf(Inst::Temp(*slot)),
            Expr::Query(query) => {
                let query = self.query(query);
                self.leaf(Inst::Query(query));
            }
            Expr::Neg(inner) => {
                self.expr(inner);
                self.insts.push(Inst::Neg);
            }
            Expr::Not(inner) => {
                self.expr(inner);
                self.insts.push(Inst::Not);
            }
            Expr::Bin(op @ (Op::And | Op::Or), left, right) => {
                self.expr(left);
                self.depth -= 1;
                let skip = self.jump(if *op == Op::And {
                    Inst::AndSkip(0)
                } else {
                    Inst::OrSkip(0)
                });
                self.expr(right);
                self.insts.push(Inst::Truthy);
                self.land(skip);
            }
            Expr::Bin(op, left, right) => {
                self.expr(left);
                self.expr(right);
                self.insts.push(Inst::Bin(*op));
                self.depth -= 1;
            }
            Expr::Cond(condition, yes, no) => {
                self.expr(condition);
                self.depth -= 1;
                let otherwise = self.jump(Inst::JumpIfZero(0));
                self.expr(yes);
                self.depth -= 1;
                let end = self.jump(Inst::Jump(0));
                self.land(otherwise);
                self.expr(no);
                self.land(end);
            }
            Expr::Coalesce(left, right) => match left.as_ref() {
                Expr::Var(slot) => {
                    let assigned = self.jump(Inst::CoalesceVar(*slot, 0));
                    self.expr(right);
                    self.land(assigned);
                }
                Expr::Temp(slot) => {
                    let assigned = self.jump(Inst::CoalesceTemp(*slot, 0));
                    self.expr(right);
                    self.land(assigned);
                }
                // Any other left operand is always "set": the fallback never evaluates.
                other => self.expr(other),
            },
            Expr::Call(func, args) => {
                let count = args.len().min(4);
                for arg in &args[..count] {
                    self.expr(arg);
                }
                self.insts.push(Inst::Call(*func, count as u8));
                self.depth -= count;
                self.depth += 1;
                self.max = self.max.max(self.depth);
            }
            Expr::AssignVar(slot, value) => {
                self.expr(value);
                self.insts.push(Inst::AssignVar(*slot));
            }
            Expr::AssignTemp(slot, value) => {
                self.expr(value);
                self.insts.push(Inst::AssignTemp(*slot));
            }
            Expr::Return(value) => {
                self.expr(value);
                self.insts.push(Inst::Return);
            }
        }
    }
}

/// Replaces a non-finite result with 0.
fn clean(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

/// An assigned variable's value; unassigned (NaN or out of range) reads 0.
fn read_var(vars: &[f32], slot: u16) -> f32 {
    vars.get(slot as usize)
        .copied()
        .filter(|value| !value.is_nan())
        .unwrap_or(0.0)
}

impl Code {
    /// Runs the program; the result is the `return` value or the last statement's value.
    pub(super) fn eval(&self, vars: &mut Vec<f32>, rng: &mut Rng, queries: &Queries) -> f32 {
        if self.stack <= INLINE_STACK {
            self.run(&mut [0.0f32; INLINE_STACK], vars, rng, queries)
        } else {
            self.run(&mut vec![0.0f32; self.stack], vars, rng, queries)
        }
    }

    /// Executes on `stack`, which holds at least [`Code::stack`] values.
    fn run(&self, stack: &mut [f32], vars: &mut Vec<f32>, rng: &mut Rng, queries: &Queries) -> f32 {
        let mut temps = [f32::NAN; MAX_TEMPS];
        let mut sp = 0usize;
        let mut pc = 0usize;
        while let Some(&inst) = self.insts.get(pc) {
            pc += 1;
            match inst {
                Inst::Num(value) => {
                    stack[sp] = value;
                    sp += 1;
                }
                Inst::Var(slot) => {
                    stack[sp] = read_var(vars, slot);
                    sp += 1;
                }
                Inst::Temp(slot) => {
                    let value = temps[slot as usize];
                    stack[sp] = if value.is_nan() { 0.0 } else { value };
                    sp += 1;
                }
                Inst::Query(query) => {
                    stack[sp] = match query {
                        QueryRef::IsInWater => queries.is_in_water,
                        QueryRef::IsInWaterOrRain => queries.is_in_water_or_rain,
                        QueryRef::IsBaby => queries.is_baby,
                        QueryRef::GroundSpeed => queries.ground_speed,
                        QueryRef::FrameAlpha => queries.frame_alpha,
                        QueryRef::SpellColor(component) => queries.spell_color[component as usize],
                        QueryRef::Named(index) => queries
                            .actor
                            .as_ref()
                            .and_then(|values| values.get(self.names[index as usize].as_ref()))
                            .copied()
                            .unwrap_or(0.0),
                    };
                    sp += 1;
                }
                Inst::Neg => stack[sp - 1] = -stack[sp - 1],
                Inst::Not => stack[sp - 1] = f32::from(stack[sp - 1] == 0.0),
                Inst::Bin(op) => {
                    sp -= 1;
                    let (a, b) = (stack[sp - 1], stack[sp]);
                    stack[sp - 1] = clean(match op {
                        Op::Add => a + b,
                        Op::Sub => a - b,
                        Op::Mul => a * b,
                        Op::Div => {
                            if b == 0.0 {
                                0.0
                            } else {
                                a / b
                            }
                        }
                        Op::Lt => f32::from(a < b),
                        Op::Le => f32::from(a <= b),
                        Op::Gt => f32::from(a > b),
                        Op::Ge => f32::from(a >= b),
                        Op::Eq => f32::from(a == b),
                        Op::Ne => f32::from(a != b),
                        Op::And | Op::Or => 0.0,
                    });
                }
                Inst::AndSkip(to) => {
                    if stack[sp - 1] != 0.0 {
                        sp -= 1;
                    } else {
                        stack[sp - 1] = 0.0;
                        pc = to as usize;
                    }
                }
                Inst::OrSkip(to) => {
                    if stack[sp - 1] != 0.0 {
                        stack[sp - 1] = 1.0;
                        pc = to as usize;
                    } else {
                        sp -= 1;
                    }
                }
                Inst::Truthy => stack[sp - 1] = f32::from(stack[sp - 1] != 0.0),
                Inst::JumpIfZero(to) => {
                    sp -= 1;
                    if stack[sp] == 0.0 {
                        pc = to as usize;
                    }
                }
                Inst::Jump(to) => pc = to as usize,
                Inst::CoalesceVar(slot, to) => {
                    if let Some(&value) = vars.get(slot as usize)
                        && !value.is_nan()
                    {
                        stack[sp] = value;
                        sp += 1;
                        pc = to as usize;
                    }
                }
                Inst::CoalesceTemp(slot, to) => {
                    let value = temps[slot as usize];
                    if !value.is_nan() {
                        stack[sp] = value;
                        sp += 1;
                        pc = to as usize;
                    }
                }
                Inst::Call(func, count) => {
                    let count = count as usize;
                    sp -= count;
                    let mut values = [0.0f32; 4];
                    values[..count].copy_from_slice(&stack[sp..sp + count]);
                    stack[sp] = clean(call(func, values, rng));
                    sp += 1;
                }
                Inst::AssignVar(slot) => {
                    let slot_index = slot as usize;
                    if slot_index >= vars.len() {
                        vars.resize(slot_index + 1, f32::NAN);
                    }
                    vars[slot_index] = stack[sp - 1];
                    stack[sp - 1] = 0.0;
                }
                Inst::AssignTemp(slot) => {
                    temps[slot as usize] = stack[sp - 1];
                    stack[sp - 1] = 0.0;
                }
                Inst::Return => return stack[sp - 1],
                Inst::Pop => sp -= 1,
            }
        }
        stack[sp - 1]
    }
}

/// Applies a math function to its first arguments; missing arguments read 0. Uses the shared
/// vocabulary while retaining existing particle arithmetic and bounds; unknown names read 0.
fn call(func: Option<MolangFunction>, values: [f32; 4], rng: &mut Rng) -> f32 {
    use MolangFunction as F;
    let Some(func) = func else {
        return 0.0;
    };
    let [a, b, c, _] = values;
    match func {
        F::Abs => a.abs(),
        F::Acos => a.clamp(-1.0, 1.0).acos().to_degrees(),
        F::Asin => a.clamp(-1.0, 1.0).asin().to_degrees(),
        F::Atan => a.atan().to_degrees(),
        F::Atan2 => a.atan2(b).to_degrees(),
        F::Ceil => a.ceil(),
        F::Clamp => a.max(b).min(c.max(b)),
        F::Cos => a.to_radians().cos(),
        F::Exp => a.exp(),
        F::Floor => a.floor(),
        F::InverseLerp => {
            if a == b {
                0.0
            } else {
                (c - a) / (b - a)
            }
        }
        F::Lerp => a + (b - a) * c,
        F::LerpRotate => {
            let mut delta = (b - a).rem_euclid(360.0);
            if delta > 180.0 {
                delta -= 360.0;
            }
            a + delta * c
        }
        F::Ln => {
            if a > 0.0 {
                a.ln()
            } else {
                0.0
            }
        }
        F::Max => a.max(b),
        F::Min => a.min(b),
        F::Mod => {
            if b == 0.0 {
                0.0
            } else {
                a % b
            }
        }
        F::Pow => a.powf(b),
        F::Random => rng.range(a, b),
        F::RandomInteger => {
            let (low, high) = (a.min(b).ceil(), a.max(b).floor());
            (low + (rng.unit() * (high - low + 1.0)).floor()).min(high)
        }
        F::DieRoll => {
            let mut total = 0.0;
            for _ in 0..(a.clamp(0.0, 64.0) as u32) {
                total += rng.range(b, c);
            }
            total
        }
        F::Round => a.round(),
        F::Sign => {
            if a > 0.0 {
                1.0
            } else if a < 0.0 {
                -1.0
            } else {
                0.0
            }
        }
        F::Sin => a.to_radians().sin(),
        F::Sqrt => {
            if a >= 0.0 {
                a.sqrt()
            } else {
                0.0
            }
        }
        F::Trunc => a.trunc(),
        F::HermiteBlend => {
            let t = a.clamp(0.0, 1.0);
            3.0 * t * t - 2.0 * t * t * t
        }
        _ => assets::entity::molang_call(func, values, &mut || rng.unit()),
    }
}
