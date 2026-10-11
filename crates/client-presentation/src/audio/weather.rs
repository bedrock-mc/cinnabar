//! Tick-driven rain sound admission and playback.

use render::{Precipitation, classify_precipitation};

use super::engine::{AudioEngine, SoundRequest};

/// Rain definition resolved through the active pack stack.
pub const RAIN_SOUND: &str = "ambient.weather.rain";
const SAMPLE_RANGE: i32 = 10;
const MAX_ATTEMPTS: f32 = 100.0;

/// A loaded precipitation surface and its biome climate.
#[derive(Clone, Copy, Debug)]
pub struct RainColumn {
    pub surface_y: i32,
    pub temperature: f32,
    pub downfall: f32,
}

/// Retains the rain sound cadence across weather ticks.
#[derive(Default)]
pub struct RainSoundScheduler {
    eligible_ticks: u32,
}

impl RainSoundScheduler {
    /// Observes one weather tick, requesting audio only for admitted rain.
    pub fn tick(
        &mut self,
        level: f32,
        listener: [f32; 3],
        fancy: bool,
        engine: &mut AudioEngine,
        mut column: impl FnMut(i32, i32) -> Option<RainColumn>,
    ) {
        if !level.is_finite() || listener.iter().any(|value| !value.is_finite()) {
            return;
        }
        let level = level.clamp(0.0, 1.0);
        let sample_level = if fancy { level } else { level * 0.5 };
        let attempts = (MAX_ATTEMPTS * sample_level * sample_level) as u32;
        if attempts == 0 {
            return;
        }
        let [cx, cy, cz] = listener.map(|value| value.floor() as i32);
        let mut admitted = 0;
        let mut position = [0.0; 3];
        for _ in 0..attempts {
            let x = cx.saturating_add(offset(engine));
            let z = cz.saturating_add(offset(engine));
            let Some(sample) = column(x, z) else { continue };
            if sample.surface_y < cy.saturating_sub(SAMPLE_RANGE)
                || sample.surface_y > cy.saturating_add(SAMPLE_RANGE)
                || classify_precipitation(sample.temperature, sample.downfall, sample.surface_y)
                    != Precipitation::Rain
            {
                continue;
            }
            admitted += 1;
            position = [
                x as f32 + engine.unit(),
                sample.surface_y as f32 + 0.2,
                z as f32 + engine.unit(),
            ];
        }
        if admitted == 0 {
            return;
        }
        let previous = self.eligible_ticks;
        self.eligible_ticks += 1;
        if (engine.unit() * 3.0) as u32 >= previous {
            return;
        }
        self.eligible_ticks = 0;
        let height_above =
            column(cx, cz).map_or(0.0, |sample| sample.surface_y as f32 - listener[1]);
        let sheltered = position[1] > listener[1] + 1.0 && height_above > 0.0;
        let (gain, pitch) = if sheltered {
            ((1.0 - 0.095 * height_above).max(0.05), 0.5)
        } else {
            (1.0, 1.0)
        };
        engine.enqueue(
            SoundRequest::new(RAIN_SOUND)
                .at(position)
                .scaled(gain * admitted as f32 / attempts as f32 * level, pitch),
        );
    }
}

/// Triangular column offset from two independent bounded draws.
fn offset(engine: &mut AudioEngine) -> i32 {
    (engine.unit() * SAMPLE_RANGE as f32) as i32 - (engine.unit() * SAMPLE_RANGE as f32) as i32
}
