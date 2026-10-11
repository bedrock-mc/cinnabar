//! Borrows loaded precipitation columns for the presentation rain scheduler.

use bevy::prelude::{Local, Query, Res, ResMut, Transform, With};
use client_presentation::{
    audio::{
        AudioEngine,
        weather::{RainColumn, RainSoundScheduler},
    },
    camera::{FlyCamera, ServerCameraView},
    local_player::LocalViewPose,
};

use crate::{
    environment::{WeatherTickFrame, WorldClock},
    menu::MenuRuntime,
    runtime::world::ClientWorld,
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
    profiler: Option<Res<render::RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::Audio));
    let generation = Some(clock.session_generation());
    if state.0 != generation {
        *state = (generation, RainSoundScheduler::default());
    }
    let Some(stream) = world
        .stream
        .as_ref()
        .filter(|stream| stream.current_dimension() == 0)
    else {
        *state = (None, RainSoundScheduler::default());
        return;
    };
    if !engine.has_bank() {
        return;
    }
    let listener = rain_listener(&view, camera.single().ok(), server_camera.as_deref());
    let rules = &world.runtime_assets.biome_assets().rules;
    let fancy = menu
        .as_ref()
        .is_none_or(|menu| menu.settings_snapshot().0.value("graphics_mode") != 0);
    for level in ticks.current_rain_levels() {
        state.1.tick(level, listener, fancy, &mut engine, |x, z| {
            sample_rain_column(x, z, stream.top_non_air_block_y(x, z)?, |position| {
                let biome = stream.camera_biome_id(position)?;
                let rule = &rules[rules.binary_search_by_key(&biome, |rule| rule.id).ok()?];
                Some((rule.temperature(), rule.downfall()))
            })
        });
    }
}

/// Selects the world-space origin for rain sampling and shelter checks.
fn rain_listener(
    view: &LocalViewPose,
    camera: Option<&Transform>,
    server_camera: Option<&ServerCameraView>,
) -> [f32; 3] {
    client_presentation::audio::listener::camera_listener(
        view,
        camera,
        server_camera.and_then(|camera| camera.active_listener()),
    )
    .position
}

/// Resolves climate for a loaded surface and retains its precipitation height.
fn sample_rain_column(
    x: i32,
    z: i32,
    top: i32,
    climate: impl FnOnce([f32; 3]) -> Option<(f32, f32)>,
) -> Option<RainColumn> {
    let surface_y = top.saturating_add(1);
    let (temperature, downfall) = climate([x as f32 + 0.5, top as f32 + 0.5, z as f32 + 0.5])?;
    Some(RainColumn {
        surface_y,
        temperature,
        downfall,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_presentation::audio::listener::PLAYER_LISTENER;
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

    #[test]
    fn highest_loaded_surface_keeps_its_rain_climate() {
        let highest = 31;
        let sample = sample_rain_column(0, 0, highest, |position| {
            (position[1].floor() as i32 <= highest).then_some((0.8, 0.4))
        })
        .expect("biome within the retained surface block");
        assert_eq!(sample.surface_y, highest + 1);
        assert_eq!(sample.temperature, 0.8);
        assert_eq!(sample.downfall, 0.4);
    }
}
