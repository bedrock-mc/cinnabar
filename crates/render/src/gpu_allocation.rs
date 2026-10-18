//! Recoverable render allocations: out-of-memory skips the owner's work instead of
//! quitting. Other GPU errors remain fatal.

use crate::queue_writes::ProfiledQueue as _;
use bevy::render::{
    render_resource::{Buffer, BufferDescriptor, BufferUsages},
    renderer::{RenderDevice, RenderQueue},
};

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
