//! Camera input adapters over the shared semantic frame and movement resources.
use crate::movement::{LocalMovementSpeedAuthority, LocalPhysicsController};
use bevy::{prelude::*, window::PrimaryWindow};
use client_presentation::camera::CameraFovInputs;
use client_presentation::{
    camera::{
        AutoFly, CameraSettingsAuthority, FlyCamera, FlyCameraUpdateSet, ServerCameraView, fov,
        look,
    },
    local_player::LocalViewPose,
};
use client_state::{ClientFrameSet, ModCameraInputSet, RuntimeSettings, SemanticInputSnapshot};
use client_ui::ui_runtime::UiRuntime;

pub mod aim_assist;

/// Registers camera settings, semantic look and movement input in their existing frame phases.
#[derive(Default)]
pub struct CameraInputPlugin;

impl Plugin for CameraInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                (apply_runtime_camera_settings, collect_fov_inputs)
                    .chain()
                    .after(ClientFrameSet::SemanticFinalize)
                    .before(FlyCameraUpdateSet),
                (update_perspective, update_look, update_movement)
                    .chain()
                    .in_set(FlyCameraUpdateSet),
            ),
        )
        .configure_sets(
            Update,
            ModCameraInputSet
                .in_set(FlyCameraUpdateSet)
                .after(update_look)
                .before(update_movement),
        );
    }
}

/// Forwards each retained settings revision to the camera authority once.
pub fn apply_runtime_camera_settings(
    runtime: Res<RuntimeSettings>,
    mut camera: ResMut<CameraSettingsAuthority>,
) {
    let (generation, settings) = runtime.user_settings_update();
    if generation > camera.generation() {
        let _ = camera.replace(generation, settings);
    }
}
/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub fn update_perspective(
    input: Res<SemanticInputSnapshot>,
    settings: ResMut<CameraSettingsAuthority>,
) {
    client_presentation::camera::update_perspective(
        client_presentation::observations::InputObservation(input.snapshot()),
        settings,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub fn update_look(
    spyglass: (
        Option<Res<client_state::GameplayScreenControls>>,
        Option<Res<fov::CameraFovInputs>>,
    ),
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    input: Res<SemanticInputSnapshot>,
    auto_fly: Res<AutoFly>,
    settings: ResMut<CameraSettingsAuthority>,
    time: Res<Time>,
    smoother: ResMut<look::LookSmoother>,
    view: ResMut<LocalViewPose>,
    server: Option<ResMut<ServerCameraView>>,
) {
    client_presentation::camera::update_look(
        (
            spyglass.0.as_ref().map_or(0.0, |controls| {
                match input
                    .snapshot()
                    .map_or(semantic_input::InputMode::KeyboardMouse, |snapshot| {
                        snapshot.input_mode
                    }) {
                    semantic_input::InputMode::KeyboardMouse => controls.spyglass_mouse_damping,
                    semantic_input::InputMode::GamePad => controls.spyglass_gamepad_damping,
                    semantic_input::InputMode::Touch => 0.0,
                }
            }),
            spyglass.1,
        ),
        window.map_or(0, |window| window.physical_width()),
        client_presentation::observations::InputObservation(input.snapshot()),
        auto_fly,
        settings,
        time,
        smoother,
        view,
        server,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub fn update_movement(
    input: Res<SemanticInputSnapshot>,
    time: Res<Time>,
    auto_fly: ResMut<AutoFly>,
    local_physics: Option<Res<crate::movement::LocalPhysicsController>>,
    camera: Single<&FlyCamera>,
    view: ResMut<LocalViewPose>,
) {
    client_presentation::camera::update_movement(
        client_presentation::observations::InputObservation(input.snapshot()),
        time,
        auto_fly,
        local_physics
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::PhysicsObservation),
        camera,
        view,
    );
}
/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub fn collect_fov_inputs(
    input: Res<SemanticInputSnapshot>,
    settings: Res<CameraSettingsAuthority>,
    ui: Option<Res<UiRuntime>>,
    physics: Option<Res<LocalPhysicsController>>,
    movement_speed: Option<Res<LocalMovementSpeedAuthority>>,
    inputs: ResMut<CameraFovInputs>,
) {
    client_presentation::camera::presentation::collect_fov_inputs(
        client_presentation::observations::InputObservation(input.snapshot()),
        settings,
        ui.as_deref(),
        physics
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::PhysicsObservation),
        movement_speed.and_then(|speed| speed.current()),
        inputs,
    );
}

#[cfg(test)]
mod antialiasing_settings_tests {
    use bevy::anti_alias::smaa::Smaa;
    use client_presentation::camera::antialiasing;
    use {
        super::*,
        client_presentation::camera::{CameraSettingsAuthority, FlyCamera},
    };

    #[test]
    fn runtime_settings_enable_and_disable_world_smaa_alongside_msaa() {
        let mut app = App::new();
        app.init_resource::<RuntimeSettings>()
            .init_resource::<CameraSettingsAuthority>()
            .init_resource::<antialiasing::CameraAntiAliasingSupport>()
            .add_systems(
                Update,
                (
                    apply_runtime_camera_settings,
                    antialiasing::apply_camera_antialiasing,
                )
                    .chain(),
            );
        let camera = app
            .world_mut()
            .spawn((FlyCamera::default(), Msaa::Off))
            .id();
        let mut settings = ui::UserSettings::default();
        settings.video.smaa_mode = ui::SmaaMode::Smaa;
        app.world_mut()
            .resource_mut::<RuntimeSettings>()
            .replace_user_settings(settings.clone());
        app.update();
        assert!(app.world().get::<Smaa>(camera).is_some());
        assert_eq!(
            app.world().get::<Msaa>(camera).unwrap().samples(),
            settings.video.anti_aliasing_samples
        );
        settings.video.smaa_mode = ui::SmaaMode::Off;
        app.world_mut()
            .resource_mut::<RuntimeSettings>()
            .replace_user_settings(settings);
        app.update();
        assert!(app.world().get::<Smaa>(camera).is_none());
    }
}
