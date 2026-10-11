//! Borrows loaded precipitation columns for the presentation rain scheduler.

use bevy::prelude::{Local, Query, Res, ResMut, Transform, With};
use client_audio::audio::{AudioEngine, weather::RainSoundScheduler};
use client_presentation::{
    camera::{FlyCamera, ServerCameraView},
    local_player::LocalViewPose,
};

use {
    crate::{
        environment::{WeatherTickFrame, WorldClock},
        runtime::world::ClientWorld,
    },
    client_shell::menu::MenuRuntime,
};

/// Samples rain on the same completed weather ticks that update the atmosphere.
#[allow(clippy::too_many_arguments)]
pub(super) fn drive_rain_audio(
    ticks: Res<WeatherTickFrame>,
    clock: Res<WorldClock>,
    world: Res<ClientWorld>,
    menu: Option<Res<MenuRuntime>>,
    view: Res<LocalViewPose>,
    camera: Query<&Transform, With<FlyCamera>>,
    server_camera: Option<Res<ServerCameraView>>,
    mut engine: ResMut<AudioEngine>,
    mut state: Local<(Option<u64>, RainSoundScheduler)>,
    profiler: Option<Res<render_scene::RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render_scene::RuntimeStage::Audio));
    client_audio::audio::weather::drive_rain_audio(
        clock.session_generation(),
        world.stream.as_ref(),
        &world.runtime_assets,
        ticks.current_rain_levels(),
        rain_listener(&view, camera.single().ok(), server_camera.as_deref()),
        menu.as_ref()
            .is_none_or(|menu| menu.settings_snapshot().0.value("graphics_mode") != 0),
        &mut engine,
        &mut state,
    );
}

/// Selects the world-space origin for rain sampling and shelter checks.
fn rain_listener(
    view: &LocalViewPose,
    camera: Option<&Transform>,
    server_camera: Option<&ServerCameraView>,
) -> [f32; 3] {
    client_audio::audio::listener::camera_listener(
        view.eye_translation(),
        view.rotation(),
        camera,
        server_camera.and_then(|camera| camera.active_listener()),
    )
    .position
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_audio::audio::listener::PLAYER_LISTENER;
    use client_presentation::camera::ViewContext;
    use protocol::{CameraEvent, CameraInstructionEvent, CameraPreset, CameraSetInstruction};

    #[test]
    fn rain_origin_follows_the_selected_player_audio_listener() {
        let view = LocalViewPose::default();
        let remote = Transform::from_xyz(500.0, 80.0, 500.0);
        let context = ViewContext {
            base: Transform::IDENTITY,
            subject: Transform::IDENTITY,
            base_fov: 90.0,
            actors: &|_| None,
        };
        let mut camera = ServerCameraView::default();
        camera.apply(
            1,
            &CameraEvent::Presets(
                vec![CameraPreset {
                    name: "rain-fixture".into(),
                    inherit_from: "minecraft:free".into(),
                    listener: Some(PLAYER_LISTENER),
                    ..Default::default()
                }]
                .into(),
            ),
            &context,
        );
        camera.apply(
            2,
            &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
                set: Some(CameraSetInstruction {
                    preset_id: 0,
                    ease: None,
                    position: None,
                    rotation_degrees: None,
                    facing_position: None,
                    view_offset: None,
                    entity_offset: None,
                    default_preset: None,
                    remove_ignore_starting_values: false,
                }),
                ..Default::default()
            })),
            &context,
        );
        assert_eq!(camera.active_listener(), Some(PLAYER_LISTENER));
        assert_eq!(
            rain_listener(&view, Some(&remote), Some(&camera)),
            view.eye_translation().to_array()
        );
        assert_eq!(
            rain_listener(&view, Some(&remote), None),
            remote.translation.to_array()
        );
    }
}
