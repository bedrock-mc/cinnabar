//! Scoped render allocations: out-of-memory skips or retries the owner's work instead of
//! reaching Bevy's render error handler.

use crate::queue_writes::ProfiledQueue as _;
use bevy::render::{
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
const MAX_UPLOAD_RETRY_FRAMES: u32 = 1920;

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
            (self.interval * 2).min(MAX_UPLOAD_RETRY_FRAMES)
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

/// Frames before a failed request's first retry; each further failure doubles the wait up to
/// [`MAX_REQUEST_RETRY_FRAMES`].
const FIRST_RETRY_FRAMES: u32 = 2;
const MAX_REQUEST_RETRY_FRAMES: u32 = 120;

/// One view's last failed request and how many attempts to skip before retrying it.
struct Failure<K> {
    request: K,
    failures: u32,
    wait: u32,
}

/// Per-view requests whose allocation failed. A changed request (a resize or setting) retries at
/// once; an unchanged request keeps retrying with bounded backoff.
pub(crate) struct FailedRequests<K>(
    bevy::platform::collections::HashMap<bevy::prelude::Entity, Failure<K>>,
);

impl<K> Default for FailedRequests<K> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<K: PartialEq> FailedRequests<K> {
    /// Runs `allocate` unless `view`'s same request is still backing off; call once per frame
    /// while the owner needs the resource.
    pub(crate) fn attempt<T>(
        &mut self,
        view: bevy::prelude::Entity,
        request: K,
        allocate: impl FnOnce() -> Option<T>,
    ) -> Option<T> {
        let previous = self
            .0
            .get_mut(&view)
            .filter(|failure| failure.request == request);
        let failures = match previous {
            Some(failure) if failure.wait > 0 => {
                failure.wait -= 1;
                return None;
            }
            Some(failure) => failure.failures + 1,
            None => 1,
        };
        let allocated = allocate();
        if allocated.is_some() {
            self.0.remove(&view);
        } else {
            let wait = (FIRST_RETRY_FRAMES << (failures - 1).min(6)).min(MAX_REQUEST_RETRY_FRAMES);
            self.0.insert(
                view,
                Failure {
                    request,
                    failures,
                    wait,
                },
            );
        }
        allocated
    }

    /// Forgets views that no longer exist.
    pub(crate) fn retain(&mut self, mut live: impl FnMut(bevy::prelude::Entity) -> bool) {
        self.0.retain(|view, _| live(*view));
    }
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

    // A failure with nothing released still retries, backing off to a bounded interval.
    #[test]
    fn failed_uploads_retry_with_bounded_backoff_until_one_succeeds() {
        let frames_until_ready = |retry: &mut UploadRetry| {
            (1..=MAX_UPLOAD_RETRY_FRAMES)
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
        assert_eq!(frames_until_ready(&mut retry), MAX_UPLOAD_RETRY_FRAMES);
        retry.record(true, None);
        assert!(!retry.failed() && !retry.take_ready());
        retry.record(false, None);
        assert_eq!(frames_until_ready(&mut retry), MIN_RETRY_FRAMES);
    }
}

#[cfg(test)]
mod failed_request_tests {
    use super::*;
    use bevy::prelude::Entity;

    /// A failure is neither retried every frame nor cached for good.
    #[test]
    fn unchanged_failures_retry_with_bounded_backoff() {
        let mut failed = FailedRequests::default();
        let mut frames = Vec::new();
        for frame in 0..400 {
            failed.attempt(Entity::PLACEHOLDER, 1u32, || {
                frames.push(frame);
                None::<()>
            });
        }
        assert_eq!(&frames[..4], &[0, 3, 8, 17]);
        let gaps: Vec<_> = frames.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert!(
            gaps.iter()
                .all(|gap| *gap > 1 && *gap <= MAX_REQUEST_RETRY_FRAMES as usize + 1)
        );
        assert!(frames.len() > 4, "{frames:?}");
    }

    /// A new request retries at once, and a success clears the backoff.
    #[test]
    fn changed_requests_and_successes_reset_the_backoff() {
        let mut failed = FailedRequests::default();
        let view = Entity::PLACEHOLDER;
        assert_eq!(failed.attempt(view, 1u32, || None::<()>), None);
        assert_eq!(failed.attempt(view, 2, || Some(())), Some(()));
        assert_eq!(failed.attempt(view, 2, || Some(())), Some(()));
        assert_eq!(failed.attempt(view, 2, || None::<()>), None);
        let mut ran = false;
        failed.attempt(view, 2, || {
            ran = true;
            Some(())
        });
        assert!(!ran);
    }
}
