//! Opt-in bounded counters distinguish missing query samples from skipped readbacks.

use super::readback::{SpanValidity, span_validity};
use crate::RuntimeStage;
use bevy::platform::time::Instant;
use std::time::Duration;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct StageHealth {
    attempts: u64,
    valid: u64,
    zero_begin: u64,
    zero_end: u64,
    reversed: u64,
    sentinel: u64,
    first_invalid: Option<(u64, u64)>,
}

impl StageHealth {
    /// Retains one raw invalid pair while counting every resolved span without allocation.
    fn sample(&mut self, begin: u64, end: u64) {
        self.attempts += 1;
        let validity = span_validity(begin, end);
        match validity {
            SpanValidity::Valid => self.valid += 1,
            SpanValidity::ZeroBegin => self.zero_begin += 1,
            SpanValidity::ZeroEnd => self.zero_end += 1,
            SpanValidity::Reversed => self.reversed += 1,
            SpanValidity::Sentinel => self.sentinel += 1,
        }
        if validity != SpanValidity::Valid && self.first_invalid.is_none() {
            self.first_invalid = Some((begin, end));
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct HealthInterval {
    frames: u64,
    ring_skips: u64,
    readbacks: u64,
    map_failures: u64,
    stages: [StageHealth; RuntimeStage::GPU.len()],
}

impl Default for HealthInterval {
    fn default() -> Self {
        Self {
            frames: 0,
            ring_skips: 0,
            readbacks: 0,
            map_failures: 0,
            stages: [StageHealth::default(); RuntimeStage::GPU.len()],
        }
    }
}

impl HealthInterval {
    /// Builds JSON only when reporting an interval; untouched stages are omitted.
    fn json(&self, elapsed: Duration) -> serde_json::Value {
        let stages: serde_json::Map<_, _> = RuntimeStage::GPU
            .into_iter()
            .zip(self.stages)
            .filter(|(_, health)| health.attempts > 0)
            .map(|(stage, health)| {
                (
                    stage.name().to_owned(),
                    serde_json::json!({
                        "attempts": health.attempts,
                        "valid": health.valid,
                        "zero_begin": health.zero_begin,
                        "zero_end": health.zero_end,
                        "reversed": health.reversed,
                        "sentinel": health.sentinel,
                        "first_invalid": health.first_invalid,
                    }),
                )
            })
            .collect();
        serde_json::json!({
            "interval_ms": elapsed.as_secs_f64() * 1000.0,
            "frames": self.frames,
            "ring_skips": self.ring_skips,
            "readbacks": self.readbacks,
            "map_failures": self.map_failures,
            "stages": stages,
        })
    }
}

pub(super) struct QueryHealth {
    started: Instant,
    interval: HealthInterval,
}

impl QueryHealth {
    /// The optional diagnostics resource reads only its named flag once at startup.
    pub(super) fn requested() -> Option<Self> {
        (std::env::var("RUST_MCBE_GPU_QUERY_HEALTH").as_deref() == Ok("1")).then(|| Self {
            started: Instant::now(),
            interval: HealthInterval::default(),
        })
    }

    /// Counts resolved pairs; attempts here exclude frames that could not claim a ring slot.
    pub(super) fn sample(&mut self, stage: RuntimeStage, begin: u64, end: u64) {
        if let Some(index) = stage.gpu_index() {
            self.interval.stages[index].sample(begin, end);
        }
    }

    /// Counts one completed frame buffer even if every span in it was invalid.
    pub(super) fn readback(&mut self) {
        self.interval.readbacks += 1;
    }

    /// Counts failed maps separately from unavailable or invalid timestamp pairs.
    pub(super) fn map_failure(&mut self) {
        self.interval.map_failures += 1;
    }

    /// Records a frame attempt and emits at most once a second without blocking the GPU.
    pub(super) fn frame(&mut self, skipped: bool) {
        self.interval.frames += 1;
        self.interval.ring_skips += u64::from(skipped);
        if let Some((elapsed, interval)) = self.take_due(Instant::now()) {
            eprintln!("RUST_MCBE_GPU_QUERY_HEALTH {}", interval.json(elapsed));
        }
    }

    /// An explicit clock value makes the interval reset deterministic to test.
    fn take_due(&mut self, now: Instant) -> Option<(Duration, HealthInterval)> {
        let elapsed = now.saturating_duration_since(self.started);
        if elapsed < Duration::from_secs(1) {
            return None;
        }
        self.started = now;
        Some((elapsed, std::mem::take(&mut self.interval)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_count_each_invalid_reason_and_keep_the_first_example() {
        let mut stage = StageHealth::default();
        for pair in [(8, 9), (8, 8), (0, 0), (8, 0), (8, 7), (u64::MAX, 9)] {
            stage.sample(pair.0, pair.1);
        }
        assert_eq!(
            stage,
            StageHealth {
                attempts: 6,
                valid: 2,
                zero_begin: 1,
                zero_end: 1,
                reversed: 1,
                sentinel: 1,
                first_invalid: Some((0, 0)),
            }
        );
    }

    #[test]
    fn due_intervals_reset_counters_and_examples_without_wall_clock_waits() {
        let started = Instant::now();
        let mut health = QueryHealth {
            started,
            interval: HealthInterval::default(),
        };
        health.sample(RuntimeStage::GpuOpaque, 100, 0);
        health.sample(RuntimeStage::GpuTerrainModel, 100, 200);
        health.readback();
        health.map_failure();
        health.interval.frames = 3;
        health.interval.ring_skips = 1;
        assert!(
            health
                .take_due(started + Duration::from_millis(999))
                .is_none()
        );
        let (elapsed, interval) = health.take_due(started + Duration::from_secs(1)).unwrap();
        let json = interval.json(elapsed);
        assert_eq!(json["frames"], 3);
        assert_eq!(json["ring_skips"], 1);
        assert_eq!(json["readbacks"], 1);
        assert_eq!(json["map_failures"], 1);
        assert_eq!(json["stages"].as_object().unwrap().len(), 2);
        assert_eq!(
            json["stages"]["gpu_opaque"]["first_invalid"],
            serde_json::json!([100, 0])
        );
        assert_eq!(json["stages"]["gpu_terrain_model"]["valid"], 1);
        assert_eq!(health.interval, HealthInterval::default());
        health.sample(RuntimeStage::GpuOpaque, 300, 100);
        let (_, interval) = health.take_due(started + Duration::from_secs(2)).unwrap();
        assert_eq!(
            interval.stages[RuntimeStage::GpuOpaque.gpu_index().unwrap()].first_invalid,
            Some((300, 100))
        );
    }
}
