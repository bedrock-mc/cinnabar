//! Requests native refresh for synchronized windows while Metal owns drawable presentation.
//! On iOS the link is also the app's refresh-rate request: without one, Core Animation
//! composites a ProMotion iPhone's layer at 60 Hz however fast frames are presented.

use bevy::{
    prelude::{App, Entity, IntoScheduleConfigs, Last, NonSendMut, Query, Res, With},
    window::{PrimaryWindow, Window},
    winit::WINIT_WINDOWS,
};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{NSObject, NSObjectProtocol},
    sel,
};
#[cfg(target_os = "macos")]
use objc2_app_kit::NSView;
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange};
use render::FramePacing;
use render_model::FrameRate;
#[cfg(target_os = "macos")]
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{DisplayRefresh, FramePacingRuntime, PresentModeRuntime, update_frame_pacing};

define_class!(
    // SAFETY: NSObject has no subclass requirements; the callback stays on the main thread.
    #[unsafe(super = NSObject)]
    #[name = "CinnabarDisplayRefreshTarget"]
    #[thread_kind = MainThreadOnly]
    #[ivars = Entity]
    struct RefreshTarget;

    // SAFETY: NSObjectProtocol adds no implementation requirements.
    unsafe impl NSObjectProtocol for RefreshTarget {}

    impl RefreshTarget {
        /// Wakes the window's redraw path at the requested display cadence.
        #[unsafe(method(displayRefresh:))]
        fn display_refresh(&self, _link: &CADisplayLink) {
            WINIT_WINDOWS.with_borrow(|windows| {
                if let Some(window) = windows.get_window(*self.ivars()) {
                    window.request_redraw();
                }
            });
        }
    }
);

impl RefreshTarget {
    /// Keeps only the window's entity identity; a closed window cannot be redrawn.
    fn new(window: Entity, main_thread: MainThreadMarker) -> Retained<Self> {
        let target = Self::alloc(main_thread).set_ivars(window);
        // SAFETY: NSObject's initializer accepts this initialized subclass allocation.
        unsafe { msg_send![super(target), init] }
    }
}

/// A run-loop registration whose target is retained by the link until invalidation.
struct WindowRefreshLink {
    window: Entity,
    rate: FrameRate,
    link: Retained<CADisplayLink>,
}

impl Drop for WindowRefreshLink {
    /// Removes the run-loop registration and releases its retained callback target.
    fn drop(&mut self) {
        self.link.invalidate();
    }
}

#[derive(Default)]
struct NativeRefresh(Option<WindowRefreshLink>);

/// Adds the native request after admission policy reads current settings and window state.
pub(crate) fn install(app: &mut App) {
    app.init_non_send::<NativeRefresh>()
        .add_systems(Last, update_refresh.after(update_frame_pacing));
}

/// Changes native demand only when the window or its requested cadence changes.
fn update_refresh(
    runtime: Res<FramePacingRuntime>,
    presentation: Res<PresentModeRuntime>,
    display: Res<DisplayRefresh>,
    pacing: Res<FramePacing>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    mut native: NonSendMut<NativeRefresh>,
) {
    let requested = windows.single().ok().and_then(|(window, state)| {
        runtime
            .display_link_rate(
                presentation.intent(),
                display.0,
                state.focused && state.visible,
                *pacing,
            )
            .map(|rate| (window, rate))
    });
    let Some((window, rate)) = requested else {
        native.0 = None;
        return;
    };
    if let Some(current) = &mut native.0
        && current.window == window
    {
        if current.rate != rate {
            set_rate(&current.link, rate);
            current.rate = rate;
        }
        return;
    }
    native.0 = None;
    native.0 = window_link(window, rate);
}

/// Registers the window's link on the main run loop; systems without links keep FIFO pacing.
fn window_link(window: Entity, rate: FrameRate) -> Option<WindowRefreshLink> {
    let link = native_link(window, MainThreadMarker::new()?)?;
    set_rate(&link, rate);
    // SAFETY: This resource and its target live on the main thread that runs this run loop.
    unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
    Some(WindowRefreshLink { window, rate, link })
}

/// Binds to the window so AppKit follows display changes; macOS before 14 has no window links.
#[cfg(target_os = "macos")]
fn native_link(window: Entity, main_thread: MainThreadMarker) -> Option<Retained<CADisplayLink>> {
    WINIT_WINDOWS.with_borrow(|windows| {
        let window_handle = windows.get_window(window)?;
        let RawWindowHandle::AppKit(handle) = window_handle.window_handle().ok()?.as_raw() else {
            return None;
        };
        // SAFETY: winit owns the view and AppKit access stays on the main thread.
        let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        let native_window = view.window()?;
        if !native_window.respondsToSelector(sel!(displayLinkWithTarget:selector:)) {
            return None;
        }
        let target = RefreshTarget::new(window, main_thread);
        // SAFETY: The selector has the documented void(CADisplayLink *) signature, and
        // the availability check above covers macOS versions before display-link support.
        Some(unsafe {
            native_window.displayLinkWithTarget_selector(&target, sel!(displayRefresh:))
        })
    })
}

/// Follows the main screen, the only display a full-screen iOS app is shown on.
#[cfg(target_os = "ios")]
fn native_link(window: Entity, main_thread: MainThreadMarker) -> Option<Retained<CADisplayLink>> {
    WINIT_WINDOWS.with_borrow(|windows| windows.get_window(window).map(drop))?;
    let target = RefreshTarget::new(window, main_thread);
    // SAFETY: The selector has the documented void(CADisplayLink *) signature.
    Some(unsafe { CADisplayLink::displayLinkWithTarget_selector(&target, sel!(displayRefresh:)) })
}

/// Makes a precise refresh request without adding a second software frame limiter.
fn set_rate(link: &CADisplayLink, rate: FrameRate) {
    let hz = rate.millihertz() as f32 / 1_000.0;
    link.setPreferredFrameRateRange(CAFrameRateRange {
        minimum: hz,
        maximum: hz,
        preferred: hz,
    });
}
