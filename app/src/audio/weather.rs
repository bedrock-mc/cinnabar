//! Borrows loaded precipitation columns for the presentation rain scheduler.

use bevy::prelude::{Local, Query, Res, ResMut, Transform, With};
use client_presentation::{
    audio::{
        AudioEngine,
        weather::{RainColumn, RainSoundScheduler},
    },
    camera::FlyCamera,
    local_player::LocalViewPose,
};

use crate::{
    environment::{WeatherTickFrame, WorldClock},
    menu::MenuRuntime,
    runtime::world::ClientWorld,
};

/// Samples rain on the same completed weather ticks that update the atmosphere.
pub(super) fn drive_rain_audio(
    ticks: Res<WeatherTickFrame>,
    clock: Res<WorldClock>,
    world: Res<ClientWorld>,
    menu: Option<Res<MenuRuntime>>,
    view: Res<LocalViewPose>,
    camera: Query<&Transform, With<FlyCamera>>,
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
    let listener = camera
        .single()
        .map_or_else(|_| view.eye_translation(), |camera| camera.translation)
        .to_array();
    let rules = &world.runtime_assets.biome_assets().rules;
    let fancy = menu
        .as_ref()
        .is_none_or(|menu| menu.settings_snapshot().0.value("graphics_mode") != 0);
    for level in ticks.current_rain_levels() {
        state.1.tick(level, listener, fancy, &mut engine, |x, z| {
            let surface_y = stream.top_non_air_block_y(x, z)?.saturating_add(1);
            let biome = stream.camera_biome_id([x as f32, surface_y as f32, z as f32])?;
            let rule = &rules[rules.binary_search_by_key(&biome, |rule| rule.id).ok()?];
            Some(RainColumn {
                surface_y,
                temperature: rule.temperature(),
                downfall: rule.downfall(),
            })
        });
    }
}
