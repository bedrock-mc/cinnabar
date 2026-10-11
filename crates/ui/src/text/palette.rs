//! Formatting colors loaded from the active JSON-UI global variables.
//!
//! Vanilla reads RGB triples from the global colour definitions into its shared
//! format-code colour table.

use super::{BedrockColor, FORMATTING_COLORS};

/// The active pack's formatting colors, separate from cached text geometry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FormattingPalette([Option<[u8; 3]>; BedrockColor::PartyBlue as usize + 1]);

impl FormattingPalette {
    /// Reads vanilla's global names; absent or malformed triples become white.
    pub fn from_globals(mut lookup: impl FnMut(&str) -> Option<[f32; 3]>) -> Self {
        let mut palette = Self::default();
        for entry in FORMATTING_COLORS {
            let rgb = lookup(entry.global).unwrap_or([1.0; 3]);
            palette.0[entry.color as usize] =
                Some(rgb.map(|component| (component.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
        palette
    }

    /// Resolves a formatting color while preserving the label's RGB for unformatted text.
    pub fn rgb(&self, color: BedrockColor) -> Option<[u8; 3]> {
        self.0[color as usize].or_else(|| color.rgb())
    }
}
