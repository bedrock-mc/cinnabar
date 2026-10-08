//! Portable camera, equipment and authored UI presentation without session or platform adapters.

pub mod armor_pose;
pub mod camera;
pub mod cape;
pub mod equipment_display;
pub mod equipment_sprite_atlas;
pub mod nametag_atlas;
pub mod nametags;
pub mod progress;
pub mod text;
pub mod ui_adapter;
pub mod ui_atlas;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAtlasError {
    InvalidFontTexture,
}

impl std::fmt::Display for UiAtlasError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "UI atlas rejected input: {self:?}")
    }
}
impl std::error::Error for UiAtlasError {}

#[cfg(test)]
mod test_support;
