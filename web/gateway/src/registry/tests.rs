//! Read-only observations shared by the original socket fixtures.

use super::Registry;

/// Reports retained connection permits and sessions without admitting or retiring any owner.
pub(crate) fn retained_counts(registry: &Registry) -> (usize, usize) {
    let state = registry
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    (state.active, state.sessions.len())
}
