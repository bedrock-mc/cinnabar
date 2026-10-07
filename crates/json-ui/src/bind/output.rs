//! Reuse bound properties while their bag, component values and template agree.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

use serde_json::Value;

use super::Node;
use super::bag::Bag;
use super::source::Patch;
use crate::lru::Lru;
use crate::tree::Properties;

struct Entry {
    owner: Weak<BTreeMap<String, Value>>,
    own: Bag,
    native: BTreeMap<String, Value>,
    patch: Option<Arc<Patch>>,
    properties: Properties,
}

/// Bound memory even when many different screens or collection rows pass through.
const MAX_OUTPUTS: usize = 4096;

thread_local! {
    static CACHE: RefCell<Lru<(usize, u64), Entry>> = RefCell::new(Lru::new(MAX_OUTPUTS));
}

/// The prior output if every input to this control's property baking is unchanged.
pub(super) fn get(node: &Node) -> Option<Properties> {
    let key = (node.src.get().properties.identity(), node.key);
    CACHE.with(|cache| {
        let cache = cache.borrow();
        let entry = cache.get(&key)?;
        (entry.own == node.own
            && entry.native == node.native.props
            && entry.patch == node.src.patch)
            .then(|| entry.properties.clone())
    })
}

/// Save a changed control's inputs and shared property output.
pub(super) fn put(node: &Node, properties: Properties) {
    let key = (node.src.get().properties.identity(), node.key);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= MAX_OUTPUTS {
            cache.retain(|entry| entry.owner.strong_count() != 0);
        }
        cache.insert(
            key,
            Entry {
                owner: node.src.get().properties.weak(),
                own: node.own.clone(),
                native: node.native.props.clone(),
                patch: node.src.patch.clone(),
                properties,
            },
        );
    });
}

#[cfg(test)]
mod tests;
