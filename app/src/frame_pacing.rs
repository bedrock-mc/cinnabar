//! Chooses the frame admission cadence from the session's frame-rate limit and window state.

use std::time::{Duration, Instant};

use bevy::{
    ecs::system::NonSendMarker,
    prelude::{Entity, Local, MessageReader, Query, Res, ResMut, Resource, With},
    window::{PrimaryWindow, Window, WindowOccluded},
    winit::WINIT_WINDOWS,
};
use render::FramePacing;
use render_api::FrameRateLimit;
use render_model::{
    DisplayTiming, FrameRate, PresentationIntent, WindowActivity, effective_frame_rate,
    frame_rate_target,
};

use crate::present_mode::{DisplayRefresh, PresentModeRuntime};

/// Monitors can change refresh while a window stays put, so it is re-read this often.
const DISPLAY_RECHECK: Duration = Duration::from_secs(1);

/// Session inputs to the cadence that the saved settings do not carry.
#[derive(Resource, Debug)]
pub(crate) struct FramePacingRuntime {
    /// Hidden developer surfaces and acceptance runs keep their requested cadence in the
    /// background, so window state never skews their measurements.
    ignore_window_state: bool,
    /// Fixed-clock recordings advance time per frame and must not be throttled.
    suspended: bool,
    occluded: bool,
}

impl FramePacingRuntime {
    pub(crate) fn new(ignore_window_state: bool) -> Self {
        Self {
            ignore_window_state,
            suspended: false,
            occluded: false,
        }
    }

    /// Lifts the cadence while a fixed-clock recording steps time per frame.
    #[cfg(any(feature = "developer-control", test))]
    pub(crate) fn set_suspended(&mut self, suspended: bool) {
        self.suspended = suspended;
    }

    fn pacing(
        &self,
        intent: PresentationIntent,
        limit: FrameRateLimit,
        display: DisplayTiming,
        focused: bool,
    ) -> FramePacing {
        let activity = if self.ignore_window_state {
            WindowActivity::Focused
        } else if self.occluded {
            WindowActivity::Occluded
        } else if focused {
            WindowActivity::Focused
        } else {
            WindowActivity::Unfocused
        };
        let requested = frame_rate_target(intent, limit, display);
        FramePacing {
            rate: (!self.suspended)
                .then(|| effective_frame_rate(requested, activity))
                .flatten(),
            precise: activity == WindowActivity::Focused,
        }
    }
}

/// Publishes the cadence the next update's input sample is admitted at.
pub(crate) fn update_frame_pacing(
    mut runtime: ResMut<FramePacingRuntime>,
    presentation: Res<PresentModeRuntime>,
    display: Res<DisplayRefresh>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    mut occlusion: MessageReader<WindowOccluded>,
    mut pacing: ResMut<FramePacing>,
) {
    let primary = windows.single().ok();
    for event in occlusion.read() {
        if primary.is_some_and(|(entity, _)| entity == event.window) {
            runtime.occluded = event.occluded;
        }
    }
    let focused = primary.is_none_or(|(_, window)| window.focused);
    let next = runtime.pacing(
        presentation.intent(),
        presentation.limit(),
        display.0,
        focused,
    );
    if *pacing != next {
        *pacing = next;
    }
}

/// Re-reads the primary window's monitor refresh; changes only when it does.
pub(crate) fn track_display_refresh(
    windows: Query<Entity, With<PrimaryWindow>>,
    mut display: ResMut<DisplayRefresh>,
    mut checked: Local<Option<Instant>>,
    _main_thread: NonSendMarker,
) {
    let now = Instant::now();
    if checked.is_some_and(|last| now.saturating_duration_since(last) < DISPLAY_RECHECK) {
        return;
    }
    *checked = Some(now);
    let refresh = windows.single().ok().and_then(|window| {
        WINIT_WINDOWS.with_borrow(|windows| {
            windows
                .get_window(window)?
                .current_monitor()?
                .refresh_rate_millihertz()
        })
    });
    let next = DisplayRefresh(DisplayTiming {
        refresh: refresh.and_then(FrameRate::from_millihertz),
        ..display.0
    });
    if *display != next {
        *display = next;
    }
}

#[cfg(test)]
#[path = "frame_pacing/tests.rs"]
mod tests;
