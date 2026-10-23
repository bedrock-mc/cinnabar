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
        static WARNING: std::sync::Once = std::sync::Once::new();
        obstruction_or_warn_once(android::bottom_obstruction_pixels(), &WARNING, |error| {
            warn!(
                "CinnabarActivity.bottomObstructionPixels() failed; keyboard avoidance is unavailable: {error}"
            );
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        0.0
    }
}

/// Preserves valid geometry and reports a failed read only once before falling back to zero.
#[cfg(any(target_os = "android", test))]
fn obstruction_or_warn_once<E>(
    result: Result<i32, E>,
    warning: &std::sync::Once,
    report: impl FnOnce(E),
) -> f32 {
    match result {
        Ok(pixels) => pixels as f32,
        Err(error) => {
            warning.call_once(|| report(error));
            0.0
        }
    }
}

#[cfg(target_os = "android")]
mod android {
    /// Reads geometry published by the activity's UI thread without resizing the render surface.
    pub(super) fn bottom_obstruction_pixels() -> jni::errors::Result<i32> {
        let app = bevy::android::ANDROID_APP
            .get()
            .ok_or(jni::errors::Error::NullPtr(
                "Android activity is unavailable",
            ))?;
        // Bevy keeps the VM and this activity's global reference alive for the native loop.
        let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast())? };
        let mut env = vm.attach_current_thread()?;
        let activity = unsafe { jni::objects::JObject::from_raw(app.activity_as_ptr().cast()) };
        let result = env.call_method(&activity, "bottomObstructionPixels", "()I", &[]);
        if result.is_err() && env.exception_check()? {
            env.exception_clear()?;
        }
        result?.i()
    }
}

#[cfg(test)]
mod tests {
    use super::obstruction_or_warn_once;

    #[test]
    fn failed_obstruction_reads_warn_once_and_allow_later_geometry() {
        let warning = std::sync::Once::new();
        let mut errors = Vec::new();
        for (result, expected) in [
            (Ok(280), 280.0),
            (Err("missing activity method"), 0.0),
            (Err("missing activity method"), 0.0),
            (Ok(360), 360.0),
            (Ok(0), 0.0),
        ] {
            assert_eq!(
                obstruction_or_warn_once(result, &warning, |error| errors.push(error)),
                expected,
            );
        }
        assert_eq!(errors, ["missing activity method"]);
    }
}
