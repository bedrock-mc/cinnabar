//! Embedded app identity for native window chrome, including unpackaged builds.

use bevy::{ecs::system::NonSendMarker, prelude::*, window::WindowCreated, winit::WINIT_WINDOWS};
use winit::window::Icon;

pub(crate) fn apply(
    mut created: MessageReader<WindowCreated>,
    mut cached: Local<Option<Icon>>,
    _main_thread: NonSendMarker,
) {
    for event in created.read() {
        let icon = cached.get_or_insert_with(first_run::window_icon::icon);
        WINIT_WINDOWS.with_borrow(|windows| {
            if let Some(window) = windows.get_window(event.window) {
                window.set_window_icon(Some(icon.clone()));
            }
        });
    }
}
