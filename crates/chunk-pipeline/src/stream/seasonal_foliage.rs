//! Vanilla's per-tick seasonal foliage palette rows.

use super::*;
mod admission;
#[cfg(test)]
mod lifecycle_tests;

/// Vanilla refreshes the seasons palette at tick 0 and every hundred ticks.
const PALETTE_REFRESH_TICKS: u64 = 100;
const RAIN_THRESHOLD: f32 = 0.2;
const COLD_ACCUMULATION: f32 = 0.04;
const WARM_MELT: f32 = 0.08;
// Vanilla uses one ULP below the literal -0.002.
const DRY_MELT: f32 = f32::from_bits(0xbb03_126e);

fn advance_row(snow: f32, temperature: f32, downfall: f32, rain: [f32; 2]) -> f32 {
    let delta = if rain[1] > RAIN_THRESHOLD {
        let rate = if temperature > assets::SEASONAL_FOLIAGE_COLD_THRESHOLD {
            (assets::SEASONAL_FOLIAGE_COLD_THRESHOLD - temperature) * WARM_MELT
        } else {
            COLD_ACCUMULATION
        };
        rate * rain[0] * downfall / world::TICKS_PER_SECOND as f32
    } else {
        DRY_MELT
    };
    (snow + delta).clamp(0.0, 1.0)
}

/// Copied native registry rows, independent of the join-time Biome definitions.
#[derive(Default)]
pub(super) struct SeasonalFoliage {
    snow: Vec<f32>,
    tick: u64,
    dirty: bool,
}

impl SeasonalFoliage {
    pub(super) fn reset(&mut self, definitions: &[BiomeDefinitionEvent]) {
        self.snow = definitions.iter().map(|row| row.snow_foliage).collect();
        self.dirty = false;
    }

    fn tick(
        &mut self,
        definitions: &[BiomeDefinitionEvent],
        rain: [f32; 2],
        enabled: bool,
    ) -> bool {
        if enabled {
            for (snow, row) in self.snow.iter_mut().zip(definitions) {
                if !snow.is_finite() || !row.temperature.is_finite() || !row.downfall.is_finite() {
                    continue;
                }
                let next = advance_row(*snow, row.temperature, row.downfall, rain);
                self.dirty |= next != *snow;
                *snow = next;
            }
        }
        let refresh = self.tick.is_multiple_of(PALETTE_REFRESH_TICKS);
        self.tick = self.tick.wrapping_add(1);
        refresh && self.dirty
    }
}

impl WorldStream {
    /// Advances native seasonal rows once per world tick. Returns true only when
    /// the hundred-tick renderer refresh changes palette colours. Dense IDs and
    /// mesh tint identity remain unchanged; geometry never depends on this value.
    ///
    /// Only the built-in Overworld weather route is currently admitted. Custom
    /// dimension weather and positional climate remain explicitly incomplete.
    pub fn advance_seasonal_foliage(
        &mut self,
        rain: [f32; 2],
        weather_cycle_enabled: bool,
    ) -> bool {
        if !rain.iter().all(|value| value.is_finite()) {
            return false;
        }
        let rain = rain.map(|value| value.clamp(0.0, 1.0));
        let definitions = self.biome_definitions_snapshot();
        if !self.seasonal_foliage.tick(
            &definitions,
            rain,
            weather_cycle_enabled && self.authority.current_dimension() == 0,
        ) {
            return false;
        }
        let live: Vec<_> = definitions
            .iter()
            .zip(&self.seasonal_foliage.snow)
            .map(|(row, &snow_foliage)| LiveBiomeDefinition {
                name: &row.name,
                biome_id: row.biome_id,
                temperature: row.temperature,
                downfall: row.downfall,
                snow_foliage,
                max_snow_accumulation: row.max_snow_accumulation,
                map_water_argb: row.map_water_color,
            })
            .collect();
        let Ok(resolved) = self
            .authority
            .runtime_assets()
            .biome_assets()
            .resolve_live(&live)
        else {
            self.record_normalization_error(
                NormalizationErrorReason::BiomeDefinitionResolutionFailure,
            );
            return false;
        };
        // Palette-only updates must never change the geometry's dense-ID contract.
        if resolved.raw_id_to_dense != self.authority.resolved_biome_tints().raw_id_to_dense {
            self.record_normalization_error(
                NormalizationErrorReason::BiomeDefinitionResolutionFailure,
            );
            return false;
        }
        self.seasonal_foliage.dirty = false;
        if resolved == **self.authority.resolved_biome_tints() {
            return false;
        }
        #[cfg(debug_assertions)]
        {
            let (count, minimum, maximum) = definitions
                .iter()
                .zip(&self.seasonal_foliage.snow)
                .filter(|(row, _)| row.temperature < assets::SEASONAL_FOLIAGE_COLD_THRESHOLD)
                .fold(
                    (0, 1.0_f32, 0.0_f32),
                    |(count, minimum, maximum), (_, &snow)| {
                        (count + 1, minimum.min(snow), maximum.max(snow))
                    },
                );
            eprintln!(
                "BIOME_SEASONS palette_tick={} previous_rain={} current_rain={} cold_rows={} snow_min={} snow_max={}",
                self.seasonal_foliage.tick - 1,
                rain[0],
                rain[1],
                count,
                minimum,
                maximum
            );
        }
        self.authority.replace_seasonal_biome_tints(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seasonal_row_accumulates_from_previous_rain_and_downfall() {
        let snow = advance_row(0.0, -0.5, 0.8, [0.75, 1.0]);
        assert!((snow - 0.0012).abs() < 1.0e-7, "{snow}");
    }

    #[test]
    fn seasonal_row_uses_current_rain_threshold_and_native_dry_melt() {
        assert_eq!(
            advance_row(0.5, -0.5, 0.8, [1.0, RAIN_THRESHOLD]),
            0.5 + DRY_MELT
        );
        assert_eq!(advance_row(0.5, -0.5, 0.8, [0.0, 1.0]), 0.5);
        assert_eq!(advance_row(0.0, -0.5, 0.8, [0.0, 0.0]), 0.0);
        assert_eq!(advance_row(1.0, -0.5, 0.8, [1.0, 1.0]), 1.0);
    }

    #[test]
    fn seasonal_row_warm_rain_melts_and_boundary_temperature_still_grows() {
        let warm = advance_row(0.5, 0.65, 0.8, [1.0, 1.0]);
        assert!((warm - 0.4984).abs() < 1.0e-7);
        assert!(
            advance_row(
                0.5,
                assets::SEASONAL_FOLIAGE_COLD_THRESHOLD,
                0.8,
                [1.0, 1.0]
            ) > 0.5
        );
    }
}
