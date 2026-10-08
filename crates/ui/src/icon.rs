/// One packed item icon: texture-array page plus exact pixel UVs, both
/// produced by the deterministic atlas placement — never guessed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IconRef {
    pub page: u16,
    pub uv: [u16; 4],
    /// Draws the enchantment glint over the icon.
    pub glint: bool,
}

impl IconRef {
    /// Returns this icon with the requested enchantment-glint flag.
    #[must_use]
    pub const fn with_glint(self, glint: bool) -> Self {
        Self { glint, ..self }
    }

    /// The icon's sprite, glinting when marked.
    pub const fn visual(self, color: [u8; 4]) -> crate::UiVisual {
        if self.glint {
            crate::UiVisual::GlintSprite {
                texture_page: self.page,
                uv: self.uv,
                color,
            }
        } else {
            crate::UiVisual::Sprite {
                texture_page: self.page,
                uv: self.uv,
                color,
            }
        }
    }
}
