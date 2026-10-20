//! Compiles parsed [`Expr`] trees into evaluable [`Node`] trees, resolving names and
//! checking function arity and every type that is known before the first frame.

use std::collections::HashMap;
use std::f32::consts::PI;
use std::fmt;

use crate::{BinaryOp, Expr, UnaryOp};

use super::value::Value;

/// Name of the only named constant besides `true` and `false`.
const PI_NAME: &str = "pi";

/// Static shape of an expression. Built-ins read whole are [`Shape::Dynamic`]: their type
/// is only known when the host answers, so their checks happen at run time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    /// A float, int or bool.
    Scalar,
    /// A vector with this many components.
    Vector(u8),
    /// A whole built-in uniform.
    Dynamic,
}

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Shape::Scalar => f.write_str("a scalar"),
            Shape::Vector(len) => write!(f, "a vec{len}"),
            Shape::Dynamic => f.write_str("a built-in of unknown type"),
        }
    }
}

/// What an earlier definition's name resolves to.
#[derive(Clone, Copy, Debug)]
pub(super) enum Binding {
    /// A compiled definition: its index among compiled definitions and declared shape.
    Defined { index: usize, shape: Shape },
    /// A definition that failed to compile; referencing it is an error.
    Failed,
}

/// How a built-in input is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Access {
    /// The whole value.
    Whole,
    /// One vector component.
    Component(usize),
    /// A value that must be scalar (a matrix element requested by its dotted name).
    Scalar,
}

/// Single-argument functions, applied per component to vectors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Func1 {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Radians,
    Degrees,
    Exp,
    Exp2,
    Exp10,
    Ln,
    Log2,
    Log10,
    Sqrt,
    Abs,
    Sign,
    Floor,
    Ceil,
    Frac,
    Round,
    Rint,
}

impl Func1 {
    /// The function called `name`, excluding `atan` and `log`, whose meaning depends on
    /// the argument count.
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "sin" => Self::Sin,
            "cos" => Self::Cos,
            "tan" => Self::Tan,
            "asin" => Self::Asin,
            "acos" => Self::Acos,
            "torad" | "radians" => Self::Radians,
            "todeg" | "degrees" => Self::Degrees,
            "exp" => Self::Exp,
            "exp2" => Self::Exp2,
            "exp10" => Self::Exp10,
            "log2" => Self::Log2,
            "log10" => Self::Log10,
            "sqrt" => Self::Sqrt,
            "abs" => Self::Abs,
            "sign" | "signum" => Self::Sign,
            "floor" => Self::Floor,
            "ceil" => Self::Ceil,
            "frac" => Self::Frac,
            "round" => Self::Round,
            "rint" => Self::Rint,
            _ => return None,
        })
    }
}

/// Two-argument functions, applied per component with scalar broadcast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Func2 {
    /// `pow(x, y)`.
    Pow,
    /// `atan2(y, x)` / `atan(y, x)`.
    Atan2,
    /// `fmod(x, y)`: remainder with the divisor's sign.
    Fmod,
    /// `edge(k, x)`: 0 when `x < k`, else 1 (GLSL `step`).
    Edge,
    /// `log(base, value)`.
    LogBase,
}

/// Three-argument functions, applied per component with scalar broadcast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Func3 {
    /// `clamp(x, min, max)`.
    Clamp,
    /// `mix(x, y, a)`.
    Mix,
}

/// Variadic `min`/`max`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fold {
    Min,
    Max,
}

/// A compiled expression.
#[derive(Debug)]
pub(super) enum Node {
    Const(Value),
    /// A built-in, by index into the input list.
    Input {
        slot: usize,
        access: Access,
    },
    /// An earlier definition, by index, optionally one of its components.
    Local {
        index: usize,
        component: Option<usize>,
    },
    Neg(Box<Node>),
    Not(Box<Node>),
    Binary(BinaryOp, Box<Node>, Box<Node>),
    /// `if(c1, v1, c2, v2, ..., otherwise)`.
    If {
        branches: Vec<(Node, Node)>,
        otherwise: Box<Node>,
    },
    /// `vec2`/`vec3`/`vec4` from 2–4 scalars.
    Construct(Vec<Node>),
    Map1(Func1, Box<Node>),
    Map2(Func2, Box<[Node; 2]>),
    Map3(Func3, Box<[Node; 3]>),
    /// At least two operands.
    Fold(Fold, Vec<Node>),
    /// `between(x, min, max)`.
    Between(Box<[Node; 3]>),
    /// `equals(a, b, epsilon)`.
    Equals(Box<[Node; 3]>),
    /// `in(x, candidates...)`.
    In(Box<Node>, Vec<Node>),
    Smooth(Box<SmoothNode>),
    /// `random()` or `random(min, max)`.
    Random(Option<Box<[Node; 2]>>),
    /// `randomInt()`, `randomInt(bound)` or `randomInt(min, max)`.
    RandomInt(Option<Box<[Node; 2]>>),
}

/// `smooth([id,] value[, fadeUpTime[, fadeDownTime]])`.
#[derive(Debug)]
pub(super) struct SmoothNode {
    /// Index of this call's accumulator.
    pub(super) slot: usize,
    pub(super) value: Node,
    pub(super) fade_up: Option<Node>,
    pub(super) fade_down: Option<Node>,
}

/// A compiled node with its static shape.
type Compiled = (Node, Shape);

/// Compiles the expression of the definition named `current`.
pub(super) struct Compiler<'a> {
    /// Name of the definition being compiled.
    pub(super) current: &'a str,
    /// Every earlier definition by name.
    pub(super) bindings: &'a HashMap<String, Binding>,
    /// Built-in names read so far; new names are appended.
    pub(super) inputs: &'a mut Vec<String>,
    /// Number of independent `smooth()` call sites compiled so far.
    pub(super) smooth_count: &'a mut usize,
}

impl Compiler<'_> {
    /// Compiles `expr`, returning the node and its static shape or an error message.
    pub(super) fn compile(&mut self, expr: &Expr) -> Result<Compiled, String> {
        match expr {
            Expr::Number(value) => Ok((Node::Const(Value::Num(*value as f32)), Shape::Scalar)),
            Expr::Integer(value) => Ok((Node::Const(Value::Int(*value)), Shape::Scalar)),
            Expr::Bool(value) => Ok((Node::Const(Value::Bool(*value)), Shape::Scalar)),
            Expr::Ident(name) => self.ident(name),
            Expr::Defined(name) => Err(format!(
                "`defined {name}` is only valid in preprocessor conditions"
            )),
            Expr::Unary(op, operand) => {
                let (node, shape) = self.compile(operand)?;
                match op {
                    UnaryOp::Neg => Ok((Node::Neg(Box::new(node)), shape)),
                    UnaryOp::Not => {
                        require_scalar(shape, "the operand of `!`")?;
                        Ok((Node::Not(Box::new(node)), Shape::Scalar))
                    }
                }
            }
            Expr::Binary(op, lhs, rhs) => self.binary(*op, lhs, rhs),
            Expr::Call(name, args) => self.call(name, args),
        }
    }

    /// Resolves a (possibly dotted) name: an earlier definition, `pi`, or a built-in read
    /// whole, by component, or as a matrix element. Dependencies have already compiled.
    fn ident(&mut self, name: &str) -> Result<Compiled, String> {
        let (base, path) = match name.split_once('.') {
            Some((base, path)) => (base, Some(path)),
            None => (name, None),
        };
        if base == self.current {
            return Err(format!("`{base}` refers to itself"));
        }
        match self.bindings.get(base) {
            Some(&Binding::Defined { index, shape }) => {
                let Some(path) = path else {
                    return Ok((
                        Node::Local {
                            index,
                            component: None,
                        },
                        shape,
                    ));
                };
                let component = local_component(base, shape, path)?;
                return Ok((
                    Node::Local {
                        index,
                        component: Some(component),
                    },
                    Shape::Scalar,
                ));
            }
            Some(Binding::Failed) => {
                return Err(format!("`{base}` failed to compile"));
            }
            None => {}
        }
        let Some(path) = path else {
            if base == PI_NAME {
                return Ok((Node::Const(Value::Num(PI)), Shape::Scalar));
            }
            let slot = self.input(base);
            return Ok((
                Node::Input {
                    slot,
                    access: Access::Whole,
                },
                Shape::Dynamic,
            ));
        };
        let (input, access) = if let Some(component) = component_index(path) {
            (base, Access::Component(component))
        } else if is_matrix_element(path) {
            (name, Access::Scalar)
        } else {
            return Err(format!(
                "`.{path}` on `{base}` is neither a component (.x/.y/.z/.w, .r/.g/.b/.a, \
                 .s/.t/.p/.q, .0-.3) nor a matrix element (.<column>.<row>)"
            ));
        };
        let slot = self.input(input);
        Ok((Node::Input { slot, access }, Shape::Scalar))
    }

    /// Index of built-in `name` in the input list, appending it when new.
    fn input(&mut self, name: &str) -> usize {
        if let Some(slot) = self.inputs.iter().position(|input| input == name) {
            return slot;
        }
        self.inputs.push(name.to_owned());
        self.inputs.len() - 1
    }

    /// Arithmetic is component-wise; ordering and logic need scalars; `==`/`!=` need
    /// operands of one shape.
    fn binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> Result<Compiled, String> {
        let (lhs, lhs_shape) = self.compile(lhs)?;
        let (rhs, rhs_shape) = self.compile(rhs)?;
        let symbol = binary_symbol(op);
        let shape = match op {
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem | BinaryOp::Add | BinaryOp::Sub => {
                zip_shapes(lhs_shape, rhs_shape, symbol)?
            }
            BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge
            | BinaryOp::And
            | BinaryOp::Or => {
                let what = format!("an operand of `{symbol}`");
                require_scalar(lhs_shape, &what)?;
                require_scalar(rhs_shape, &what)?;
                Shape::Scalar
            }
            BinaryOp::Eq | BinaryOp::Ne => match (lhs_shape, rhs_shape) {
                (Shape::Dynamic, _) | (_, Shape::Dynamic) => Shape::Scalar,
                (left, right) if left == right => Shape::Scalar,
                (left, right) => {
                    return Err(format!("`{symbol}` cannot compare {left} with {right}"));
                }
            },
        };
        Ok((Node::Binary(op, Box::new(lhs), Box::new(rhs)), shape))
    }

    /// Compiles a call to a supported OptiFine/Iris function, checking its arity.
    fn call(&mut self, name: &str, args: &[Expr]) -> Result<Compiled, String> {
        match name {
            "if" => self.if_chain(args),
            "smooth" => self.smooth(args),
            "vec2" | "vec3" | "vec4" => self.construct(name, args),
            "between" | "equals" => {
                let nodes = self.scalars::<3>(name, args)?;
                let node = if name == "between" {
                    Node::Between(Box::new(nodes))
                } else {
                    Node::Equals(Box::new(nodes))
                };
                Ok((node, Shape::Scalar))
            }
            "in" => {
                check_min_arity(name, args, 2)?;
                let mut nodes = Vec::with_capacity(args.len());
                for arg in args {
                    nodes.push(self.scalar(arg, "an argument of `in`")?);
                }
                let value = nodes.remove(0);
                Ok((Node::In(Box::new(value), nodes), Shape::Scalar))
            }
            "min" | "max" => {
                check_min_arity(name, args, 2)?;
                let mut nodes = Vec::with_capacity(args.len());
                let mut shape = Shape::Scalar;
                for (position, arg) in args.iter().enumerate() {
                    let (node, arg_shape) = self.compile(arg)?;
                    shape = if position == 0 {
                        arg_shape
                    } else {
                        zip_shapes(shape, arg_shape, name)?
                    };
                    nodes.push(node);
                }
                let fold = if name == "min" { Fold::Min } else { Fold::Max };
                Ok((Node::Fold(fold, nodes), shape))
            }
            "clamp" | "mix" => {
                let [(a, sa), (b, sb), (c, sc)] = self.fixed::<3>(name, args)?;
                let shape = zip_shapes(zip_shapes(sa, sb, name)?, sc, name)?;
                let func = if name == "clamp" {
                    Func3::Clamp
                } else {
                    Func3::Mix
                };
                Ok((Node::Map3(func, Box::new([a, b, c])), shape))
            }
            "pow" => self.map2(Func2::Pow, name, args),
            "atan2" => self.map2(Func2::Atan2, name, args),
            "fmod" => self.map2(Func2::Fmod, name, args),
            "edge" => self.map2(Func2::Edge, name, args),
            "atan" if args.len() == 2 => self.map2(Func2::Atan2, name, args),
            "log" if args.len() == 2 => self.map2(Func2::LogBase, name, args),
            "atan" => self.map1(Func1::Atan, name, args),
            "log" => self.map1(Func1::Ln, name, args),
            "random" => {
                let range = match args.len() {
                    0 => None,
                    2 => Some(Box::new(self.scalars::<2>(name, args)?)),
                    found => {
                        return Err(format!("`{name}` expects 0 or 2 arguments, found {found}"));
                    }
                };
                Ok((Node::Random(range), Shape::Scalar))
            }
            "randomInt" => {
                let range = match args {
                    [] => None,
                    [bound] => Some(Box::new([Node::Const(Value::Int(0)), self.integer(bound)?])),
                    [min, max] => Some(Box::new([self.integer(min)?, self.integer(max)?])),
                    _ => {
                        return Err(format!(
                            "`{name}` expects 0, 1 or 2 arguments, found {}",
                            args.len()
                        ));
                    }
                };
                Ok((Node::RandomInt(range), Shape::Scalar))
            }
            _ => match Func1::from_name(name) {
                Some(func) => self.map1(func, name, args),
                None => Err(format!("unknown function `{name}`")),
            },
        }
    }

    /// Compiles an integer argument without rounding integer literals through a float.
    fn integer(&mut self, expr: &Expr) -> Result<Node, String> {
        if let Expr::Number(value) = expr {
            return Ok(Node::Const(Value::Int(*value as i32)));
        }
        if let Expr::Unary(UnaryOp::Neg, operand) = expr
            && let Expr::Number(value) = **operand
        {
            return Ok(Node::Const(Value::Int(-value as i32)));
        }
        self.scalar(expr, "an integer argument")
    }

    /// OptiFine multi-branch `if`: an odd number of at least 3 arguments, scalar conditions
    /// and branches of one shape.
    fn if_chain(&mut self, args: &[Expr]) -> Result<Compiled, String> {
        let arity_error = || {
            format!(
                "`if` expects an odd number of at least 3 arguments, found {}",
                args.len()
            )
        };
        let Some((otherwise, rest)) = args.split_last() else {
            return Err(arity_error());
        };
        let (pairs, leftover) = rest.as_chunks::<2>();
        if pairs.is_empty() || !leftover.is_empty() {
            return Err(arity_error());
        }
        let mut branches = Vec::with_capacity(pairs.len());
        let mut shape = None;
        for [condition, value] in pairs {
            let condition = self.scalar(condition, "an `if` condition")?;
            let (value, value_shape) = self.compile(value)?;
            shape = Some(unify_branches(shape, value_shape)?);
            branches.push((condition, value));
        }
        let (otherwise, otherwise_shape) = self.compile(otherwise)?;
        let shape = unify_branches(shape, otherwise_shape)?;
        let otherwise = Box::new(otherwise);
        Ok((
            Node::If {
                branches,
                otherwise,
            },
            shape,
        ))
    }

    /// `smooth([id,] value[, fadeUpTime[, fadeDownTime]])`. A leading numeric literal
    /// followed by more arguments is an ignored compatibility id. Every
    /// call site has its own accumulator.
    fn smooth(&mut self, args: &[Expr]) -> Result<Compiled, String> {
        let constant_id = matches!(args.first(), Some(Expr::Number(_) | Expr::Integer(_)));
        let rest = match args {
            [_, rest @ ..] if constant_id && !rest.is_empty() => rest,
            _ => args,
        };
        let (value, fades) = match rest.split_first() {
            Some((value, fades)) if fades.len() <= 2 => (value, fades),
            _ => {
                return Err(format!(
                    "`smooth` expects ([id,] value[, fadeUpTime[, fadeDownTime]]), found {} \
                     argument(s)",
                    args.len()
                ));
            }
        };
        let (value, shape) = self.compile(value)?;
        let mut fades = fades.iter();
        let fade_up = match fades.next() {
            Some(fade) => Some(self.scalar(fade, "a `smooth` fade time")?),
            None => None,
        };
        let fade_down = match fades.next() {
            Some(fade) => Some(self.scalar(fade, "a `smooth` fade time")?),
            None => None,
        };
        let slot = *self.smooth_count;
        *self.smooth_count += 1;
        let node = Node::Smooth(Box::new(SmoothNode {
            slot,
            value,
            fade_up,
            fade_down,
        }));
        Ok((node, shape))
    }

    /// `vec2`/`vec3`/`vec4` from exactly as many scalars.
    fn construct(&mut self, name: &str, args: &[Expr]) -> Result<Compiled, String> {
        let len: u8 = match name {
            "vec2" => 2,
            "vec3" => 3,
            _ => 4,
        };
        if args.len() != usize::from(len) {
            return Err(format!(
                "`{name}` expects {len} scalar arguments, found {}",
                args.len()
            ));
        }
        let mut items = Vec::with_capacity(args.len());
        for arg in args {
            items.push(self.scalar(arg, &format!("an argument of `{name}`"))?);
        }
        Ok((Node::Construct(items), Shape::Vector(len)))
    }

    /// A one-argument function applied per component.
    fn map1(&mut self, func: Func1, name: &str, args: &[Expr]) -> Result<Compiled, String> {
        let [(x, shape)] = self.fixed::<1>(name, args)?;
        Ok((Node::Map1(func, Box::new(x)), shape))
    }

    /// A two-argument function applied per component with scalar broadcast.
    fn map2(&mut self, func: Func2, name: &str, args: &[Expr]) -> Result<Compiled, String> {
        let [(a, sa), (b, sb)] = self.fixed::<2>(name, args)?;
        let shape = zip_shapes(sa, sb, name)?;
        Ok((Node::Map2(func, Box::new([a, b])), shape))
    }

    /// Compiles exactly `N` arguments of any shape.
    fn fixed<const N: usize>(
        &mut self,
        name: &str,
        args: &[Expr],
    ) -> Result<[Compiled; N], String> {
        let arity_error = || format!("`{name}` expects {N} argument(s), found {}", args.len());
        if args.len() != N {
            return Err(arity_error());
        }
        let mut compiled = Vec::with_capacity(N);
        for arg in args {
            compiled.push(self.compile(arg)?);
        }
        compiled.try_into().map_err(|_| arity_error())
    }

    /// Compiles exactly `N` scalar arguments.
    fn scalars<const N: usize>(&mut self, name: &str, args: &[Expr]) -> Result<[Node; N], String> {
        let compiled = self.fixed::<N>(name, args)?;
        let mut nodes = Vec::with_capacity(N);
        for (node, shape) in compiled {
            require_scalar(shape, &format!("an argument of `{name}`"))?;
            nodes.push(node);
        }
        nodes
            .try_into()
            .map_err(|_| format!("`{name}` expects {N} argument(s)"))
    }

    /// Compiles `expr`, which must not be a vector.
    fn scalar(&mut self, expr: &Expr, what: &str) -> Result<Node, String> {
        let (node, shape) = self.compile(expr)?;
        require_scalar(shape, what)?;
        Ok(node)
    }
}

/// Errors when `name` was called with fewer than `min` arguments.
fn check_min_arity(name: &str, args: &[Expr], min: usize) -> Result<(), String> {
    if args.len() < min {
        return Err(format!(
            "`{name}` expects at least {min} arguments, found {}",
            args.len()
        ));
    }
    Ok(())
}

/// Errors when `shape` is statically a vector where a scalar is required.
fn require_scalar(shape: Shape, what: &str) -> Result<(), String> {
    match shape {
        Shape::Vector(_) => Err(format!("{what} must be a scalar, found {shape}")),
        Shape::Scalar | Shape::Dynamic => Ok(()),
    }
}

/// Shape of a component-wise operation: vectors must agree in length and scalars
/// broadcast.
fn zip_shapes(lhs: Shape, rhs: Shape, what: &str) -> Result<Shape, String> {
    match (lhs, rhs) {
        (Shape::Vector(a), Shape::Vector(b)) if a != b => {
            Err(format!("`{what}` cannot combine {lhs} with {rhs}"))
        }
        (Shape::Vector(len), _) | (_, Shape::Vector(len)) => Ok(Shape::Vector(len)),
        (Shape::Dynamic, _) | (_, Shape::Dynamic) => Ok(Shape::Dynamic),
        (Shape::Scalar, Shape::Scalar) => Ok(Shape::Scalar),
    }
}

/// Shape of an `if` given the shape so far (`None` before the first branch) and the
/// next branch's shape.
fn unify_branches(so_far: Option<Shape>, next: Shape) -> Result<Shape, String> {
    match so_far {
        None => Ok(next),
        Some(shape) if shape == next => Ok(shape),
        Some(Shape::Dynamic) => Ok(Shape::Dynamic),
        Some(_) if next == Shape::Dynamic => Ok(Shape::Dynamic),
        Some(shape) => Err(format!(
            "`if` branches have different types: {shape} and {next}"
        )),
    }
}

/// Component index of a single-letter or digit member (`x`, `g`, `p`, `3`), if any.
fn component_index(path: &str) -> Option<usize> {
    match path {
        "x" | "r" | "s" | "0" => Some(0),
        "y" | "g" | "t" | "1" => Some(1),
        "z" | "b" | "p" | "2" => Some(2),
        "w" | "a" | "q" | "3" => Some(3),
        _ => None,
    }
}

/// Whether `path` is a matrix element path `<column>.<row>` with indices 0–3.
fn is_matrix_element(path: &str) -> bool {
    let is_index = |part: &str| matches!(part, "0" | "1" | "2" | "3");
    path.split_once('.')
        .is_some_and(|(column, row)| is_index(column) && is_index(row))
}

/// The component `path` selects on definition `name` of `shape`.
fn local_component(name: &str, shape: Shape, path: &str) -> Result<usize, String> {
    let Shape::Vector(len) = shape else {
        return Err(format!(
            "`{name}` is a scalar and has no component `.{path}`"
        ));
    };
    match component_index(path) {
        Some(index) if index < usize::from(len) => Ok(index),
        _ => Err(format!(
            "`{name}` is a vec{len} and has no component `.{path}`"
        )),
    }
}

/// Source spelling of `op`, for messages.
fn binary_symbol(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Rem => "%",
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Lt => "<",
        BinaryOp::Le => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Ge => ">=",
        BinaryOp::Eq => "==",
        BinaryOp::Ne => "!=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
    }
}
