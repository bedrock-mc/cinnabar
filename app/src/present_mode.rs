use bevy::{
    prelude::{Entity, Query, Res, ResMut, Resource, With},
    window::{PresentMode, PrimaryWindow, Window},
};
use render::{PresentModePolicy, PresentModePreference, PresentModeRemedy, window_present_mode};
use render_api::FrameRateLimit;
use render_model::{
    DisplayTiming, PresentModeKind, PresentationIntent, initial_present_mode, select_present_mode,
};

use crate::settings_runtime::RuntimeSettings;

/// The session's presentation choice; the only writer of the primary window's present mode.
#[derive(Resource, Debug)]
pub(crate) struct PresentModeRuntime {
    policy: PresentModePolicy,
    locked: bool,
    /// A hidden developer surface never reaches a display, so it never waits for one.
    hidden_surface: bool,
    observed_settings_generation: u64,
    /// Window that adopted the driver remedy; kept until the preference or window changes,
    /// since the render world withdraws its recommendation once Immediate is requested.
    remedy_adopted: Option<Entity>,
    /// `--frame-cap`, which outranks the saved limit for the whole session.
    launch_limit: Option<FrameRateLimit>,
    /// The session's effective limit, shared with the pacer's cadence.
    limit: FrameRateLimit,
}

/// The primary window's display timing, refreshed by the main thread.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DisplayRefresh(pub(crate) DisplayTiming);

impl PresentModeRuntime {
    #[must_use]
    pub(crate) fn from_startup(
        force_vsync: bool,
        no_vsync: bool,
        attributable_evidence: bool,
        hidden_surface: bool,
    ) -> Self {
        let preference = if no_vsync {
            PresentModePreference::NoVsync
        } else if force_vsync || attributable_evidence {
            PresentModePreference::Vsync
        } else {
            PresentModePreference::Auto
        };
        Self {
            policy: PresentModePolicy::new(preference),
            locked: force_vsync || no_vsync || attributable_evidence,
            hidden_surface,
            observed_settings_generation: 0,
            remedy_adopted: None,
            launch_limit: None,
            limit: FrameRateLimit::Automatic,
        }
    }

    /// Applies `--frame-cap` in place of the saved limit for this session.
    #[must_use]
    pub(crate) fn with_launch_frame_cap(mut self, fps: Option<u32>) -> Self {
        self.launch_limit = fps
            .and_then(|fps| u16::try_from(fps).ok())
            .and_then(std::num::NonZeroU16::new)
            .map(FrameRateLimit::Fixed);
        if let Some(limit) = self.launch_limit {
            self.limit = limit;
        }
        self
    }

    /// The frame-rate limit in force: the launch cap, else the saved setting.
    #[must_use]
    pub(crate) const fn limit(&self) -> FrameRateLimit {
        self.limit
    }

    #[must_use]
    pub(crate) fn policy(&self) -> PresentModePolicy {
        self.policy.clone()
    }

    /// Returns the session VSync state a launch flag or evidence run pins, if any.
    #[must_use]
    pub(crate) fn vsync_override(&self) -> Option<bool> {
        self.locked
            .then(|| self.policy.preference().intent() == PresentationIntent::Synchronized)
    }

    /// What presentation currently optimises for; hidden surfaces never wait for a display.
    #[must_use]
    pub(crate) fn intent(&self) -> PresentationIntent {
        if self.hidden_surface {
            PresentationIntent::Unpaced
        } else {
            self.policy.preference().intent()
        }
    }

    /// The window's present mode now: the probed surface's best mode for the intent, the driver
    /// remedy, or a request the renderer can fall back from before the probe completes.
    #[must_use]
    pub(crate) fn window_present_mode(&self) -> PresentMode {
        window_present_mode(self.selected_mode())
    }

    fn remedy_eligible(&self) -> bool {
        !self.hidden_surface && self.policy.preference() == PresentModePreference::Auto
    }

    fn selected_mode(&self) -> PresentModeKind {
        if self.remedy_eligible()
            && (self.remedy_adopted.is_some()
                || self.policy.remedy() == PresentModeRemedy::UseImmediate)
        {
            return PresentModeKind::Immediate;
        }
        self.policy.capabilities().map_or_else(
            || initial_present_mode(self.intent()),
            |supported| select_present_mode(self.intent(), supported),
        )
    }

    #[cfg(test)]
    const fn locked(&self) -> bool {
        self.locked
    }

    #[cfg(test)]
    const fn observed_settings_generation(&self) -> u64 {
        self.observed_settings_generation
    }
}

/// Applies the VSync setting and keeps the window on the policy's present mode.
pub(crate) fn apply_present_mode(
    settings: Res<RuntimeSettings>,
    mut runtime: ResMut<PresentModeRuntime>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
) {
    let Ok((entity, mut window)) = windows.single_mut() else {
        return;
    };
    let (generation, user_settings) = settings.user_settings_update();
    if generation > runtime.observed_settings_generation {
        if !runtime.locked {
            // VSync on stays automatic so the driver remedy can still apply.
            let preference = if user_settings.video.vsync {
                PresentModePreference::Auto
            } else {
                PresentModePreference::NoVsync
            };
            if preference != runtime.policy.preference() {
                runtime.remedy_adopted = None;
            }
            runtime.policy.set_preference(preference);
        }
        runtime.limit = runtime
            .launch_limit
            .unwrap_or(user_settings.video.frame_rate_limit);
        runtime.observed_settings_generation = generation;
    }
    if runtime
        .remedy_adopted
        .is_some_and(|adopted| adopted != entity)
    {
        runtime.remedy_adopted = None;
    }
    if runtime.remedy_eligible() && runtime.policy.remedy() == PresentModeRemedy::UseImmediate {
        runtime.remedy_adopted = Some(entity);
    }
    let selected = runtime.selected_mode();
    runtime
        .policy
        .publish_selection(runtime.policy.capabilities().map(|_| selected));
    set_present_mode_if_changed(&mut window, window_present_mode(selected));
    let latency = Some(render::frame_latency_for_vsync(
        runtime.intent() == PresentationIntent::Synchronized,
    ));
    if window.desired_maximum_frame_latency != latency {
        window.desired_maximum_frame_latency = latency;
    }
}

fn set_present_mode_if_changed(window: &mut Window, present_mode: PresentMode) -> bool {
    if window.present_mode == present_mode {
        false
    } else {
        window.present_mode = present_mode;
        true
    }
}

#[cfg(test)]
#[path = "present_mode/tests.rs"]
mod tests;
