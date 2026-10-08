//! Shared native weather tick samples feed both atmosphere and seasonal foliage.

use bevy::prelude::{Res, ResMut, Resource};
use render::ChunkBiomeTints;

use crate::runtime::world::ClientWorld;

/// Bounded per-frame weather tick samples, previous/current rain in vanilla order.
#[derive(Debug, Default, Resource)]
pub(crate) struct WeatherTickFrame {
    pub(super) rain: Vec<[f32; 2]>,
    pub(super) dimension: i32,
    pub(super) weather_cycle_enabled: bool,
}

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
