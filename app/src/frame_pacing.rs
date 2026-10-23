//! Chooses the frame admission cadence from the session's frame-rate limit and window state.

mod display;
#[cfg(target_os = "macos")]
pub(crate) mod macos_display_link;
#[cfg(target_os = "macos")]
pub(crate) mod macos_presentation;
mod power;

use std::time::{Duration, Instant};

use bevy::{
    ecs::system::{NonSendMarker, SystemParam},
    prelude::{Entity, Local, MessageReader, Query, Res, ResMut, Resource, With},
    window::{PresentMode, PrimaryWindow, Window, WindowOccluded},
    winit::WINIT_WINDOWS,
};
use render::FramePacing;
use render_api::FrameRateLimit;
use render_model::{
    DisplayTiming, FrameRate, FrameRateContext, PresentationIntent, WindowActivity,
    effective_frame_rate, frame_rate_target,
};

use crate::present_mode::{DisplayRefresh, PresentModeRuntime};

/// Monitors can change refresh while a window stays put, so it is re-read this often.
const DISPLAY_RECHECK: Duration = Duration::from_secs(1);

/// Active desktop power source, refreshed alongside the display instead of every frame.
#[derive(Resource, Default)]
pub(crate) struct PowerSource {
    on_battery: bool,
}

/// Display, power and session facts used to choose the current frame-rate context.
#[derive(SystemParam)]
pub(crate) struct FramePacingEnvironment<'w> {
    display: Res<'w, DisplayRefresh>,
    power: Res<'w, PowerSource>,
    world: Option<Res<'w, crate::runtime::world::ClientWorld>>,
    ui: Option<Res<'w, client_shell::ui_resources::UiPresentationResource>>,
    failure: Option<Res<'w, crate::runtime::shutdown::FatalError>>,
}

impl FramePacingEnvironment<'_> {
    /// Uses the menu ceiling outside a ready world, while diagnostic runs keep their cadence.
    fn context(&self, ignore_window_state: bool) -> FrameRateContext {
        if ignore_window_state
            || (self
                .world
                .as_ref()
                .is_some_and(|world| world.stream.is_some())
                && self
                    .ui
                    .as_ref()
                    .is_none_or(|ui| ui.loading_stage().is_none())
                && self
                    .failure
                    .as_ref()
                    .is_none_or(|failure| failure.0.is_none()))
        {
            FrameRateContext::Gameplay {
                on_battery: !ignore_window_state && self.power.on_battery,
            }
        } else {
            FrameRateContext::Menus
        }
    }
}

/// Session inputs to the cadence that the saved settings do not carry.
#[derive(Resource, Debug)]
pub(crate) struct FramePacingRuntime {
    /// Hidden developer surfaces and acceptance runs keep their requested cadence in the
    /// background, so window state never skews their measurements.
    ignore_window_state: bool,
    /// Fixed-clock recordings lift ordinary pacing while retaining required limits.
    suspended: bool,
    occluded: bool,
    required_limit: Option<FrameRate>,
}

impl FramePacingRuntime {
    pub(crate) fn new(ignore_window_state: bool) -> Self {
        Self {
            ignore_window_state,
            suspended: false,
            occluded: false,
            required_limit: None,
        }
    }

    /// Sets an immutable upper rate that remains active when recordings lift ordinary pacing.
    #[cfg(any(test, feature = "enhanced-diagnostics"))]
    pub(crate) fn require_limit(&mut self, rate: FrameRate) {
        self.required_limit = Some(
            self.required_limit
                .map_or(rate, |current| current.min(rate)),
        );
    }

    /// Lifts the cadence while a fixed-clock recording steps time per frame.
    #[cfg(any(test, feature = "developer-control"))]
    pub(crate) fn set_suspended(&mut self, suspended: bool) {
        self.suspended = suspended;
    }

    fn pacing(
        &self,
        intent: PresentationIntent,
        present_mode: PresentMode,
        limit: FrameRateLimit,
        display: DisplayTiming,
        focused: bool,
        context: FrameRateContext,
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
        let requested = frame_rate_target(intent, limit, display, context);
        let ordinary = (!self.suspended)
            .then(|| effective_frame_rate(requested, activity))
            .flatten();
        let rate = match (ordinary, self.required_limit) {
            (Some(ordinary), Some(required)) => Some(ordinary.min(required)),
            (ordinary, required) => ordinary.or(required),
        };
        FramePacing {
            rate,
            precise: activity == WindowActivity::Focused,
            display_sync: matches!(
                present_mode,
                PresentMode::Fifo | PresentMode::FifoRelaxed | PresentMode::AutoVsync
            ),
        }
    }

    /// Requests the display's full cadence for focused, synchronized native windows,
    /// bounded by the same limits that admit frames. Hidden and fixed-clock runs opt out.
    #[cfg(any(target_os = "macos", test))]
    fn display_link_rate(
        &self,
        intent: PresentationIntent,
        display: DisplayTiming,
        focused: bool,
        pacing: FramePacing,
    ) -> Option<FrameRate> {
        if intent != PresentationIntent::Synchronized
            || !focused
            || self.occluded
            || self.ignore_window_state
            || self.suspended
        {
            return None;
        }
        display
            .refresh
            .map(|refresh| pacing.rate.map_or(refresh, |rate| refresh.min(rate)))
    }
}

/// Publishes the cadence the next update's input sample is admitted at.
pub(crate) fn update_frame_pacing(
    mut runtime: ResMut<FramePacingRuntime>,
    presentation: Res<PresentModeRuntime>,
    settings: Res<client_state::RuntimeSettings>,
    environment: FramePacingEnvironment,
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
    let context = environment.context(runtime.ignore_window_state);
    let next = runtime.pacing(
        presentation.intent(),
        presentation.window_present_mode(),
        presentation.limit(),
        environment
            .display
            .0
            .with_vrr_preference(settings.user_settings_update().1.video.vrr),
        focused,
        context,
    );
    if *pacing != next {
        *pacing = next;
    }
}

/// Re-reads the primary window's display timing, clearing stale reports if it is absent.
pub(crate) fn track_display_refresh(
    windows: Query<Entity, With<PrimaryWindow>>,
    mut display: ResMut<DisplayRefresh>,
    mut checked: Local<Option<Instant>>,
    _main_thread: NonSendMarker,
    mut power: ResMut<PowerSource>,
) {
    let now = Instant::now();
    if checked.is_some_and(|last| now.saturating_duration_since(last) < DISPLAY_RECHECK) {
        return;
    }
    *checked = Some(now);
    power.on_battery = power::on_battery();
    let timing = windows.single().ok().and_then(|window| {
        WINIT_WINDOWS.with_borrow(|windows| {
            windows
                .get_window(window)
                .map(|window| display::window_display_timing(window))
        })
    });
    let next = DisplayRefresh(timing.unwrap_or_default());
    if *display != next {
        *display = next;
    }
}

#[cfg(test)]
#[path = "frame_pacing/tests.rs"]
mod tests;
