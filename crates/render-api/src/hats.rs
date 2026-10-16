//! Client-owned wardrobe choices, independent of server skin payloads.

/// Declares the catalog and its identifiers together so variants cannot escape the gallery.
macro_rules! hats {
    ($($variant:ident => ($id:literal, $name:literal)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Hat { $($variant),+ }

        impl Hat {
            pub const ALL: &[Self] = &[$(Self::$variant),+];

            /// Stable identifier used by wardrobe preferences and artwork keys.
            pub const fn id(self) -> &'static str {
                match self { $(Self::$variant => $id),+ }
            }

            /// Display name shared by the gallery and equipped-look details.
            pub const fn name(self) -> &'static str {
                match self { $(Self::$variant => $name),+ }
            }
        }
    };
}

hats! {
    BunnyEars => ("bunny_ears", "Bunny ears"),
    Crown => ("crown", "Crown"),
}

impl Hat {
    /// Stable artwork key of the original attachment texture.
    pub fn texture_key(self) -> String {
        format!("hat-texture:{}", self.id())
    }

    /// Stable artwork key of the gallery's model thumbnail.
    pub fn thumbnail_key(self) -> String {
        format!("hat-thumbnail:{}", self.id())
    }

    /// Resolves a saved choice; unknown cosmetics leave the head uncovered.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|hat| hat.id() == id)
    }
}
