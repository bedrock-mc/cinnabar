//! Worn armor and held items as extra rig layers that ride the owning actor's pose.

mod attachable;
pub mod blocks;
mod first_person;
#[cfg(test)]
mod frames;
mod input;
mod runtime;
#[cfg(test)]
mod tests;

pub use input::{local_input, remote_input};
pub use runtime::{
    ActorEquipmentInput, EquipmentAnimation, EquipmentPresentation, HeldKind, JavaGrip, WornItem,
    java_draws_attachable,
};
pub use runtime::{EquipmentRuntime, FirstPersonItem, StagedSessionIcons};

/// Chest equipment occupies the same layer for its pack image and cape replacement.
pub(crate) const ELYTRA_LAYER: u8 = view_presentation::equipment_display::LAYER_CHESTPLATE;
/// Worn-wing material groups after the first, in the ids free below the cape.
const ELYTRA_GROUP_LAYERS: std::ops::Range<u8> =
    view_presentation::equipment_display::LAYER_BOOTS + 1..view_presentation::cape::ACTOR_LAYER_CAPE;

/// Whether an instance draws one of the worn wings' material groups.
pub(crate) fn is_elytra_layer(layer: u8) -> bool {
    layer == ELYTRA_LAYER || ELYTRA_GROUP_LAYERS.contains(&layer)
}

/// Converts native item animation observations into portable presentation facts.
pub trait IntoFirstPersonHand {
    fn into_first_person_hand(self) -> view_presentation::equipment_display::FirstPersonHand;
}
impl IntoFirstPersonHand for client_world::ItemAnimationState {
    fn into_first_person_hand(self) -> view_presentation::equipment_display::FirstPersonHand {
        view_presentation::equipment_display::FirstPersonHand { swing: self.attack_time, equip: self.arm_height, consume: None }
    }
}
impl IntoFirstPersonHand for view_presentation::equipment_display::FirstPersonHand {
    fn into_first_person_hand(self) -> view_presentation::equipment_display::FirstPersonHand { self }
}
