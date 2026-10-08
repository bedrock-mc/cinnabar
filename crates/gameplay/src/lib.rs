//! Engine-independent local gameplay, prediction and ordered interaction admission.

pub mod block_use;
pub mod interaction_authority;
pub mod item_use;
pub mod melee;
pub mod mining;
pub mod movement;
mod placement_connections;
mod placement_doors;
mod placement_facing;
mod placement_multiface;
mod placement_prediction;
mod placement_signs;
mod placement_stacking;
mod placement_stairs;
pub mod placement_state;
mod placement_support;
mod placement_vines;
pub mod survival_mining;

pub use protocol::BatchSendError;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

mod world_view;
pub use world_view::GameplayWorld;

pub mod committed_control;
