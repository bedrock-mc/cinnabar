//! Shared native weather tick samples feed both atmosphere and seasonal foliage.

use bevy::prelude::{Res, ResMut};
use client_presentation::environment::weather::WeatherTickFrame;
use render_scene::ChunkBiomeTints;

use crate::runtime::world::ClientWorld;

pub(crate) fn update_seasonal_foliage(
    ticks: Res<WeatherTickFrame>,
    mut client_world: ResMut<ClientWorld>,
    mut active: ResMut<ChunkBiomeTints>,
) {
    let Some(stream) = client_world.stream.as_mut() else {
        return;
    };
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
}

#[cfg(test)]
mod tests;
