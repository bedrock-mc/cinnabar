use bevy::{
    prelude::{Query, Res, ResMut, Resource, With},
    window::{PresentMode, PrimaryWindow, Window},
};
use render::{Dx12PresentModePolicy, PresentModePreference, PresentModeRemedy};

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

    #[cfg(test)]
    const fn observed_settings_generation(&self) -> u64 {
        self.observed_settings_generation
    }
}

pub(crate) fn apply_runtime_vsync_setting(
    settings: Res<RuntimeSettings>,
    mut runtime: ResMut<PresentModeRuntime>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let (generation, user_settings) = settings.user_settings_update();
    if generation > runtime.observed_settings_generation {
        if runtime.locked {
            runtime.observed_settings_generation = generation;
            return;
        }
        let Ok(mut window) = windows.single_mut() else {
            return;
        };
        let (preference, present_mode) = if user_settings.video.vsync {
            (PresentModePreference::Vsync, PresentMode::Fifo)
        } else {
            (PresentModePreference::NoVsync, PresentMode::Immediate)
        };
        runtime.policy.set_preference(preference);
        set_present_mode_if_changed(&mut window, present_mode);
        runtime.observed_settings_generation = generation;
        return;
    }

    if !runtime.locked
        && runtime.policy.preference() == PresentModePreference::Auto
        && runtime.policy.remedy() == PresentModeRemedy::UseImmediate
        && let Ok(mut window) = windows.single_mut()
    {
        set_present_mode_if_changed(&mut window, PresentMode::Immediate);
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
mod tests {
    use bevy::prelude::{App, DetectChanges, MinimalPlugins};
    use render::PresentModePreference;

    use super::*;
    use crate::acceptance::markers::requested_present_mode;

    #[test]
    fn attributable_runs_and_explicit_flags_lock_the_requested_policy() {
        let automatic = PresentModeRuntime::from_startup(false, false, false);
        assert!(!automatic.locked());
        assert_eq!(automatic.policy.preference(), PresentModePreference::Auto);

        for (runtime, expected_preference, initial_mode) in [
            (
                PresentModeRuntime::from_startup(true, false, false),
                PresentModePreference::Vsync,
                requested_present_mode(false),
            ),
            (
                PresentModeRuntime::from_startup(false, true, false),
                PresentModePreference::NoVsync,
                requested_present_mode(true),
            ),
            (
                PresentModeRuntime::from_startup(false, false, true),
                PresentModePreference::Vsync,
                requested_present_mode(false),
            ),
        ] {
            assert!(runtime.locked());
            assert_eq!(runtime.policy.preference(), expected_preference);
            assert_eq!(
                initial_mode,
                match expected_preference {
                    PresentModePreference::NoVsync => PresentMode::Immediate,
                    PresentModePreference::Auto | PresentModePreference::Vsync => {
                        PresentMode::Fifo
                    }
                }
            );
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
            app.world()
                .resource::<PresentModeRuntime>()
                .policy
                .preference(),
            PresentModePreference::NoVsync
        );
        let present_mode = {
            let world = app.world_mut();
            let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
            windows.single(world).unwrap().present_mode
        };
        assert_eq!(present_mode, PresentMode::Immediate);

        let mut settings = ui::UserSettings::default();
        settings.video.vsync = true;
        app.world_mut()
            .resource_mut::<RuntimeSettings>()
            .replace_user_settings(settings);
        app.update();
        assert_eq!(
            app.world()
                .resource::<PresentModeRuntime>()
                .policy
                .preference(),
            PresentModePreference::Vsync
        );
        let present_mode = {
            let world = app.world_mut();
            let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
            windows.single(world).unwrap().present_mode
        };
        assert_eq!(present_mode, PresentMode::Fifo);
    }

    #[test]
    fn locked_acceptance_policy_ignores_runtime_setting_replacements() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<RuntimeSettings>()
            .insert_resource(PresentModeRuntime::from_startup(false, false, true))
            .add_systems(bevy::prelude::Update, apply_runtime_vsync_setting);
        app.world_mut().spawn((Window::default(), PrimaryWindow));

        let mut settings = ui::UserSettings::default();
        settings.video.vsync = false;
        app.world_mut()
            .resource_mut::<RuntimeSettings>()
            .replace_user_settings(settings);
        app.update();

        assert_eq!(
            app.world()
                .resource::<PresentModeRuntime>()
                .policy
                .preference(),
            PresentModePreference::Vsync
        );
        let present_mode = {
            let world = app.world_mut();
            let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
            windows.single(world).unwrap().present_mode
        };
        assert_eq!(present_mode, PresentMode::Fifo);
    }

    #[test]
    fn a_setting_update_retries_until_the_primary_window_exists() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<RuntimeSettings>()
            .insert_resource(PresentModeRuntime::from_startup(false, false, false))
            .add_systems(bevy::prelude::Update, apply_runtime_vsync_setting);

        let mut settings = ui::UserSettings::default();
        settings.video.vsync = false;
        app.world_mut()
            .resource_mut::<RuntimeSettings>()
            .replace_user_settings(settings);
        app.update();

        let runtime = app.world().resource::<PresentModeRuntime>();
        assert_eq!(runtime.observed_settings_generation(), 0);
        assert_eq!(runtime.policy.preference(), PresentModePreference::Auto);

        app.world_mut().spawn((Window::default(), PrimaryWindow));
        app.update();

        let runtime = app.world().resource::<PresentModeRuntime>();
        assert_eq!(runtime.observed_settings_generation(), 1);
        assert_eq!(runtime.policy.preference(), PresentModePreference::NoVsync);
        let present_mode = {
            let world = app.world_mut();
            let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
            windows.single(world).unwrap().present_mode
        };
        assert_eq!(present_mode, PresentMode::Immediate);
    }

    #[test]
    fn automatic_remedy_transitions_the_main_window_only_once() {
        let mut app = App::new();
        let runtime = PresentModeRuntime::from_startup(false, false, false);
        let render_policy = runtime.policy();
        app.add_plugins(MinimalPlugins)
            .init_resource::<RuntimeSettings>()
            .insert_resource(runtime)
            .add_systems(bevy::prelude::Update, apply_runtime_vsync_setting);
        let window_entity = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();

        render_policy.publish_remedy(PresentModeRemedy::UseImmediate);
        app.update();
        assert_eq!(
            app.world()
                .entity(window_entity)
                .get::<Window>()
                .unwrap()
                .present_mode,
            PresentMode::Immediate
        );

        app.world_mut().clear_trackers();
        app.update();
        let window = app
            .world()
            .entity(window_entity)
            .get_ref::<Window>()
            .unwrap();
        assert_eq!(window.present_mode, PresentMode::Immediate);
        assert!(
            !window.is_changed(),
            "a stable automatic remedy must not request another surface reconfigure"
        );
    }

    #[test]
    fn present_mode_transition_reports_whether_it_changed_the_window() {
        let mut window = Window::default();
        assert!(set_present_mode_if_changed(
            &mut window,
            PresentMode::Immediate
        ));
        assert!(!set_present_mode_if_changed(
            &mut window,
            PresentMode::Immediate
        ));
    }
}
