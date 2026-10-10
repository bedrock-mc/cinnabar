//! Native window icon shared by setup and the game window.
use winit::window::Icon;

// Rasterized from packaging/icons/cinnabar.svg; shared with the Windows installer artwork.
use launcher::branding::ICON as ICON_BYTES;

/// Decodes the shared original icon for a native window.
pub fn icon() -> Icon {
    let image = image::load_from_memory(ICON_BYTES)
        .expect("bundled Cinnabar window icon must decode")
        .into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height)
        .expect("bundled Cinnabar window icon must have valid RGBA dimensions")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_icon_is_visible_with_transparent_corners() {
        let image = image::load_from_memory(ICON_BYTES).unwrap().into_rgba8();
        assert_eq!(image.width(), image.height());
        assert_eq!(image.get_pixel(0, 0).0[3], 0);
        assert_eq!(
            image.get_pixel(image.width() / 2, image.height() / 2).0[3],
            255
        );
        let _ = icon();
    }
}
