//! Platform keyboard requests and keyboard obstruction geometry.

use std::collections::HashSet;

use bevy::{ecs::system::NonSendMarker, prelude::*, winit::WINIT_WINDOWS};

/// Explicit editor activations, including retaps after the OS dismissed its keyboard.
#[derive(Resource, Default)]
pub struct KeyboardRequests(HashSet<Entity>);

impl KeyboardRequests {
    /// Requests a platform show even when text input was already enabled.
    pub fn show(&mut self, window: Entity) {
        self.0.insert(window);
    }

    /// Whether this frame requested a keyboard for the given window.
    #[cfg(test)]
    pub(crate) fn requested(&self, window: Entity) -> bool {
        self.0.contains(&window)
    }
}

/// Delivers activation requests independently of Bevy's cached enabled flag.
pub(crate) fn show_keyboard_requests(
    mut requests: ResMut<KeyboardRequests>,
    windows: Query<&Window>,
    _main_thread: NonSendMarker,
) {
    for entity in requests.0.drain() {
        if windows
            .get(entity)
            .is_ok_and(|window| window.focused && window.ime_enabled)
        {
            WINIT_WINDOWS.with_borrow(|native| {
                if let Some(window) = native.get_window(entity) {
                    window.set_ime_allowed(true);
                }
            });
        }
    }
}

/// Bottom system-window obstruction in physical surface pixels; desktop windows resize themselves.
pub fn bottom_obstruction_pixels() -> f32 {
    #[cfg(target_os = "android")]
    {
        android::bottom_obstruction_pixels().unwrap_or(0) as f32
    }
    #[cfg(not(target_os = "android"))]
    {
        0.0
    }
}

#[cfg(target_os = "android")]
mod android {
    /// Reads geometry published by the activity's UI thread without resizing the render surface.
    pub(super) fn bottom_obstruction_pixels() -> jni::errors::Result<i32> {
        let context = ndk_context::android_context();
        // The Android runtime owns the VM and the activity reference for this native loop.
        let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast())? };
        let mut env = vm.attach_current_thread()?;
        let activity = unsafe { jni::objects::JObject::from_raw(context.context().cast()) };
        let result = env.call_method(&activity, "bottomObstructionPixels", "()I", &[]);
        if result.is_err() && env.exception_check()? {
            env.exception_clear()?;
        }
        result?.i()
    }
}
