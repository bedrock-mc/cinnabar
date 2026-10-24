//! Named wire IDs shared by packet, asset and WebAssembly consumers.

#![no_std]

/// Item descriptor auxiliary value that accepts any auxiliary value on the other item.
pub const ITEM_AUXILIARY_WILDCARD: u16 = i16::MAX as u16;

macro_rules! wire_ids {
    ($name:ident, $raw:ty, {$($variant:ident => $value:expr),* $(,)?}) => {
        /// Named values admitted by the pinned protocol contract.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
        pub enum $name { $($variant),* }

        impl $name {
            /// Every named value, including aliases sharing a wire ID.
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            /// Returns the value transmitted on the wire.
            pub const fn wire_value(self) -> $raw {
                match self { $(Self::$variant => $value),* }
            }

            /// Resolves a named contract variant without repeating its numeric value.
            pub fn from_name(name: &str) -> Option<Self> {
                match name { $(stringify!($variant) => Some(Self::$variant),)* _ => None }
            }

            /// Resolves a known ID; aliases resolve to the first declared name.
            pub const fn from_wire(raw: $raw) -> Option<Self> {
                $(if raw == $value { return Some(Self::$variant); })*
                None
            }
        }
    };
}

mod generated;
pub use generated::{
    ActorDataKey, ActorFlag, Attribute, CommandArgFlag, CommandArgType, ContainerName, Effect,
    Enchantment, HASHED_AIR_NETWORK_ID, HorseFlag, LevelEvent, PlayerFlag,
    SEQUENTIAL_AIR_NETWORK_ID, SPEED_EFFECT_MODIFIER_ID, SPRINT_SPEED_MODIFIER_ID, WindowID,
    WindowType,
};
