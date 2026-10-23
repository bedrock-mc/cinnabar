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

/// Reads the supported refresh ceiling; an adaptive range does not prove VRR is active.
#[cfg(any(target_os = "macos", test))]
fn timing_from_intervals(minimum: f64, maximum: f64) -> Option<DisplayTiming> {
    if !minimum.is_finite() || !maximum.is_finite() || minimum <= 0.0 || maximum < minimum {
        return None;
    }
    let millihertz = (1_000.0 / minimum).round();
    if !(1.0..=f64::from(u32::MAX)).contains(&millihertz) {
        return None;
    }
    Some(DisplayTiming {
        refresh: FrameRate::from_millihertz(millihertz as u32),
        vrr: if maximum > minimum {
            VrrStatus::Unknown
        } else {
            VrrStatus::Inactive
        },
    })
}

/// Queries the NSScreen attached to this window, on AppKit's main thread.
#[cfg(target_os = "macos")]
fn macos_display_timing(window: &winit::window::Window) -> Option<DisplayTiming> {
    use objc2::{MainThreadMarker, runtime::NSObjectProtocol, sel};
    use objc2_app_kit::NSView;
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
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_refresh_capability_does_not_cap_vsync_off() {
        use render_api::{FrameRateLimit, VrrPreference};
        use render_model::{FrameRateContext, PresentationIntent, frame_rate_target};

        let timing = timing_from_intervals(1.0 / 120.0, 1.0 / 48.0).unwrap();
        let context = FrameRateContext::Gameplay { on_battery: false };
        for intent in [
            PresentationIntent::LowLatency,
            PresentationIntent::AllowTearing,
        ] {
            assert_eq!(
                frame_rate_target(
                    intent,
                    FrameRateLimit::Unlimited,
                    timing.with_vrr_preference(VrrPreference::Automatic),
                    context,
                ),
                None,
            );
            assert_eq!(
                frame_rate_target(
                    intent,
                    FrameRateLimit::Unlimited,
                    timing.with_vrr_preference(VrrPreference::On),
                    context,
                ),
                FrameRate::from_hz(116),
            );
        }
    }

    #[test]
    fn refresh_intervals_report_capabilities_without_claiming_active_vrr() {
        let fastest = 1.0 / 120.0;
        for (slowest, status) in [
            (1.0 / 48.0, VrrStatus::Unknown),
            (fastest, VrrStatus::Inactive),
        ] {
            assert_eq!(
                timing_from_intervals(fastest, slowest),
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
            assert_eq!(timing_from_intervals(minimum, maximum), None);
        }
    }
}
