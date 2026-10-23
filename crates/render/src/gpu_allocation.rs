//! Recoverable render allocations: out-of-memory skips the owner's work instead of
//! quitting. Other GPU errors remain fatal.

use crate::queue_writes::ProfiledQueue as _;
use bevy::render::{
    render_resource::{Buffer, BufferDescriptor, BufferUsages},
    renderer::{RenderDevice, RenderQueue},
};
use std::sync::{
    Arc, Mutex, OnceLock, Weak,
    atomic::{AtomicBool, Ordering},
};

type DeviceKey = (wgpu::Device, usize);
type FailureObservers = std::collections::HashMap<DeviceKey, Weak<AtomicBool>>;

/// Distinguishes devices whose wgpu IDs may repeat across independent native instances.
fn device_key(device: &RenderDevice) -> DeviceKey {
    #[cfg(not(target_arch = "wasm32"))]
    let address = {
        let mut address = native_device_address::<wgpu::hal::api::Noop>(device.wgpu_device());
        #[cfg(target_vendor = "apple")]
        {
            address = address
                .or_else(|| native_device_address::<wgpu::hal::api::Metal>(device.wgpu_device()));
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            address = address
                .or_else(|| native_device_address::<wgpu::hal::api::Vulkan>(device.wgpu_device()));
        }
        #[cfg(target_os = "windows")]
        {
            address = address
                .or_else(|| native_device_address::<wgpu::hal::api::Dx12>(device.wgpu_device()));
        }
        address.unwrap_or(0)
    };
    #[cfg(target_arch = "wasm32")]
    let address = 0;
    (device.wgpu_device().clone(), address)
}

/// Borrows a native device only to identify its stable address; it is never modified.
#[cfg(not(target_arch = "wasm32"))]
fn native_device_address<A: wgpu::hal::Api>(device: &wgpu::Device) -> Option<usize> {
    // SAFETY: The guard keeps the device alive; reading its address does not use or destroy it.
    unsafe { device.as_hal::<A>() }.map(|guard| std::ptr::from_ref(&*guard).cast::<()>().addr())
}

/// Returns the device-scoped observers without keeping a renderer alive.
fn observers() -> &'static Mutex<FailureObservers> {
    static OBSERVERS: OnceLock<Mutex<FailureObservers>> = OnceLock::new();
    OBSERVERS.get_or_init(Default::default)
}

/// Remembers allocation failures across worker threads until the next terrain publication.
pub(crate) struct FailureFlag {
    device: DeviceKey,
    failed: Arc<AtomicBool>,
}

impl FailureFlag {
    /// Watches allocations on this renderer's device independently of other renderers.
    pub(crate) fn new(device: &RenderDevice) -> Self {
        let failed = Arc::new(AtomicBool::new(false));
        let device = device_key(device);
        observers()
            .lock()
            .unwrap()
            .insert(device.clone(), Arc::downgrade(&failed));
        Self { device, failed }
    }

    /// Whether any checked allocation failed since the last recovery began.
    pub(crate) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    /// Starts a new publication while retaining failures raised during its work.
    pub(crate) fn take(&self) -> bool {
        self.failed.swap(false, Ordering::AcqRel)
    }
}

impl Drop for FailureFlag {
    /// Removes the observer when its renderer goes away.
    fn drop(&mut self) {
        observers().lock().unwrap().remove(&self.device);
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
    checked_with_owner(device, label, bytes, false, create)
}

/// Whether failed growth makes a draw invalid or only loses spare capacity.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Requirement {
    Required,
    Optional,
}

impl Requirement {
    /// Invalidates terrain only when the allocation is required by a draw.
    pub(crate) fn checked<T>(
        self,
        device: &RenderDevice,
        label: &str,
        bytes: u64,
        create: impl FnOnce() -> T,
    ) -> Option<T> {
        match self {
            Self::Required => terrain_checked(device, label, bytes, create),
            Self::Optional => checked(device, label, bytes, create),
        }
    }
}

/// Captures allocation failures while identifying which publication owns the request.
fn checked_with_owner<T>(
    device: &RenderDevice,
    label: &str,
    bytes: u64,
    _terrain: bool,
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
        || FAILURE.with_borrow_mut(|hook| {
            hook.as_mut()
                .is_some_and(|hook| hook(label, bytes, _terrain))
        });
    if failed {
        warn_once(label, bytes);
        None
    } else {
        Some(resource)
    }
}

/// A required terrain write could not complete; its entire generation must be discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerrainWriteError;

thread_local! {
    static ISOLATED_TERRAIN: std::cell::RefCell<Option<Arc<AtomicBool>>> = const { std::cell::RefCell::new(None) };
}

/// Builds an invisible generation whose failures cannot invalidate the live arena.
pub(crate) fn isolated_terrain<T>(work: impl FnOnce() -> Option<T>) -> Option<T> {
    struct Restore(Option<Arc<AtomicBool>>);
    impl Drop for Restore {
        /// Restores the caller's generation even when construction unwinds.
        fn drop(&mut self) {
            ISOLATED_TERRAIN.with_borrow_mut(|slot| *slot = self.0.take());
        }
    }
    let failed = Arc::new(AtomicBool::new(false));
    let _restore =
        Restore(ISOLATED_TERRAIN.with_borrow_mut(|slot| slot.replace(Arc::clone(&failed))));
    let result = work();
    if failed.load(Ordering::Acquire) {
        None
    } else {
        result
    }
}

/// Checks a required allocation against the current terrain generation.
#[must_use]
pub(crate) fn terrain_checked<T>(
    device: &RenderDevice,
    label: &str,
    bytes: u64,
    create: impl FnOnce() -> T,
) -> Option<T> {
    let observer = ISOLATED_TERRAIN.with_borrow(Clone::clone).or_else(|| {
        observers()
            .lock()
            .unwrap()
            .get(&device_key(device))
            .and_then(Weak::upgrade)
    });
    if observer
        .as_ref()
        .is_some_and(|flag| flag.load(Ordering::Acquire))
    {
        return None;
    }
    let resource = checked_with_owner(device, label, bytes, true, create);
    if resource.is_none()
        && let Some(observer) = observer
    {
        observer.store(true, Ordering::Release);
    }
    resource
}

/// Returns an explicit error when a required upload fails its generation.
pub(crate) fn terrain_write<T>(
    device: &RenderDevice,
    label: &str,
    bytes: u64,
    write: impl FnOnce() -> T,
) -> Result<T, TerrainWriteError> {
    terrain_checked(device, label, bytes, write).ok_or(TerrainWriteError)
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

/// Checks terrain buffer creation before it can be copied, bound, or uploaded.
pub(crate) fn terrain_buffer(
    device: &RenderDevice,
    descriptor: &BufferDescriptor<'_>,
) -> Option<Buffer> {
    terrain_checked(
        device,
        descriptor.label.unwrap_or("terrain buffer"),
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
/// [`MAX_RETRY_FRAMES`].
const FIRST_RETRY_FRAMES: u32 = 2;
const MAX_RETRY_FRAMES: u32 = 120;

/// Bounds the wait after consecutive allocation failures, starting at the first failure.
pub(crate) fn retry_frames(failures: u32) -> u32 {
    (FIRST_RETRY_FRAMES << failures.saturating_sub(1).min(6)).min(MAX_RETRY_FRAMES)
}

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
            let wait = retry_frames(failures);
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
type FailureHook = Box<dyn FnMut(&str, u64, bool) -> bool>;
#[cfg(test)]
thread_local! { static FAILURE: std::cell::RefCell<Option<FailureHook>> = const { std::cell::RefCell::new(None) }; }

/// Installs a scoped, thread-local failure injector at the real error-scope boundary.
#[cfg(test)]
pub(crate) fn inject(mut hook: impl FnMut(&str, u64) -> bool + 'static) -> impl Drop {
    inject_with_owner(move |label, bytes, _| hook(label, bytes))
}

/// Injects failures with a flag identifying requests that invalidate terrain publication.
#[cfg(test)]
pub(crate) fn inject_with_owner(hook: impl FnMut(&str, u64, bool) -> bool + 'static) -> impl Drop {
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
mod failure_tests {
    use super::*;

    /// A worker's terrain failure dirties only its renderer and survives until consumed.
    #[test]
    fn terrain_failures_cross_threads_without_affecting_another_device() {
        let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let device = RenderDevice::from(device);
        let flag = FailureFlag::new(&device);
        let (other, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let other = FailureFlag::new(&RenderDevice::from(other));
        std::thread::spawn(move || {
            let _failure = inject(|_, _| true);
            assert!(terrain_checked(&device, "test terrain allocation", 4, || ()).is_none());
        })
        .join()
        .unwrap();
        assert!(flag.failed());
        assert!(!other.failed());
        assert!(flag.take());
        assert!(!flag.take());
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
                .all(|gap| *gap > 1 && *gap <= MAX_RETRY_FRAMES as usize + 1)
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
