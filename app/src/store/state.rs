//! Bevy resource adapter for the engine-independent store state.

#[derive(bevy::prelude::Resource, bevy::prelude::Deref, bevy::prelude::DerefMut, Default)]
pub(crate) struct StoreState(launcher::store::StoreState);

impl StoreState {
    /// Creates an empty store state for the host's worker session.
    pub(crate) fn new() -> Self {
        Self::default()
    }
}
