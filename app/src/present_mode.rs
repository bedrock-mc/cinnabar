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
    vsync: bool, // the user choice last applied to the window
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
            vsync: !no_vsync,
        }
    }

    #[must_use]
    pub(crate) fn policy(&self) -> Dx12PresentModePolicy {
        self.policy.clone()
    }

    /// Returns the session VSync state a launch flag or evidence run pins, if any.
    #[must_use]
    pub(crate) fn vsync_override(&self) -> Option<bool> {
        self.locked
            .then(|| self.policy.preference() != PresentModePreference::NoVsync)
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
        let vsync = user_settings.video.vsync;
        if !runtime.locked && vsync != runtime.vsync {
            let Ok(mut window) = windows.single_mut() else {
                return;
            };
            // VSync on stays automatic so the driver remedy can still apply.
            let (preference, present_mode) = if vsync {
                (PresentModePreference::Auto, PresentMode::Fifo)
            } else {
                (PresentModePreference::NoVsync, PresentMode::AutoNoVsync)
            };
            runtime.policy.set_preference(preference);
            set_present_mode_if_changed(&mut window, present_mode);
            runtime.vsync = vsync;
        }
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

    fn vsync_app(runtime: PresentModeRuntime, window: bool) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<RuntimeSettings>()
            .insert_resource(runtime)
            .add_systems(bevy::prelude::Update, apply_runtime_vsync_setting);
        if window {
            spawn_primary_window(&mut app);
        }
        app
    }

    fn spawn_primary_window(app: &mut App) {
        let window = Window {
            present_mode: PresentMode::Fifo,
            ..Window::default()
        };
        app.world_mut().spawn((window, PrimaryWindow));
    }

    fn publish_vsync(app: &mut App, vsync: bool) {
        let mut settings = ui::UserSettings::default();
        settings.video.vsync = vsync;
        app.world_mut()
            .resource_mut::<RuntimeSettings>()
            .replace_user_settings(settings);
        app.update();
    }

    fn preference(app: &App) -> PresentModePreference {
        app.world()
            .resource::<PresentModeRuntime>()
            .policy
            .preference()
    }

    fn primary_present_mode(app: &mut App) -> PresentMode {
        let world = app.world_mut();
        let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
        windows.single(world).unwrap().present_mode
    }

    #[test]
    fn toggling_the_user_setting_switches_the_present_mode_live() {
        let mut app = vsync_app(PresentModeRuntime::from_startup(false, false, false), true);

        publish_vsync(&mut app, false);
        assert_eq!(preference(&app), PresentModePreference::NoVsync);
        assert_eq!(primary_present_mode(&mut app), PresentMode::AutoNoVsync);

        publish_vsync(&mut app, true);
        assert_eq!(preference(&app), PresentModePreference::Auto);
        assert_eq!(primary_present_mode(&mut app), PresentMode::Fifo);
    }

    /// An unrelated settings revision must not drop an applied driver remedy.
    #[test]
    fn vsync_on_keeps_the_automatic_driver_remedy() {
        let runtime = PresentModeRuntime::from_startup(false, false, false);
        let render_policy = runtime.policy();
        let mut app = vsync_app(runtime, true);
        render_policy.publish_remedy(PresentModeRemedy::UseImmediate);
        app.update();

        publish_vsync(&mut app, true);
        assert_eq!(preference(&app), PresentModePreference::Auto);
        assert_eq!(render_policy.remedy(), PresentModeRemedy::UseImmediate);
        assert_eq!(primary_present_mode(&mut app), PresentMode::Immediate);
    }

    #[test]
    fn launch_flags_override_the_user_setting() {
        for (force_vsync, no_vsync, setting, effective) in
            [(true, false, false, true), (false, true, true, false)]
        {
            let runtime = PresentModeRuntime::from_startup(force_vsync, no_vsync, false);
            assert_eq!(runtime.vsync_override(), Some(effective));
            let startup_mode = requested_present_mode(no_vsync);
            let startup_preference = runtime.policy.preference();
            let mut app = vsync_app(runtime, false);
            app.world_mut().spawn((
                Window {
                    present_mode: startup_mode,
                    ..Window::default()
                },
                PrimaryWindow,
            ));

            publish_vsync(&mut app, setting);
            assert_eq!(preference(&app), startup_preference);
            assert_eq!(primary_present_mode(&mut app), startup_mode);
        }
        assert_eq!(
            PresentModeRuntime::from_startup(false, false, false).vsync_override(),
            None
        );
    }

    #[test]
    fn locked_acceptance_policy_ignores_runtime_setting_replacements() {
        let mut app = vsync_app(PresentModeRuntime::from_startup(false, false, true), true);
        publish_vsync(&mut app, false);
        assert_eq!(preference(&app), PresentModePreference::Vsync);
        assert_eq!(primary_present_mode(&mut app), PresentMode::Fifo);
    }

    #[test]
    fn a_setting_update_retries_until_the_primary_window_exists() {
        let mut app = vsync_app(PresentModeRuntime::from_startup(false, false, false), false);
        publish_vsync(&mut app, false);

        let runtime = app.world().resource::<PresentModeRuntime>();
        assert_eq!(runtime.observed_settings_generation(), 0);
        assert_eq!(runtime.policy.preference(), PresentModePreference::Auto);

        spawn_primary_window(&mut app);
        app.update();

        let runtime = app.world().resource::<PresentModeRuntime>();
        assert_eq!(runtime.observed_settings_generation(), 1);
        assert_eq!(runtime.policy.preference(), PresentModePreference::NoVsync);
        assert_eq!(primary_present_mode(&mut app), PresentMode::AutoNoVsync);
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
