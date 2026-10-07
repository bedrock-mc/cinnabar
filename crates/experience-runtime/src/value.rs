//! Client-channel values between the protocol's tree and server WIT 0.3's pre-order nodes.
//!
//! WIT types cannot nest, so a record crosses into and out of the guest as its values in
//! pre-order: a scalar is a leaf node, and a list or record is a header holding its item count,
//! followed by that many values. That is the layout of MessagePack and CBOR arrays. Unlike a node
//! table with child indices it cannot share a node or form a cycle, so decoding has one check, a
//! header counting more items than follow it, besides the depth limit.

use crate::host::cinnabar::experience_server::types::{Scalar as Leaf, ValueNode};
use crate::limits::MAX_VALUE_DEPTH;
use crate::protocol::Scalar;

/// Why nodes hold no payload.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// Lists and records nest deeper than [`MAX_VALUE_DEPTH`].
    TooDeep,
    /// A header counts more items than follow it.
    Malformed,
}

/// The values whose pre-order `nodes` are, nested at most [`MAX_VALUE_DEPTH`] deep.
pub(crate) fn decode(nodes: Vec<ValueNode>) -> Result<Vec<Scalar>, Refusal> {
    fn value(nodes: &mut std::vec::IntoIter<ValueNode>, depth: usize) -> Result<Scalar, Refusal> {
        let (count, header): (u32, fn(Vec<Scalar>) -> Scalar) =
            match nodes.next().ok_or(Refusal::Malformed)? {
                ValueNode::Leaf(leaf) => return Ok(leaf.into()),
                ValueNode::List(count) => (count, Scalar::List),
                ValueNode::Record(count) => (count, Scalar::Record),
            };
        if depth == 0 {
            return Err(Refusal::TooDeep);
        }
        // Checked before anything is allocated for the items.
        let count = usize::try_from(count)
            .ok()
            .filter(|&count| count <= nodes.len())
            .ok_or(Refusal::Malformed)?;
        let items = (0..count)
            .map(|_| value(nodes, depth - 1))
            .collect::<Result<_, _>>()?;
        Ok(header(items))
    }
    let mut nodes = nodes.into_iter();
    let mut values = Vec::new();
    while nodes.len() > 0 {
        values.push(value(&mut nodes, MAX_VALUE_DEPTH)?);
    }
    Ok(values)
}

/// The pre-order nodes of `values`.
pub(crate) fn encode(values: &[Scalar]) -> Vec<ValueNode> {
    fn push(nodes: &mut Vec<ValueNode>, value: &Scalar) {
        let (items, header): (&[Scalar], fn(u32) -> ValueNode) = match value {
            Scalar::List(items) => (items, ValueNode::List),
            Scalar::Record(items) => (items, ValueNode::Record),
            Scalar::Bool(value) => return nodes.push(ValueNode::Leaf(Leaf::Bool(*value))),
            Scalar::Integer(value) => return nodes.push(ValueNode::Leaf(Leaf::Integer(*value))),
            Scalar::Text(value) => return nodes.push(ValueNode::Leaf(Leaf::Text(value.clone()))),
            Scalar::Choice(value) => return nodes.push(ValueNode::Leaf(Leaf::Choice(*value))),
        };
        // A payload that arrived in one IPC frame holds far fewer than 2^32 items.
        nodes.push(header(items.len() as u32));
        for item in items {
            push(nodes, item);
        }
    }
    let mut nodes = Vec::with_capacity(values.len());
    for value in values {
        push(&mut nodes, value);
    }
    nodes
}

/// How deep lists and records nest in `values`, a top-level one being level 1; 0 for scalars
/// alone.
pub(crate) fn depth(values: &[Scalar]) -> usize {
    values
        .iter()
        .map(|value| match value {
            Scalar::List(items) | Scalar::Record(items) => 1 + depth(items),
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

/// `value` as a WIT leaf; `None` for a list or record.
pub(crate) fn leaf_of(value: &Scalar) -> Option<Leaf> {
    Some(match value {
        Scalar::Bool(value) => Leaf::Bool(*value),
        Scalar::Integer(value) => Leaf::Integer(*value),
        Scalar::Text(value) => Leaf::Text(value.clone()),
        Scalar::Choice(value) => Leaf::Choice(*value),
        Scalar::List(_) | Scalar::Record(_) => return None,
    })
}

impl From<Leaf> for Scalar {
    fn from(leaf: Leaf) -> Self {
        match leaf {
            Leaf::Bool(value) => Self::Bool(value),
            Leaf::Integer(value) => Self::Integer(value),
            Leaf::Text(value) => Self::Text(value),
            Leaf::Choice(value) => Self::Choice(value),
        }
    }
}
