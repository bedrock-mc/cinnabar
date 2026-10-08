use bevy::{
    prelude::{Query, Res, ResMut, Resource, With},
    window::{PresentMode, PrimaryWindow, Window},
};
use render::{PresentModePolicy, PresentModePreference, PresentModeRemedy, window_present_mode};
use render_model::{PresentationIntent, initial_present_mode, select_present_mode};

use crate::settings_runtime::RuntimeSettings;

/// The session's presentation choice; the only writer of the primary window's present mode.
#[derive(Resource, Debug)]
pub(crate) struct PresentModeRuntime {
    policy: PresentModePolicy,
    locked: bool,
    /// A hidden developer surface never reaches a display, so it never waits for one.
    hidden_surface: bool,
    observed_settings_generation: u64,
}

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
        }
    }

    #[must_use]
    pub(crate) fn policy(&self) -> PresentModePolicy {
        self.policy.clone()
    }

    /// Returns the session VSync state a launch flag or evidence run pins, if any.
    #[must_use]
    pub(crate) fn vsync_override(&self) -> Option<bool> {
        self.locked
            .then(|| self.policy.preference() != PresentModePreference::NoVsync)
    }

    /// The window's present mode now: the probed surface's best mode for the intent, the driver
    /// remedy, or a request the renderer can fall back from before the probe completes.
    #[must_use]
    pub(crate) fn window_present_mode(&self) -> PresentMode {
        let preference = self.policy.preference();
        if !self.hidden_surface
            && preference == PresentModePreference::Auto
            && self.policy.remedy() == PresentModeRemedy::UseImmediate
        {
            return PresentMode::Immediate;
        }
        let intent = if self.hidden_surface {
            PresentationIntent::LowLatency
        } else {
            preference.intent()
        };
        window_present_mode(self.policy.capabilities().map_or_else(
            || initial_present_mode(intent),
            |supported| select_present_mode(intent, supported),
        ))
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
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let (generation, user_settings) = settings.user_settings_update();
    if generation > runtime.observed_settings_generation {
        if !runtime.locked {
            // VSync on stays automatic so the driver remedy can still apply.
            runtime.policy.set_preference(if user_settings.video.vsync {
                PresentModePreference::Auto
            } else {
                PresentModePreference::NoVsync
            });
        }
        runtime.observed_settings_generation = generation;
    }
    set_present_mode_if_changed(&mut window, runtime.window_present_mode());
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
