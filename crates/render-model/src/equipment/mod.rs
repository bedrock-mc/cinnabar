//! Shared equipment geometry for world rendering and inventory previews.

mod attachable;
pub mod blocks;
mod display;
#[cfg(test)]
mod tests;

pub use attachable::{BoneChannels, attach, authored_rotation};
pub use display::{
    ItemDisplay, attach_to_bone, held_block_display, held_block_display_for_hand,
    held_sprite_display, held_sprite_display_for_hand, is_hand_equipped, is_rod,
    sprite_item_transform,
};
