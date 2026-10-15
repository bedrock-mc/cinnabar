//! Native window attributes shared by the bootstrap windows and the game window: the title, the
//! icon and the smallest bootstrap window size.

use winit::{
    dpi::LogicalSize,
    window::{Icon, Window, WindowAttributes},
};

// Rasterized from packaging/icons/cinnabar.svg; shared with the Windows installer artwork.
use launcher::branding::ICON as ICON_BYTES;

/// The environment variable an embedding sets to rename the game and bootstrap windows.
pub const TITLE_ENV: &str = "CINNABAR_WINDOW_TITLE";
/// The smallest window any bootstrap window allows.
pub const MIN_SIZE: LogicalSize<f64> = LogicalSize::new(560.0, 420.0);

/// A bootstrap window of logical `size`, titled like the game window so an embedding's
/// [`TITLE_ENV`] renames both, with the product icon and [`MIN_SIZE`].
pub fn attributes(size: LogicalSize<f64>) -> WindowAttributes {
    titled_attributes(std::env::var(TITLE_ENV).ok().as_deref(), size)
}

fn titled_attributes(title: Option<&str>, size: LogicalSize<f64>) -> WindowAttributes {
    Window::default_attributes()
        .with_title(launcher::window_title(title))
        .with_window_icon(Some(icon()))
        .with_inner_size(size)
        .with_min_inner_size(MIN_SIZE)
}

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

    #[test]
    fn windows_use_embedding_title_and_keep_default_for_blank_values() {
        assert_eq!(
            titled_attributes(Some("Zeno Client"), MIN_SIZE).title,
            "Zeno Client"
        );
        for title in [None, Some(""), Some("  ")] {
            assert_eq!(
                titled_attributes(title, MIN_SIZE).title,
                launcher::PRODUCT_NAME
            );
        }
    }
}
