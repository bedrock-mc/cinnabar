//! Recoverable render allocations: out-of-memory skips the owner's work instead of
//! quitting. A lost device ends the app with its cause; other GPU errors remain fatal.

use crate::queue_writes::ProfiledQueue as _;
use bevy::render::{
    error_handler::{ErrorType, RenderError, RenderErrorHandler, RenderErrorPolicy},
    render_resource::{
        Buffer, BufferDescriptor, BufferUsages, Origin3d, TexelCopyBufferLayout,
        TexelCopyTextureInfo, Texture, TextureAspect, TextureDescriptor,
    },
    renderer::{RenderDevice, RenderQueue},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Keeps rendering after an uncaptured out-of-memory error and hands a lost device to the
/// main world; other errors keep Bevy's policy. Install after the render plugin so this
/// handler replaces the default.
pub fn install_error_policy(app: &mut bevy::app::App) {
    app.init_resource::<DeviceLoss>()
        .insert_resource(RenderErrorHandler(error_policy));
}

/// The driver's description of a lost render device; nothing renders once it is set, and the
/// app ends with it as the cause.
#[derive(bevy::prelude::Resource, Default)]
pub struct DeviceLoss(Option<String>);

impl DeviceLoss {
    /// The driver's description once the device is lost.
    pub fn message(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// Keeps the first loss; Bevy repeats a stopped renderer's error every frame.
    pub fn record(&mut self, description: &str) {
        self.0.get_or_insert_with(|| description.to_owned());
    }
}

/// wgpu reports out-of-memory per failed operation and leaves the device usable; a real device
/// loss arrives separately as `DeviceLost`, and later use of the failed resource stays fatal.
fn error_policy(
    error: &RenderError,
    main_world: &mut bevy::ecs::world::World,
    render_world: &mut bevy::ecs::world::World,
) -> RenderErrorPolicy {
    match error.ty {
        ErrorType::OutOfMemory => {
            bevy::log::warn_once!("GPU memory exhausted; continuing without the failed allocation");
            RenderErrorPolicy::Ignore
        }
        ErrorType::DeviceLost => {
            main_world
                .get_resource_or_init::<DeviceLoss>()
                .record(&error.description);
            RenderErrorPolicy::StopRendering
        }
        _ => (RenderErrorHandler::default().0)(error, main_world, render_world),
    }
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

/// Frames before the first retry of a failure; each failed retry doubles the wait.
pub(crate) const MIN_RETRY_FRAMES: u32 = 30;
/// Longest wait, so content returns after memory frees without retrying every frame.
const MAX_RETRY_FRAMES: u32 = 1920;

/// A failed upload that retries after released resources retire, or after a bounded backoff.
#[derive(Default)]
pub(crate) struct UploadRetry {
    failed: bool,
    retrying: bool,
    interval: u32,
    wait: u32,
    retired: Option<Arc<AtomicBool>>,
}

impl UploadRetry {
    /// Records an attempt; with `released`, a failure also retries once every submission so far,
    /// which holds the released resources' last use, completes.
    pub(crate) fn record(&mut self, uploaded: bool, released: Option<&RenderQueue>) {
        if uploaded {
            *self = Self::default();
            return;
        }
        self.interval = if self.retrying {
            (self.interval * 2).min(MAX_RETRY_FRAMES)
        } else {
            MIN_RETRY_FRAMES
        };
        self.retrying = false;
        self.failed = true;
        self.wait = self.interval;
        self.retired = released.map(|queue| {
            let retired = Arc::new(AtomicBool::new(false));
            let signal = Arc::clone(&retired);
            // Completed submissions release their resources before the frame poll runs callbacks.
            queue.on_submitted_work_done(move || signal.store(true, Ordering::Release));
            retired
        });
    }

    /// Reports whether the last attempt failed, so its owner does not retry every frame.
    pub(crate) fn failed(&self) -> bool {
        self.failed
    }

    /// Advances one frame of a failed upload; `true` when its owner should retry and `record` it.
    pub(crate) fn take_ready(&mut self) -> bool {
        if !self.failed {
            return false;
        }
        let retired = self
            .retired
            .as_ref()
            .is_some_and(|retired| retired.load(Ordering::Acquire));
        self.wait = self.wait.saturating_sub(1);
        if !retired && self.wait != 0 {
            return false;
        }
        self.retired = None;
        self.retrying = true;
        self.wait = self.interval;
        true
    }
}

/// Creates a texture in its own scope; an out-of-memory texture is invalid, so nothing may
/// write, view or bind it.
pub(crate) fn texture(
    device: &RenderDevice,
    descriptor: &TextureDescriptor<'_>,
) -> Option<Texture> {
    let size = descriptor.size;
    let bytes = u64::from(size.width)
        * u64::from(size.height)
        * u64::from(size.depth_or_array_layers)
        * u64::from(descriptor.format.block_copy_size(None).unwrap_or(4));
    checked(
        device,
        descriptor.label.unwrap_or("unlabeled texture"),
        bytes,
        || device.create_texture(descriptor),
    )
}

/// Creates a single-mip texture, then uploads layer-major `data` only once creation succeeded.
pub(crate) fn texture_with_data(
    device: &RenderDevice,
    queue: &RenderQueue,
    descriptor: &TextureDescriptor<'_>,
    data: &[u8],
) -> Option<Texture> {
    debug_assert_eq!(descriptor.mip_level_count, 1);
    let texture = texture(device, descriptor)?;
    let size = descriptor.size;
    let block = descriptor.format.block_copy_size(None)?;
    let label = format!("{} upload", descriptor.label.unwrap_or("unlabeled texture"));
    checked(device, &label, data.len() as u64, || {
        queue.profiled_write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            data,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * block),
                rows_per_image: Some(size.height),
            },
            size,
        );
    })?;
    Some(texture)
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

    // A failure with nothing released still retries, backing off to a bounded interval.
    #[test]
    fn failed_uploads_retry_with_bounded_backoff_until_one_succeeds() {
        let frames_until_ready = |retry: &mut UploadRetry| {
            (1..=MAX_RETRY_FRAMES)
                .find(|_| retry.take_ready())
                .expect("a failed upload always retries")
        };
        let mut retry = UploadRetry::default();
        assert!(!retry.take_ready());
        retry.record(false, None);
        assert!(retry.failed());
        assert_eq!(frames_until_ready(&mut retry), MIN_RETRY_FRAMES);
        retry.record(false, None);
        assert_eq!(frames_until_ready(&mut retry), MIN_RETRY_FRAMES * 2);
        for _ in 0..16 {
            retry.record(false, None);
            frames_until_ready(&mut retry);
        }
        retry.record(false, None);
        assert_eq!(frames_until_ready(&mut retry), MAX_RETRY_FRAMES);
        retry.record(true, None);
        assert!(!retry.failed() && !retry.take_ready());
        retry.record(false, None);
        assert_eq!(frames_until_ready(&mut retry), MIN_RETRY_FRAMES);
    }

    /// A lost device stops rendering and leaves the exit, with its cause, to the main world.
    #[test]
    fn device_loss_stops_rendering_and_records_the_driver_reason() {
        let lost = |description: &str| RenderError {
            ty: ErrorType::DeviceLost,
            description: description.into(),
            source: None,
        };
        let (mut main_world, mut render_world) = (World::new(), World::new());
        main_world.init_resource::<Messages<AppExit>>();
        for description in ["DXGI_ERROR_DEVICE_HUNG", "repeated"] {
            let policy = error_policy(&lost(description), &mut main_world, &mut render_world);
            assert!(matches!(policy, RenderErrorPolicy::StopRendering));
        }
        assert!(main_world.resource::<Messages<AppExit>>().is_empty());
        assert_eq!(
            main_world.resource::<DeviceLoss>().message(),
            Some("DXGI_ERROR_DEVICE_HUNG")
        );
    }
}
