//! Biome precipitation classification shared by weather rendering and sound.

const SNOW_TEMPERATURE: f32 = 0.15;
const TEMPERATURE_LOSS_PER_BLOCK_ABOVE_SEA: f32 = 0.05 / 30.0;
const SEA_LEVEL: f32 = 64.0;

/// What falls in one column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Precipitation {
    None,
    Rain,
    Snow,
}

/// Temperature after the altitude lapse that turns rain into snow on high ground.
#[must_use]
pub fn altitude_adjusted_temperature(temperature: f32, surface_y: i32) -> f32 {
    let above = (surface_y as f32 - SEA_LEVEL).max(0.0);
    temperature - above * TEMPERATURE_LOSS_PER_BLOCK_ABOVE_SEA
}

/// Whether a biome's downfall permits precipitation at all; sub-epsilon downfall stays dry.
#[must_use]
pub fn precipitation_eligible(downfall: f32) -> bool {
    downfall.is_finite() && downfall >= f32::EPSILON
}

/// Rain, snow or nothing for a biome: ineligible downfall never precipitates, cold falls as snow.
#[must_use]
pub fn classify_precipitation(temperature: f32, downfall: f32, surface_y: i32) -> Precipitation {
    if !temperature.is_finite() || !precipitation_eligible(downfall) {
        return Precipitation::None;
    }
    if altitude_adjusted_temperature(temperature, surface_y) <= SNOW_TEMPERATURE {
        Precipitation::Snow
    } else {
        Precipitation::Rain
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dry_biomes_never_precipitate_and_cold_ones_snow() {
        assert_eq!(classify_precipitation(2.0, 0.0, 64), Precipitation::None);
        assert_eq!(classify_precipitation(0.8, 0.4, 64), Precipitation::Rain);
        assert_eq!(classify_precipitation(0.0, 0.5, 64), Precipitation::Snow);
        assert_eq!(classify_precipitation(0.15, 0.5, 64), Precipitation::Snow);
        assert_eq!(
            classify_precipitation(f32::NAN, 0.5, 64),
            Precipitation::None
        );
    }

    #[test]
    fn precipitation_requires_finite_downfall_at_or_above_epsilon() {
        for (downfall, eligible) in [
            (0.0, false),
            (f32::EPSILON * 0.5, false),
            (f32::EPSILON, true),
            (f32::NAN, false),
            (f32::INFINITY, false),
        ] {
            assert_eq!(precipitation_eligible(downfall), eligible);
            assert_eq!(
                classify_precipitation(0.5, downfall, 64),
                if eligible {
                    Precipitation::Rain
                } else {
                    Precipitation::None
                },
            );
        }
    }

    #[test]
    fn high_ground_snows_in_otherwise_rainy_biomes() {
        assert_eq!(classify_precipitation(0.3, 0.5, 64), Precipitation::Rain);
        assert_eq!(classify_precipitation(0.3, 0.5, 200), Precipitation::Snow);
        assert_eq!(altitude_adjusted_temperature(0.5, 10), 0.5);
    }
}
