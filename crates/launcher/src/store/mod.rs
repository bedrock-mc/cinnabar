//! Store requests, purchase state and immutable screen snapshots.

pub mod action;
pub mod flow;
pub mod settings;
pub mod snapshot;
pub mod state;
pub mod worker;

pub use action::StoreAction;
pub use snapshot::{DisplayRow, StoreArt, StoreSnapshot, StoreView};
pub use state::StoreState;
pub use worker::{StoreError, StoreEvent, StoreRequest};

/// The vanilla base screen for Marketplace navigation.
pub const SDL_SCREEN: &str = "store_layout.store_data_driven_screen";
