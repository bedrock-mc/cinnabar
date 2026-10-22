//! Recoverable render allocations: out-of-memory skips the owner's work instead of
//! quitting. Other GPU errors remain fatal.

use crate::queue_writes::ProfiledQueue as _;
use bevy::render::{
    error_handler::{ErrorType, RenderError, RenderErrorHandler, RenderErrorPolicy},
    render_resource::{
        Buffer, BufferDescriptor, BufferUsages, Texture, TextureDataOrder, TextureDescriptor,
    },
    renderer::{RenderDevice, RenderQueue},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Keeps rendering after an uncaptured out-of-memory error; other errors keep Bevy's policy.
/// Install after the render plugin so this handler replaces the default.
pub fn install_error_policy(app: &mut bevy::app::App) {
    app.insert_resource(RenderErrorHandler(error_policy));
}

/// wgpu reports out-of-memory per failed operation and leaves the device usable; a real device
/// loss arrives separately as `DeviceLost`, and later use of the failed resource stays fatal.
fn error_policy(
    error: &RenderError,
    main_world: &mut bevy::ecs::world::World,
    render_world: &mut bevy::ecs::world::World,
) -> RenderErrorPolicy {
    if error.ty == ErrorType::OutOfMemory {
        bevy::log::warn_once!("GPU memory exhausted; continuing without the failed allocation");
        return RenderErrorPolicy::Ignore;
    }
    (RenderErrorHandler::default().0)(error, main_world, render_world)
}

/// Returns no resource when allocation or staging ran out of memory on this thread.
/// WebGPU reports errors asynchronously through Bevy's device error handler instead.
pub(crate) fn checked<T>(
    device: &RenderDevice,
    label: &str,
    bytes: u64,
    create: impl FnOnce() -> T,
) -> Option<T> {
    #[cfg(not(target_arch = "wasm32"))]
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let resource = create();
    #[cfg(not(target_arch = "wasm32"))]
    let failed = bevy::tasks::block_on(scope.pop()).is_some();
    #[cfg(target_arch = "wasm32")]
    let failed = {
        // A WebGPU error-scope promise cannot settle while this browser turn is blocked.
        let _ = device;
        false
    };
    #[cfg(test)]
    let failed = failed
        || FAILURE.with_borrow_mut(|hook| hook.as_mut().is_some_and(|hook| hook(label, bytes)));
    if failed {
        warn_once(label, bytes);
        None
    } else {
        Some(resource)
    }
}

/// Logs the first failure of each allocation owner without repeating every frame.
fn warn_once(label: &str, bytes: u64) {
    static WARNED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    if WARNED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(label.to_owned())
    {
        bevy::log::warn!(
            label,
            bytes,
            "GPU allocation failed; retaining resources or skipping this publication"
        );
    }
}

/// A failed replacement upload; it retries once after the GPU retires what its owner released.
#[derive(Default)]
pub(crate) struct UploadRetry {
    failed: bool,
    retired: Option<Arc<AtomicBool>>,
}

impl UploadRetry {
    /// Records an attempt; a failure after releasing resources waits for every submission so far,
    /// which holds their last use.
    pub(crate) fn record(&mut self, uploaded: bool, released: bool, queue: &RenderQueue) {
        self.failed = !uploaded;
        self.retired = None;
        if self.failed && released {
            let retired = Arc::new(AtomicBool::new(false));
            let signal = Arc::clone(&retired);
            // Completed submissions release their resources before the frame poll runs callbacks.
            queue.on_submitted_work_done(move || signal.store(true, Ordering::Release));
            self.retired = Some(retired);
        }
    }

    /// Reports whether the last attempt failed, so its owner does not retry every frame.
    pub(crate) fn failed(&self) -> bool {
        self.failed
    }

    /// Returns `true` once, when a failed attempt may retry because released resources retired.
    pub(crate) fn take_ready(&mut self) -> bool {
        let ready = self
            .retired
            .as_ref()
            .is_some_and(|retired| retired.load(Ordering::Acquire));
        if ready {
            self.retired = None;
        }
        ready
    }
}

/// Creates a texture and uploads `data` under one scope, before any view or binding uses it.
pub(crate) fn texture_with_data(
    device: &RenderDevice,
    queue: &RenderQueue,
    descriptor: &TextureDescriptor<'_>,
    data: &[u8],
) -> Option<Texture> {
    checked(
        device,
        descriptor.label.unwrap_or("unlabeled texture"),
        data.len() as u64,
        || device.create_texture_with_data(queue, descriptor, TextureDataOrder::LayerMajor, data),
    )
}

/// Checks creation before any mapping, copying, binding, or upload can use the buffer.
pub(crate) fn buffer(device: &RenderDevice, descriptor: &BufferDescriptor<'_>) -> Option<Buffer> {
    checked(
        device,
        descriptor.label.unwrap_or("unlabeled buffer"),
        descriptor.size,
        || device.create_buffer(descriptor),
    )
}

/// Creates and initializes an unmapped buffer, checking both allocation and staging.
pub(crate) fn buffer_with_data(
    device: &RenderDevice,
    queue: &RenderQueue,
    label: &str,
    bytes: &[u8],
    usage: BufferUsages,
) -> Option<Buffer> {
    let buffer = buffer(
        device,
        &BufferDescriptor {
            label: Some(label),
            size: bytes.len() as u64,
            usage: usage | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        },
    )?;
    checked(device, label, bytes.len() as u64, || {
        queue.profiled_write_buffer(&buffer, 0, bytes)
    })?;
    Some(buffer)
}

#[cfg(test)]
type FailureHook = Box<dyn FnMut(&str, u64) -> bool>;
#[cfg(test)]
thread_local! { static FAILURE: std::cell::RefCell<Option<FailureHook>> = const { std::cell::RefCell::new(None) }; }

/// Installs a scoped, thread-local failure injector at the real error-scope boundary.
#[cfg(test)]
pub(crate) fn inject(hook: impl FnMut(&str, u64) -> bool + 'static) -> impl Drop {
    struct Reset;
    impl Drop for Reset {
        /// Removes the injector even when a regression assertion panics.
        fn drop(&mut self) {
            FAILURE.with_borrow_mut(|hook| *hook = None);
        }
    }
    FAILURE.with_borrow_mut(|slot| {
        assert!(slot.is_none());
        *slot = Some(Box::new(hook));
    });
    Reset
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        app::AppExit,
        ecs::{message::Messages, world::World},
    };

    // An uncaptured out-of-memory error keeps the client running; other errors still quit.
    #[test]
    fn out_of_memory_keeps_rendering_while_validation_errors_still_quit() {
        let error = |ty| RenderError {
            ty,
            description: String::new(),
            source: None,
        };
        let (mut main_world, mut render_world) = (World::new(), World::new());
        main_world.init_resource::<Messages<AppExit>>();
        let policy = error_policy(
            &error(ErrorType::OutOfMemory),
            &mut main_world,
            &mut render_world,
        );
        assert!(matches!(policy, RenderErrorPolicy::Ignore));
        assert!(main_world.resource::<Messages<AppExit>>().is_empty());
        let policy = error_policy(
            &error(ErrorType::Validation),
            &mut main_world,
            &mut render_world,
        );
        assert!(matches!(policy, RenderErrorPolicy::StopRendering));
        assert!(!main_world.resource::<Messages<AppExit>>().is_empty());
    }
}
