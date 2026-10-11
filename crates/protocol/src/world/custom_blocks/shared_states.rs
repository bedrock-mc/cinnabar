//! Shared canonical state identities and aggregate admission work bounds.

use std::{
    collections::HashMap,
    ops::Deref,
    sync::{Arc, Mutex, OnceLock, Weak},
};

use super::{CustomBlock, CustomBlockVisuals, CustomHashedState, CustomStateValue};

const MAX_AGGREGATE_STATES: usize = super::MAX_STATES_PER_BLOCK as usize;
const MAX_AGGREGATE_STATE_BYTES: usize = 64 * 1024 * 1024;
const MAX_CACHED_DEFINITIONS: usize = MAX_AGGREGATE_STATES;

/// An immutable palette-order state collection shared by registry consumers.
#[derive(Debug, Clone)]
pub struct SharedStates(Arc<[CustomHashedState]>);

impl Deref for SharedStates {
    type Target = [CustomHashedState];
    /// Borrows canonical records without rehashing or copying their state values.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// An owning iterator whose records retain the shared value arrays.
pub struct StateIter {
    states: SharedStates,
    next: usize,
}

impl Iterator for StateIter {
    type Item = CustomHashedState;
    /// Copies a hash and a shared value handle, leaving canonical storage immutable.
    fn next(&mut self) -> Option<Self::Item> {
        let state = self.states.get(self.next)?.clone();
        self.next += 1;
        Some(state)
    }

    /// Reports the remaining records so consumers reserve their output storage once.
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.states.len() - self.next;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for StateIter {}
impl std::iter::FusedIterator for StateIter {}

impl IntoIterator for SharedStates {
    type Item = CustomHashedState;
    type IntoIter = StateIter;
    /// Retains canonical storage for the whole iteration.
    fn into_iter(self) -> Self::IntoIter {
        StateIter {
            states: self,
            next: 0,
        }
    }
}

struct Entry {
    _visual: Weak<CustomBlockVisuals>,
    _name: Weak<str>,
    states: SharedStates,
    bytes: usize,
    touched: u64,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<(usize, usize), Entry>,
    states: usize,
    bytes: usize,
    clock: u64,
}

/// Computes a conservative bound on retained values and repeated canonical hashing work.
fn state_bytes(name: &str, visual: &CustomBlockVisuals, count: usize) -> Option<usize> {
    let mut per_state = name
        .len()
        .checked_add(std::mem::size_of::<CustomHashedState>())?;
    for axis in &visual.state_axes {
        let largest = axis
            .values
            .iter()
            .map(|value| match value {
                CustomStateValue::String(text) => text.len(),
                _ => 0,
            })
            .max()
            .unwrap_or(0);
        per_state = per_state
            .checked_add(axis.name.len())?
            .checked_add(largest)?
            .checked_add(std::mem::size_of::<CustomStateValue>())?;
    }
    per_state.checked_mul(count)
}

/// Resolves a source identity; weak handles prevent address reuse and force writes to detach.
pub(super) fn resolve(block: &CustomBlock) -> SharedStates {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let key = (
        Arc::as_ptr(&block.visual).addr(),
        block.name.as_ptr().addr(),
    );
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.clock = cache.clock.wrapping_add(1);
    let touched = cache.clock;
    if let Some(entry) = cache.entries.get_mut(&key) {
        entry.touched = touched;
        return entry.states.clone();
    }
    let count = super::axis_combinations(&block.visual.state_axes).unwrap_or(0) as usize;
    let Some(bytes) = state_bytes(&block.name, &block.visual, count)
        .filter(|bytes| *bytes <= MAX_AGGREGATE_STATE_BYTES)
    else {
        return SharedStates(Arc::default());
    };
    let states = SharedStates(block.compile_states());
    while cache.entries.len() >= MAX_CACHED_DEFINITIONS
        || cache.states + states.len() > MAX_AGGREGATE_STATES
        || cache.bytes + bytes > MAX_AGGREGATE_STATE_BYTES
    {
        let Some(oldest) = cache
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.touched)
            .map(|(key, _)| *key)
        else {
            break;
        };
        if let Some(entry) = cache.entries.remove(&oldest) {
            cache.states -= entry.states.len();
            cache.bytes -= entry.bytes;
        }
    }
    cache.states += states.len();
    cache.bytes += bytes;
    cache.entries.insert(
        key,
        Entry {
            _visual: Arc::downgrade(&block.visual),
            _name: Arc::downgrade(&block.name),
            states: states.clone(),
            bytes,
            touched,
        },
    );
    states
}

#[derive(Default)]
pub(super) struct AdmissionBudget {
    states: usize,
    bytes: usize,
}

impl AdmissionBudget {
    /// Charges aggregate work before any consumer expands a custom definition's palette.
    pub fn admit(&mut self, name: &str, visual: &CustomBlockVisuals, count: u32) -> bool {
        let Some(states) = self
            .states
            .checked_add(count as usize)
            .filter(|states| *states <= MAX_AGGREGATE_STATES)
        else {
            return false;
        };
        let Some(bytes) = state_bytes(name, visual, count as usize)
            .and_then(|bytes| self.bytes.checked_add(bytes))
            .filter(|bytes| *bytes <= MAX_AGGREGATE_STATE_BYTES)
        else {
            return false;
        };
        self.states = states;
        self.bytes = bytes;
        true
    }
}

#[cfg(test)]
mod tests;
