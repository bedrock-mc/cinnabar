//! Embedded app identity for native window chrome, including unpackaged builds.

use bevy::{ecs::system::NonSendMarker, prelude::*, window::WindowCreated, winit::WINIT_WINDOWS};
use winit::window::Icon;

// Rasterized from packaging/icons/cinnabar.svg; shared with the Windows installer artwork.
const ICON_BYTES: &[u8] = include_bytes!("../../assets/branding/icon.png");

pub(crate) fn icon() -> Icon {
    let image = image::load_from_memory(ICON_BYTES)
        .expect("bundled Cinnabar window icon must decode")
        .into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height)
        .expect("bundled Cinnabar window icon must have valid RGBA dimensions")
}

pub(crate) fn apply(
    mut created: MessageReader<WindowCreated>,
    mut cached: Local<Option<Icon>>,
    _main_thread: NonSendMarker,
) {
    for event in created.read() {
        let icon = cached.get_or_insert_with(icon);
        WINIT_WINDOWS.with_borrow(|windows| {
            if let Some(window) = windows.get_window(event.window) {
                window.set_window_icon(Some(icon.clone()));
            }
        });
    }
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
