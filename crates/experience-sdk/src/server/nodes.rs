//! Client-channel values as the pre-order nodes they travel as through the `server` world.

use super::{Scalar, ValueNode};
use crate::Value;

impl From<Scalar> for Value {
    fn from(scalar: Scalar) -> Self {
        match scalar {
            Scalar::Bool(value) => Self::Bool(value),
            Scalar::Integer(value) => Self::Integer(value),
            Scalar::Text(value) => Self::Text(value),
            Scalar::Choice(value) => Self::Choice(value),
        }
    }
}

/// `payload` as `send-client` takes it: every value in pre-order, a list or record as a header
/// counting its items, followed by them. The texts move into the nodes.
///
/// # Panics
///
/// If a list or record has more than `u32::MAX` items, which no channel admits.
pub fn nodes(payload: Vec<Value>) -> Vec<ValueNode> {
    fn push(nodes: &mut Vec<ValueNode>, value: Value) {
        let (items, header): (Vec<Value>, fn(u32) -> ValueNode) = match value {
            Value::Bool(value) => return nodes.push(ValueNode::Leaf(Scalar::Bool(value))),
            Value::Integer(value) => return nodes.push(ValueNode::Leaf(Scalar::Integer(value))),
            Value::Text(value) => return nodes.push(ValueNode::Leaf(Scalar::Text(value))),
            Value::Choice(value) => return nodes.push(ValueNode::Leaf(Scalar::Choice(value))),
            Value::List(items) => (items, ValueNode::List),
            Value::Record(items) => (items, ValueNode::Record),
        };
        let count = u32::try_from(items.len()).expect("no channel admits 2^32 items");
        nodes.push(header(count));
        for item in items {
            push(nodes, item);
        }
    }
    let mut out = Vec::with_capacity(payload.len());
    for value in payload {
        push(&mut out, value);
    }
    out
}

/// The values whose pre-order `nodes` are, as `client-message` delivers them; `None` when a
/// header counts more items than follow it.
pub fn values(nodes: Vec<ValueNode>) -> Option<Vec<Value>> {
    fn value(nodes: &mut std::vec::IntoIter<ValueNode>) -> Option<Value> {
        let (count, header): (u32, fn(Vec<Value>) -> Value) = match nodes.next()? {
            ValueNode::Leaf(scalar) => return Some(scalar.into()),
            ValueNode::List(count) => (count, Value::List),
            ValueNode::Record(count) => (count, Value::Record),
        };
        let count = usize::try_from(count).ok().filter(|&n| n <= nodes.len())?;
        let items = (0..count)
            .map(|_| value(nodes))
            .collect::<Option<Vec<_>>>()?;
        Some(header(items))
    }
    let mut nodes = nodes.into_iter();
    let mut values = Vec::new();
    while nodes.len() > 0 {
        values.push(value(&mut nodes)?);
    }
    Some(values)
}

#[cfg(test)]
mod tests {
    use super::{nodes, values};
    use crate::Value;
    use crate::server::{Scalar, ValueNode};

    /// A payload that nests lists and records, with empty ones, between top-level scalars.
    fn payload() -> Vec<Value> {
        vec![
            Value::Bool(true),
            Value::List(vec![
                Value::Record(vec![Value::Integer(-3), Value::Text("cell".to_owned())]),
                Value::Record(Vec::new()),
            ]),
            Value::List(Vec::new()),
            Value::Choice(4),
        ]
    }

    /// The nodes are the values in pre-order, each list or record a header counting its items.
    #[test]
    fn nodes_are_the_values_in_pre_order() {
        assert!(matches!(
            nodes(payload()).as_slice(),
            [
                ValueNode::Leaf(Scalar::Bool(true)),
                ValueNode::List(2),
                ValueNode::Record(2),
                ValueNode::Leaf(Scalar::Integer(-3)),
                ValueNode::Leaf(Scalar::Text(cell)),
                ValueNode::Record(0),
                ValueNode::List(0),
                ValueNode::Leaf(Scalar::Choice(4)),
            ] if cell == "cell"
        ));
        assert_eq!(values(nodes(payload())), Some(payload()));
    }

    /// A header that counts more items than follow it holds no values.
    #[test]
    fn a_header_without_its_items_is_refused() {
        let mut short = nodes(payload());
        short.pop();
        short.push(ValueNode::Record(1));
        assert_eq!(values(short), None);
        assert_eq!(values(vec![ValueNode::List(u32::MAX)]), None);
    }
}
