//! Worn armor and held items as extra rig layers that ride the owning actor's pose.

mod armor;
mod atlas;
mod display;
mod first_person;
#[cfg(test)]
mod frames;
mod input;
mod runtime;
#[cfg(test)]
mod tests;

pub use display::FirstPersonHand;
pub use input::{local_input, remote_input};
pub use runtime::{
    ActorEquipmentInput, EquipmentAnimation, EquipmentPresentation, HeldKind, JavaGrip, WornItem,
    java_draws_attachable,
};
pub use runtime::{EquipmentRuntime, FirstPersonArms, FirstPersonItem, StagedSessionIcons};

/// Chest equipment occupies the same layer for its pack image and cape replacement.
pub(crate) const ELYTRA_LAYER: u8 = display::LAYER_CHESTPLATE;
/// Worn-wing material groups after the first, in the ids free below the cape.
const ELYTRA_GROUP_LAYERS: std::ops::Range<u8> =
    display::LAYER_BOOTS + 1..super::cape::ACTOR_LAYER_CAPE;

/// Whether an instance draws one of the worn wings' material groups.
pub(crate) fn is_elytra_layer(layer: u8) -> bool {
    layer == ELYTRA_LAYER || ELYTRA_GROUP_LAYERS.contains(&layer)
}
