//! Shared canonical state identities and aggregate admission work bounds.

use std::{
    collections::{HashMap, VecDeque},
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

impl SharedStates {
    /// Shares an empty result without retaining unsupported definition sources.
    fn empty() -> Self {
        static EMPTY: OnceLock<SharedStates> = OnceLock::new();
        EMPTY.get_or_init(|| Self(Arc::default())).clone()
    }
}

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
}

#[derive(Default)]
struct Cache {
    entries: HashMap<(usize, usize), Entry>,
    states: usize,
    bytes: usize,
    eviction_order: VecDeque<(usize, usize)>,
    #[cfg(test)]
    eviction_visits: usize,
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
    if block.visual.state_identity_incomplete || block.visual.materialization_disabled {
        return SharedStates::empty();
    }
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.resolve(block)
}

impl Cache {
    /// Shares cached identities and evicts oldest admissions in amortized constant time.
    fn resolve(&mut self, block: &CustomBlock) -> SharedStates {
        let key = (
            Arc::as_ptr(&block.visual).addr(),
            block.name.as_ptr().addr(),
        );
        if let Some(entry) = self.entries.get(&key) {
            return entry.states.clone();
        }
        let Some(count) = super::axis_combinations(&block.visual.state_axes) else {
            return SharedStates::empty();
        };
        let Some(bytes) = state_bytes(&block.name, &block.visual, count as usize)
            .filter(|bytes| *bytes <= MAX_AGGREGATE_STATE_BYTES)
        else {
            return SharedStates::empty();
        };
        let states = SharedStates(block.compile_states());
        while self.entries.len() >= MAX_CACHED_DEFINITIONS
            || self.states + states.len() > MAX_AGGREGATE_STATES
            || self.bytes + bytes > MAX_AGGREGATE_STATE_BYTES
        {
            let Some(oldest) = self.eviction_order.pop_front() else {
                break;
            };
            #[cfg(test)]
            {
                self.eviction_visits += 1;
            }
            if let Some(entry) = self.entries.remove(&oldest) {
                self.states -= entry.states.len();
                self.bytes -= entry.bytes;
            }
        }
        self.states += states.len();
        self.bytes += bytes;
        self.entries.insert(
            key,
            Entry {
                _visual: Arc::downgrade(&block.visual),
                _name: Arc::downgrade(&block.name),
                states: states.clone(),
                bytes,
            },
        );
        self.eviction_order.push_back(key);
        states
    }
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
