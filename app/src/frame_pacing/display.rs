//! Native display timing for the primary window; unsupported platforms stay unknown.

use render_model::{DisplayTiming, FrameRate, VrrStatus};

/// Reads the current window's timing without inferring VRR from its refresh rate alone.
pub(super) fn window_display_timing(window: &winit::window::Window) -> DisplayTiming {
    #[cfg(target_os = "macos")]
    if let Some(timing) = macos_display_timing(window) {
        return timing;
    }
    DisplayTiming {
        refresh: window
            .current_monitor()
            .and_then(|monitor| monitor.refresh_rate_millihertz())
            .and_then(FrameRate::from_millihertz),
        vrr: VrrStatus::Unknown,
    }
}

/// Converts AppKit's current refresh intervals, requiring native fullscreen for adaptive sync.
#[cfg(any(target_os = "macos", test))]
fn timing_from_intervals(minimum: f64, maximum: f64, fullscreen: bool) -> Option<DisplayTiming> {
    if !minimum.is_finite() || !maximum.is_finite() || minimum <= 0.0 || maximum < minimum {
        return None;
    }
    let millihertz = (1_000.0 / minimum).round();
    if !(1.0..=f64::from(u32::MAX)).contains(&millihertz) {
        return None;
    }
    Some(DisplayTiming {
        refresh: FrameRate::from_millihertz(millihertz as u32),
        vrr: if fullscreen && maximum > minimum {
            VrrStatus::Active
        } else {
            VrrStatus::Inactive
        },
    })
}

/// Queries the NSScreen attached to this window, on AppKit's main thread.
#[cfg(target_os = "macos")]
fn macos_display_timing(window: &winit::window::Window) -> Option<DisplayTiming> {
    use objc2::{MainThreadMarker, runtime::NSObjectProtocol, sel};
    use objc2_app_kit::{NSView, NSWindowStyleMask};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let _main_thread = MainThreadMarker::new()?;
    let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    // SAFETY: winit owns this live NSView; AppKit access stays on the main thread.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let native_window = view.window()?;
    let screen = native_window.screen()?;
    if !screen.respondsToSelector(sel!(minimumRefreshInterval))
        || !screen.respondsToSelector(sel!(maximumRefreshInterval))
    {
        return None;
    }
    timing_from_intervals(
        screen.minimumRefreshInterval(),
        screen.maximumRefreshInterval(),
        native_window
            .styleMask()
            .contains(NSWindowStyleMask::FullScreen)
            && native_window.isVisible()
            && !native_window.isMiniaturized(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vrr_requires_valid_adaptive_intervals_and_native_fullscreen() {
        let fastest = 1.0 / 120.0;
        for (slowest, fullscreen, status) in [
            (1.0 / 48.0, true, VrrStatus::Active),
            (1.0 / 48.0, false, VrrStatus::Inactive),
            (fastest, true, VrrStatus::Inactive),
            (fastest, false, VrrStatus::Inactive),
        ] {
            assert_eq!(
                timing_from_intervals(fastest, slowest, fullscreen),
                Some(DisplayTiming {
                    refresh: FrameRate::from_hz(120),
                    vrr: status,
                })
            );
        }
        for (minimum, maximum) in [
            (0.0, 0.0),
            (-1.0, 1.0),
            (fastest, 0.0),
            (f64::NAN, fastest),
            (fastest, f64::INFINITY),
            (f64::MIN_POSITIVE, fastest),
        ] {
            assert_eq!(timing_from_intervals(minimum, maximum, true), None);
        }
    }
}
