//! Worn armor and held items as extra rig layers that ride the owning actor's pose.

mod armor;
mod atlas;
mod attachable;
pub mod blocks;
mod display;
mod elytra;
mod first_person;
#[cfg(test)]
mod frames;
mod input;
mod runtime;
#[cfg(test)]
mod tests;

pub use display::FirstPersonHand;
pub use input::{local_input, remote_input};
pub use runtime::{ActorEquipmentInput, HeldKind, WornItem};
pub use runtime::{EquipmentRuntime, FirstPersonArms, FirstPersonItem, StagedSessionIcons};
