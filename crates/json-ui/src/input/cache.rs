//! Input declarations are pure in the control's immutable property map.

use std::cell::{OnceCell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Weak;

use serde_json::Value;

use super::{FocusMeta, InputComponent};
use crate::lru::Lru;
use crate::tree::Properties;

pub(super) struct Entry {
    owner: Weak<BTreeMap<String, Value>>,
    pub(super) input: OnceCell<InputComponent>,
    pub(super) mappings: OnceCell<InputComponent>,
    pub(super) focus: OnceCell<FocusMeta>,
}

/// Bound metadata retained across screen and pack replacements.
const MAX_INPUT_MAPS: usize = 4096;

thread_local! {
    static CACHE: RefCell<Lru<usize, Rc<Entry>>> = RefCell::new(Lru::new(MAX_INPUT_MAPS));
}

/// Share parsed metadata until copy-on-write replaces the property map.
pub(super) fn entry(properties: &Properties) -> Rc<Entry> {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let key = properties.identity();
        if let Some(entry) = cache.get(&key) {
            return Rc::clone(entry);
        }
        if cache.len() >= MAX_INPUT_MAPS {
            cache.retain(|entry| entry.owner.strong_count() != 0);
        }
        let entry = Rc::new(Entry {
            owner: properties.weak(),
            input: OnceCell::new(),
            mappings: OnceCell::new(),
            focus: OnceCell::new(),
        });
        cache.insert(key, Rc::clone(&entry));
        entry
    })
}

#[cfg(test)]
mod tests;
