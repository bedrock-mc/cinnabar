use bevy::{
    prelude::{Query, Res, ResMut, Resource, With},
    window::{PresentMode, PrimaryWindow, Window},
};
use render::{Dx12PresentModePolicy, PresentModePreference};

use crate::settings_runtime::RuntimeSettings;

#[derive(Resource, Debug)]
pub(crate) struct PresentModeRuntime {
    policy: Dx12PresentModePolicy,
    locked: bool,
    observed_settings_generation: u64,
}

impl PresentModeRuntime {
    #[must_use]
    pub(crate) fn from_startup(
        force_vsync: bool,
        no_vsync: bool,
        attributable_evidence: bool,
    ) -> Self {
        let preference = if no_vsync {
            PresentModePreference::NoVsync
        } else if force_vsync || attributable_evidence {
            PresentModePreference::Vsync
        } else {
            PresentModePreference::Auto
        };
        Self {
            policy: Dx12PresentModePolicy::new(preference),
            locked: force_vsync || no_vsync || attributable_evidence,
            observed_settings_generation: 0,
        }
    }

    #[must_use]
    pub(crate) fn policy(&self) -> Dx12PresentModePolicy {
        self.policy.clone()
    }

    #[cfg(test)]
    const fn locked(&self) -> bool {
        self.locked
    }
}

pub(crate) fn apply_runtime_vsync_setting(
    settings: Res<RuntimeSettings>,
    mut runtime: ResMut<PresentModeRuntime>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let (generation, user_settings) = settings.user_settings_update();
    if generation == 0 || generation <= runtime.observed_settings_generation {
        return;
    }
    runtime.observed_settings_generation = generation;
    if runtime.locked {
        return;
    }
    let (preference, present_mode) = if user_settings.video.vsync {
        (PresentModePreference::Vsync, PresentMode::Fifo)
    } else {
        (PresentModePreference::NoVsync, PresentMode::Immediate)
    };
    runtime.policy.set_preference(preference);
    if let Ok(mut window) = windows.single_mut() {
        window.present_mode = present_mode;
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::{App, MinimalPlugins};
    use render::PresentModePreference;

    use super::*;

    #[test]
    fn attributable_runs_and_explicit_flags_lock_the_requested_policy() {
        let automatic = PresentModeRuntime::from_startup(false, false, false);
        assert!(!automatic.locked());
        assert_eq!(automatic.policy.preference(), PresentModePreference::Auto);

        for runtime in [
            PresentModeRuntime::from_startup(true, false, false),
            PresentModeRuntime::from_startup(false, true, false),
            PresentModeRuntime::from_startup(false, false, true),
        ] {
            assert!(runtime.locked());
        }
    }

    #[test]
    fn a_new_user_video_setting_replaces_auto_but_not_a_locked_cli_policy() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<RuntimeSettings>()
            .insert_resource(PresentModeRuntime::from_startup(false, false, false))
            .add_systems(bevy::prelude::Update, apply_runtime_vsync_setting);
        app.world_mut().spawn((Window::default(), PrimaryWindow));

        let mut settings = ui::UserSettings::default();
        settings.video.vsync = false;
        app.world_mut()
            .resource_mut::<RuntimeSettings>()
            .replace_user_settings(settings);
        app.update();

        assert_eq!(
            app.world().resource::<PresentModeRuntime>().policy.preference(),
            PresentModePreference::NoVsync
        );
        let present_mode = {
            let world = app.world_mut();
            let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
            windows.single(world).unwrap().present_mode
        };
        assert_eq!(present_mode, PresentMode::Immediate);
    }
}
