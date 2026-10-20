//! Runtime values of custom uniform expressions and their component-wise operations.

use super::UniformValue;

/// A value during evaluation. Float operations and vector lanes retain float precision;
/// integer inputs and random results keep their full signed range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Value {
    Bool(bool),
    Int(i32),
    Num(f32),
    /// A 2–4 component vector; components past `len` are zero.
    Vector {
        len: u8,
        items: [f32; 4],
    },
}

/// A run-time type error, such as a built-in of unexpected type. The definition being
/// evaluated falls back to its type's zero value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Mismatch;

impl Value {
    /// The value as a number (`true` is 1); `None` for a vector.
    pub(super) fn number(self) -> Option<f32> {
        match self {
            Value::Bool(value) => Some(if value { 1.0 } else { 0.0 }),
            Value::Num(value) => Some(value),
            Value::Int(value) => Some(value as f32),
            Value::Vector { .. } => None,
        }
    }

    /// The value as a Java integer cast, preserving integer inputs exactly.
    pub(super) fn integer(self) -> Option<i32> {
        match self {
            Value::Int(value) => Some(value),
            other => other.number().map(|value| value as i32),
        }
    }

    /// The value as a condition (non-zero is true); `None` for a vector.
    pub(super) fn truth(self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(value),
            Value::Num(value) => Some(value != 0.0),
            Value::Int(value) => Some(value != 0),
            Value::Vector { .. } => None,
        }
    }

    /// Component `index` of a vector; `None` for a scalar or an index past its length.
    pub(super) fn component(self, index: usize) -> Option<f32> {
        match self {
            Value::Vector { len, items } if index < usize::from(len) => items.get(index).copied(),
            _ => None,
        }
    }

    /// Component count, `None` for a scalar.
    pub(super) fn lane_count(self) -> Option<u8> {
        match self {
            Value::Vector { len, .. } => Some(len),
            Value::Bool(_) | Value::Int(_) | Value::Num(_) => None,
        }
    }

    /// The value with a boolean turned into 0 or 1.
    pub(super) fn to_numeric(self) -> Value {
        match self {
            Value::Bool(_) => self.map(|n| n),
            Value::Int(_) | Value::Num(_) | Value::Vector { .. } => self,
        }
    }

    /// Applies `f` to a scalar or to every component of a vector.
    pub(super) fn map(self, f: impl Fn(f32) -> f32) -> Value {
        let (len, items) = self.lanes();
        from_lanes(len, items.map(f))
    }

    /// Applies `f` component-wise; a scalar operand broadcasts, vectors must agree in
    /// length.
    pub(super) fn zip(self, other: Value, f: impl Fn(f32, f32) -> f32) -> Result<Value, Mismatch> {
        let (a_len, a) = self.lanes();
        let (b_len, b) = other.lanes();
        let len = common_len(a_len, b_len)?;
        Ok(from_lanes(len, std::array::from_fn(|i| f(a[i], b[i]))))
    }

    /// Three-operand [`Value::zip`].
    pub(super) fn zip3(
        a: Value,
        b: Value,
        c: Value,
        f: impl Fn(f32, f32, f32) -> f32,
    ) -> Result<Value, Mismatch> {
        let (a_len, a) = a.lanes();
        let (b_len, b) = b.lanes();
        let (c_len, c) = c.lanes();
        let len = common_len(common_len(a_len, b_len)?, c_len)?;
        Ok(from_lanes(
            len,
            std::array::from_fn(|i| f(a[i], b[i], c[i])),
        ))
    }

    /// `==`: scalars compare as numbers, vectors of one length compare every component.
    pub(super) fn equals(self, other: Value) -> Result<bool, Mismatch> {
        match (self, other) {
            (Value::Vector { len: a, items: x }, Value::Vector { len: b, items: y }) if a == b => {
                Ok(x == y)
            }
            (Value::Vector { .. }, _) | (_, Value::Vector { .. }) => Err(Mismatch),
            (Value::Int(a), Value::Int(b)) => Ok(a == b),
            (a, b) => Ok(a.number().zip(b.number()).is_some_and(|(a, b)| a == b)),
        }
    }

    /// Component count (`None` for a scalar) and components; a scalar fills every lane.
    fn lanes(self) -> (Option<u8>, [f32; 4]) {
        match self {
            Value::Vector { len, items } => (Some(len), items),
            Value::Bool(_) | Value::Int(_) | Value::Num(_) => {
                (None, [self.number().unwrap_or(0.0); 4])
            }
        }
    }
}

/// Builds a scalar (`len` is `None`) or a vector, zeroing components past `len`.
pub(super) fn from_lanes(len: Option<u8>, items: [f32; 4]) -> Value {
    match len {
        None => Value::Num(items[0]),
        Some(len) => Value::Vector {
            len,
            items: std::array::from_fn(|i| if i < usize::from(len) { items[i] } else { 0.0 }),
        },
    }
}

/// Length of a component-wise result; `None` (scalar) only when both operands are scalar.
fn common_len(a: Option<u8>, b: Option<u8>) -> Result<Option<u8>, Mismatch> {
    match (a, b) {
        (Some(a), Some(b)) if a != b => Err(Mismatch),
        (Some(len), _) | (_, Some(len)) => Ok(Some(len)),
        (None, None) => Ok(None),
    }
}

impl From<UniformValue> for Value {
    fn from(value: UniformValue) -> Self {
        let vector = |items: &[f32]| {
            let mut lanes = [0.0; 4];
            for (lane, item) in lanes.iter_mut().zip(items) {
                *lane = *item;
            }
            Value::Vector {
                len: items.len() as u8,
                items: lanes,
            }
        };
        match value {
            UniformValue::Bool(value) => Value::Bool(value),
            UniformValue::Int(value) => Value::Int(value),
            UniformValue::Float(value) => Value::Num(value),
            UniformValue::Vec2(items) => vector(&items),
            UniformValue::Vec3(items) => vector(&items),
            UniformValue::Vec4(items) => vector(&items),
        }
    }
}
