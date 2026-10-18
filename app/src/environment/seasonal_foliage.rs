//! Shared native weather tick samples feed both atmosphere and seasonal foliage.

use bevy::prelude::{Local, Res, ResMut, Resource};
use render_scene::ChunkBiomeTints;

use crate::runtime::world::ClientWorld;

/// Bounded per-frame weather tick samples, previous/current rain in vanilla order.
#[derive(Debug, Default, Resource)]
pub(crate) struct WeatherTickFrame {
    pub(super) rain: Vec<[f32; 2]>,
    pub(super) dimension: i32,
    pub(super) weather_cycle_enabled: bool,
}

impl WeatherTickFrame {
    /// Visits current rain levels once per completed weather tick, in simulation order.
    pub(crate) fn current_rain_levels(&self) -> impl Iterator<Item = f32> + '_ {
        self.rain.iter().map(|rain| rain[1])
    }
}

pub(crate) fn update_seasonal_foliage(
    ticks: Res<WeatherTickFrame>,
    mut client_world: ResMut<ClientWorld>,
    mut active: ResMut<ChunkBiomeTints>,
    java: Option<Res<render_scene::JavaWorldVisuals>>,
    mut java_tints: Local<super::java_world_tints::JavaTintState>,
) {
    let Some(stream) = client_world.stream.as_mut() else {
        return;
    };
    let java_enabled = java.as_ref().is_some_and(|java| java.enabled());
    if stream.java_world_lighting() != java_enabled {
        stream.set_java_world_lighting(java_enabled, web_time::Instant::now());
    }
    if ticks.dimension != stream.current_dimension() {
        return;
    }
    let mut changed = false;
    for &rain in &ticks.rain {
        changed |= stream.advance_seasonal_foliage(rain, ticks.weather_cycle_enabled);
    }
    if changed {
        let resolved = stream.resolved_biome_tints_snapshot();
        *active =
            ChunkBiomeTints::from_resolved_with_identity(&resolved, stream.biome_tint_identity());
    }
    if let Some(java) = java {
        java_tints.update(
            &java,
            stream.authority().resolved_biome_tints(),
            &mut active,
        );
    }
}

#[cfg(test)]
mod tests;
