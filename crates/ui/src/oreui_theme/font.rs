//! Named font roles and their carrier definitions.
use assets::carriers::{self, Carrier};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OreUiFont {
    Seven,
    Ten,
    Five,
    FiveBold,
}

impl OreUiFont {
    pub const ALL: [Self; 4] = [Self::Seven, Self::Ten, Self::Five, Self::FiveBold];

    /// Selects the shared carrier definition for this theme role.
    pub const fn carrier(self) -> &'static Carrier {
        match self {
            Self::Seven => &carriers::FONT_SEVEN,
            Self::Ten => &carriers::FONT_TEN,
            Self::Five => &carriers::FONT_FIVE,
            Self::FiveBold => &carriers::FONT_FIVE_BOLD,
        }
    }

    /// Returns the shipped family name used by the theme and font catalog.
    pub const fn name(self) -> &'static str {
        self.carrier().font_face.unwrap().name
    }
}
