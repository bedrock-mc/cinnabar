//! Named wire IDs shared by packet, asset and WebAssembly consumers.

#![no_std]

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

            /// Resolves a known ID; aliases resolve to the first declared name.
            pub const fn from_wire(raw: $raw) -> Option<Self> {
                $(if raw == $value { return Some(Self::$variant); })*
                None
            }
        }
    };
}

include!("generated.rs");
