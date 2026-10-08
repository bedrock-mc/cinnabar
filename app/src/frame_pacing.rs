//! Chooses the frame admission cadence from the frame-rate setting, launch flags and window state.

use bevy::{
    prelude::{Entity, MessageReader, Query, Res, ResMut, Resource, With},
    window::{PrimaryWindow, Window, WindowOccluded},
};
use render::FramePacing;
use render_model::{FrameRate, WindowActivity, effective_frame_rate};

use crate::settings_runtime::RuntimeSettings;

/// Session inputs to the cadence that the saved settings do not carry.
#[derive(Resource, Debug)]
pub(crate) struct FramePacingRuntime {
    /// `--frame-cap`, which outranks the saved setting for the session.
    launch_cap: Option<FrameRate>,
    /// Hidden developer surfaces and acceptance runs keep their requested cadence in the
    /// background, so window state never skews their measurements.
    ignore_window_state: bool,
    /// Fixed-clock recordings advance time per frame and must not be throttled.
    suspended: bool,
    occluded: bool,
}

impl FramePacingRuntime {
    pub(crate) fn new(launch_cap: Option<u32>, ignore_window_state: bool) -> Self {
        Self {
            launch_cap: launch_cap.and_then(FrameRate::from_hz),
            ignore_window_state,
            suspended: false,
            occluded: false,
        }
    }

    /// Lifts the cadence while a fixed-clock recording steps time per frame.
    pub(crate) fn set_suspended(&mut self, suspended: bool) {
        self.suspended = suspended;
    }

    fn pacing(&self, saved_cap: Option<u16>, focused: bool) -> FramePacing {
        let activity = if self.ignore_window_state {
            WindowActivity::Focused
        } else if self.occluded {
            WindowActivity::Occluded
        } else if focused {
            WindowActivity::Focused
        } else {
            WindowActivity::Unfocused
        };
        let requested = self
            .launch_cap
            .or_else(|| saved_cap.and_then(|cap| FrameRate::from_hz(u32::from(cap))));
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
    settings: Res<RuntimeSettings>,
    mut runtime: ResMut<FramePacingRuntime>,
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
    let next = runtime.pacing(settings.user_settings_update().1.video.frame_cap, focused);
    if *pacing != next {
        *pacing = next;
    }
}

#[cfg(test)]
#[path = "frame_pacing/tests.rs"]
mod tests;
