//! Runtime pack identities for the three chalkboard sizes.

/// The model size carried by a chalkboard block entity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChalkboardSize {
    Small,
    Medium,
    Large,
}

impl ChalkboardSize {
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    /// Resolves the encoded size without assigning an unknown value a different model.
    pub const fn from_nbt(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Small),
            1 => Some(Self::Medium),
            2 => Some(Self::Large),
            _ => None,
        }
    }

    /// Index in the size-ordered runtime model cache.
    pub const fn index(self) -> usize {
        match self {
            Self::Small => 0,
            Self::Medium => 1,
            Self::Large => 2,
        }
    }

    /// The geometry identifier read from the active pack stack.
    pub const fn geometry(self) -> &'static str {
        match self {
            Self::Small => "geometry.chalkboard_small",
            Self::Medium => "geometry.chalkboard_medium",
            Self::Large => "geometry.chalkboard_large",
        }
    }

    /// The world model's texture, distinct from its inventory icon.
    pub const fn texture(self) -> &'static str {
        match self {
            Self::Small => "textures/item/chalkboard/chalkboard_small",
            Self::Medium => "textures/item/chalkboard/chalkboard_medium",
            Self::Large => "textures/item/chalkboard/chalkboard_large",
        }
    }
}
