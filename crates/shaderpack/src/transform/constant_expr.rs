//! Scalar constant evaluation for array bounds, without evaluating user functions.

use crate::token::{Tok, TokKind};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Scalar {
    Int,
    Uint,
    Float,
    Double,
    Bool,
}

impl Scalar {
    /// Recognizes the scalar types supported by the array-bound folder.
    pub(super) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "int" => Self::Int,
            "uint" => Self::Uint,
            "float" => Self::Float,
            "double" => Self::Double,
            "bool" => Self::Bool,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Value {
    Int(i32),
    Uint(u32),
    Float(f32),
    Double(f64),
    Bool(bool),
}

impl Value {
    /// Converts a constant initializer using GLSL's implicit scalar conversions.
    pub(super) fn assign(self, ty: Scalar) -> Option<Self> {
        match (self, ty) {
            (Self::Int(_), Scalar::Int)
            | (Self::Uint(_), Scalar::Uint)
            | (Self::Float(_), Scalar::Float)
            | (Self::Double(_), Scalar::Double)
            | (Self::Bool(_), Scalar::Bool)
            | (Self::Int(_), Scalar::Uint)
            | (Self::Int(_) | Self::Uint(_), Scalar::Float)
            | (Self::Int(_) | Self::Uint(_) | Self::Float(_), Scalar::Double) => self.cast(ty),
            _ => None,
        }
    }

    /// Evaluates an explicit scalar constructor; undefined float-to-integer results stay unknown.
    fn cast(self, ty: Scalar) -> Option<Self> {
        let value = match ty {
            Scalar::Int => Self::Int(match self {
                Self::Int(v) => v,
                Self::Uint(v) => v as i32,
                Self::Bool(v) => i32::from(v),
                Self::Float(v)
                    if (f64::from(i32::MIN)..f64::from(i32::MAX) + 1.0).contains(&f64::from(v)) =>
                {
                    v as i32
                }
                Self::Double(v)
                    if (f64::from(i32::MIN)..f64::from(i32::MAX) + 1.0).contains(&v) =>
                {
                    v as i32
                }
                _ => return None,
            }),
            Scalar::Uint => Self::Uint(match self {
                Self::Int(v) => v as u32,
                Self::Uint(v) => v,
                Self::Bool(v) => u32::from(v),
                Self::Float(v) if (0.0..f64::from(u32::MAX) + 1.0).contains(&f64::from(v)) => {
                    v as u32
                }
                Self::Double(v) if (0.0..f64::from(u32::MAX) + 1.0).contains(&v) => v as u32,
                _ => return None,
            }),
            Scalar::Float => Self::Float(match self {
                Self::Int(v) => v as f32,
                Self::Uint(v) => v as f32,
                Self::Float(v) => v,
                Self::Double(v) => v as f32,
                Self::Bool(v) => u32::from(v) as f32,
            }),
            Scalar::Double => Self::Double(match self {
                Self::Int(v) => f64::from(v),
                Self::Uint(v) => f64::from(v),
                Self::Float(v) => f64::from(v),
                Self::Double(v) => v,
                Self::Bool(v) => f64::from(u32::from(v)),
            }),
            Scalar::Bool => Self::Bool(match self {
                Self::Int(v) => v != 0,
                Self::Uint(v) => v != 0,
                Self::Float(v) => v != 0.0,
                Self::Double(v) => v != 0.0,
                Self::Bool(v) => v,
            }),
        };
        value.finite()
    }

    /// Returns a positive integral bound with its original integer type.
    pub(super) fn array_literal(self) -> Option<String> {
        match self {
            Self::Int(v) if v > 0 => Some(v.to_string()),
            Self::Uint(v) if v > 0 => Some(format!("{v}u")),
            _ => None,
        }
    }

    /// Rejects non-finite intermediate values instead of inventing a portable result.
    fn finite(self) -> Option<Self> {
        match self {
            Self::Float(v) if !v.is_finite() => None,
            Self::Double(v) if !v.is_finite() => None,
            _ => Some(self),
        }
    }

    /// Parses one scalar literal, retaining signedness and 32-bit float rounding.
    fn literal(token: &Tok) -> Option<Self> {
        Some(match &token.kind {
            TokKind::Int(text) => {
                let digits = text.trim_end_matches(['u', 'U']);
                let (digits, radix) = if let Some(hex) = digits
                    .strip_prefix("0x")
                    .or_else(|| digits.strip_prefix("0X"))
                {
                    (hex, 16)
                } else if digits.len() > 1 && digits.starts_with('0') {
                    (digits, 8)
                } else {
                    (digits, 10)
                };
                let value = u32::from_str_radix(digits, radix).ok()?;
                if text.ends_with(['u', 'U']) {
                    Self::Uint(value)
                } else {
                    Self::Int(value as i32)
                }
            }
            TokKind::Float(text) => {
                if let Some(digits) = text.strip_suffix("lf").or_else(|| text.strip_suffix("LF")) {
                    Self::Double(digits.parse().ok()?)
                } else {
                    Self::Float(text.trim_end_matches(['f', 'F']).parse().ok()?)
                }
            }
            TokKind::Ident(text) if text == "true" || text == "false" => Self::Bool(text == "true"),
            _ => return None,
        })
        .finite()
    }
}

/// Promotes two scalar operands to their common GLSL type, without converting booleans.
fn promote(a: Value, b: Value) -> Option<(Value, Value)> {
    use Value::*;
    let ty = match (a, b) {
        (Bool(_), Bool(_)) => Scalar::Bool,
        (Bool(_), _) | (_, Bool(_)) => return None,
        (Double(_), _) | (_, Double(_)) => Scalar::Double,
        (Float(_), _) | (_, Float(_)) => Scalar::Float,
        (Uint(_), _) | (_, Uint(_)) => Scalar::Uint,
        _ => Scalar::Int,
    };
    Some((a.assign(ty)?, b.assign(ty)?))
}

/// Applies a supported pure unary operator to a typed value.
fn unary(op: &str, value: Value) -> Option<Value> {
    use Value::*;
    Some(match (op, value) {
        ("+", Int(_) | Uint(_) | Float(_) | Double(_)) => value,
        ("-", Int(v)) => Int(v.wrapping_neg()),
        ("-", Uint(v)) => Uint(v.wrapping_neg()),
        ("-", Float(v)) => Float(-v),
        ("-", Double(v)) => Double(-v),
        ("~", Int(v)) => Int(!v),
        ("~", Uint(v)) => Uint(!v),
        ("!", Bool(v)) => Bool(!v),
        _ => return None,
    })
}

/// Applies arithmetic, comparisons and logical operations using GLSL scalar promotions.
fn binary(op: &str, a: Value, b: Value) -> Option<Value> {
    use Value::*;
    if matches!(op, "<<" | ">>") {
        let shift = match b {
            Int(v) => u32::try_from(v).ok()?,
            Uint(v) => v,
            _ => return None,
        };
        return Some(match (op, a) {
            ("<<", Int(v)) => Int(v.checked_shl(shift)?),
            (">>", Int(v)) => Int(v.checked_shr(shift)?),
            ("<<", Uint(v)) => Uint(v.checked_shl(shift)?),
            (">>", Uint(v)) => Uint(v.checked_shr(shift)?),
            _ => return None,
        });
    }
    let (a, b) = promote(a, b)?;
    macro_rules! comparison {
        ($a:expr, $b:expr) => {
            match op {
                "==" => Bool($a == $b),
                "!=" => Bool($a != $b),
                "<" => Bool($a < $b),
                "<=" => Bool($a <= $b),
                ">" => Bool($a > $b),
                ">=" => Bool($a >= $b),
                _ => return None,
            }
        };
    }
    macro_rules! integer {
        ($a:expr, $b:expr, $kind:ident) => {
            match op {
                "+" => $kind($a.wrapping_add($b)),
                "-" => $kind($a.wrapping_sub($b)),
                "*" => $kind($a.wrapping_mul($b)),
                "/" => $kind($a.checked_div($b)?),
                "%" => $kind($a.checked_rem($b)?),
                "&" => $kind($a & $b),
                "^" => $kind($a ^ $b),
                "|" => $kind($a | $b),
                _ => comparison!($a, $b),
            }
        };
    }
    match (a, b) {
        (Int(a), Int(b)) => {
            if op == "%" && (a < 0 || b < 0) {
                return None;
            }
            Some(integer!(a, b, Int))
        }
        (Uint(a), Uint(b)) => Some(integer!(a, b, Uint)),
        (Float(a), Float(b)) => Some(match op {
            "+" => Float(a + b),
            "-" => Float(a - b),
            "*" => Float(a * b),
            "/" => Float(a / b),
            _ => comparison!(a, b),
        })
        .and_then(Value::finite),
        (Double(a), Double(b)) => Some(match op {
            "+" => Double(a + b),
            "-" => Double(a - b),
            "*" => Double(a * b),
            "/" => Double(a / b),
            _ => comparison!(a, b),
        })
        .and_then(Value::finite),
        (Bool(a), Bool(b)) => Some(Bool(match op {
            "&&" => a && b,
            "||" => a || b,
            "^^" | "!=" => a != b,
            "==" => a == b,
            _ => return None,
        })),
        _ => None,
    }
}

enum Operator<'a> {
    Group(Option<Scalar>, usize),
    Unary(&'a str),
    Binary(&'a str),
    Question,
    Select,
}

impl Operator<'_> {
    /// Returns precedence for operators accepted in scalar constant expressions.
    fn precedence(&self) -> u8 {
        match self {
            Self::Unary(_) => 13,
            Self::Binary(op) => match *op {
                "*" | "/" | "%" => 12,
                "+" | "-" => 11,
                "<<" | ">>" => 10,
                "<" | "<=" | ">" | ">=" => 9,
                "==" | "!=" => 8,
                "&" => 7,
                "^" => 6,
                "|" => 5,
                "&&" => 4,
                "^^" => 3,
                "||" => 2,
                _ => 0,
            },
            Self::Select => 1,
            _ => 0,
        }
    }

    /// Reduces one operator, requiring every operand to be a proven constant.
    fn apply(self, values: &mut Vec<Value>) -> Option<()> {
        let b = values.pop()?;
        let value = match self {
            Self::Unary(op) => unary(op, b)?,
            Self::Binary(op) => binary(op, values.pop()?, b)?,
            Self::Select => {
                let (a, b) = promote(values.pop()?, b)?;
                let Value::Bool(condition) = values.pop()? else {
                    return None;
                };
                if condition { a } else { b }
            }
            _ => return None,
        };
        values.push(value);
        Some(())
    }
}

/// Folds a scalar expression using explicit stacks, so nesting never recurses on the host stack.
/// Unsupported syntax, runtime names, calls and undefined arithmetic remain unresolved.
pub(super) fn evaluate(tokens: &[Tok], lookup: impl Fn(usize) -> Option<Value>) -> Option<Value> {
    let mut values = Vec::new();
    let mut operators = Vec::new();
    let mut operand = true;
    let mut i = 0;
    while let Some(token) = tokens.get(i) {
        if operand {
            let cast = token.ident().and_then(Scalar::parse);
            if token.is("(") || cast.is_some() && tokens.get(i + 1).is_some_and(|t| t.is("(")) {
                operators.push(Operator::Group(cast, values.len()));
                i += usize::from(cast.is_some());
            } else if matches!(token.text(), "+" | "-" | "!" | "~") {
                operators.push(Operator::Unary(token.text()));
            } else {
                values.push(Value::literal(token).or_else(|| lookup(i))?);
                operand = false;
            }
        } else if token.is(")") {
            loop {
                let operator = operators.pop()?;
                if let Operator::Group(cast, base) = operator {
                    if values.len() != base + 1 {
                        return None;
                    }
                    if let Some(ty) = cast {
                        let value = values.pop()?.cast(ty)?;
                        values.push(value);
                    }
                    break;
                }
                operator.apply(&mut values)?;
            }
        } else if token.is(":") {
            loop {
                match operators.pop()? {
                    Operator::Question => break,
                    operator => operator.apply(&mut values)?,
                }
            }
            operators.push(Operator::Select);
            operand = true;
        } else {
            let operator = if token.is("?") {
                Operator::Question
            } else {
                Operator::Binary(token.text())
            };
            let precedence = if token.is("?") {
                1
            } else {
                operator.precedence()
            };
            if precedence == 0 {
                return None;
            }
            while operators.last().is_some_and(|prior| {
                prior.precedence() > precedence
                    || !token.is("?") && prior.precedence() == precedence
            }) {
                operators.pop()?.apply(&mut values)?;
            }
            operators.push(operator);
            operand = true;
        }
        i += 1;
    }
    if operand {
        return None;
    }
    while let Some(operator) = operators.pop() {
        operator.apply(&mut values)?;
    }
    (values.len() == 1).then(|| values[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::lex;

    #[test]
    fn scalar_expressions_keep_types_precedence_and_rounding() {
        use Value::*;
        for (source, expected) in [
            ("2 + 3 * 4", Int(14)),
            ("(2 + 3) * 4", Int(20)),
            ("7 / 2", Int(3)),
            ("float(2) / 4", Float(0.5)),
            ("int(float(2) / 4 * 8)", Int(4)),
            ("16777216.0 + 1.0 - 16777216.0", Float(0.0)),
            ("0x10u + 010", Uint(24)),
            ("1u + -1", Uint(0)),
            ("-2 >> 1u", Int(-1)),
            ("(1 << 3) | 2", Int(10)),
            ("~0u & 7u", Uint(7)),
            ("9 % 4", Int(1)),
            ("!false && 2 < 3 ^^ false || false", Bool(true)),
            ("true ? 2 : 4u", Uint(2)),
            ("false ? 1 : true ? 2 : 3", Int(2)),
            ("true ? false ? 1 : 2 : 3", Int(2)),
            ("uint(true) + int(2.75)", Uint(3)),
            ("bool(0.0) == false", Bool(true)),
            ("double(16777216) + 1 - 16777216", Double(1.0)),
            ("1.0lf / 4", Double(0.25)),
        ] {
            assert_eq!(evaluate(&lex(source), |_| None), Some(expected), "{source}");
        }
        assert_eq!(Value::Int(2).assign(Scalar::Float), Some(Float(2.0)));
        assert_eq!(Value::Int(2).assign(Scalar::Bool), None);
        assert_eq!(Value::Float(2.0).assign(Scalar::Int), None);
    }

    #[test]
    fn runtime_and_undefined_expressions_remain_unresolved() {
        for source in [
            "next()",
            "a + a",
            "1 / 0",
            "1.0 / 0.0",
            "1 << 32",
            "1 >> -1",
            "true + 1",
            "2 % -1",
            "int(1e20)",
            "true ? 2 : next()",
            "1, 2",
            "value.member",
            "value[0]",
            "++value",
            "1)",
            "(1",
            "int()",
            "1 +",
            "1 2",
        ] {
            assert_eq!(evaluate(&lex(source), |_| None), None, "{source}");
        }
    }

    #[test]
    fn nested_constant_expressions_use_heap_stacks() {
        let source = format!("{}1{}", "(".repeat(4096), ")".repeat(4096));
        assert_eq!(evaluate(&lex(&source), |_| None), Some(Value::Int(1)));
    }
}
