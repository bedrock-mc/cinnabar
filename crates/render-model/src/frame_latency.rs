//! Clock-free queue policy for fixed-refresh synchronized presentation.

use std::{num::NonZeroU32, time::Duration};

use crate::FrameRate;

const HISTORY: usize = 32;
const HEADROOM_SAMPLES: u32 = 120;
const QUALIFICATION: Duration = Duration::from_secs(2);
const MAX_SAMPLE_AGE: Duration = Duration::from_millis(250);
const PROBE_WINDOW: Duration = Duration::from_secs(10);
const MAX_COOLDOWN: Duration = Duration::from_secs(120);
const CADENCE_WINDOW: u8 = 8;
const MISSES_TO_ESCALATE: u32 = 6;

/// A coarse render-thread/GPU estimate used only to decide when to try one queued frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameWorkSample {
    pub frame: u64,
    pub at: Duration,
    pub render: Duration,
    pub gpu: Duration,
}

/// Inputs to the pure depth decision; work estimates never escalate the queue.
#[derive(Clone, Copy, Debug, Default)]
struct Evidence {
    missed_cadence: bool,
    headroom: bool,
    cooldown_finished: bool,
}

/// Escalates on delivered cadence and probes one frame only after headroom and a cool-down.
fn select_latency(current: NonZeroU32, evidence: Evidence) -> NonZeroU32 {
    if current == NonZeroU32::MIN && evidence.missed_cadence {
        NonZeroU32::new(2).expect("two is nonzero")
    } else if evidence.headroom && evidence.cooldown_finished {
        NonZeroU32::MIN
    } else {
        current
    }
}

/// Deterministic policy. A bad estimate causes a brief cadence-checked probe, then backs off.
#[derive(Debug)]
pub struct FrameLatencyPolicy {
    latency: NonZeroU32,
    samples: [FrameWorkSample; HISTORY],
    len: usize,
    next: usize,
    last: Option<FrameWorkSample>,
    headroom_since: Option<Duration>,
    headroom_samples: u32,
    changed_at: Duration,
    cooldown: Duration,
    next_probe: Duration,
    cadence_misses: u8,
    cadence_samples: u8,
}

impl Default for FrameLatencyPolicy {
    fn default() -> Self {
        Self {
            latency: NonZeroU32::new(2).expect("two is nonzero"),
            samples: [FrameWorkSample::default(); HISTORY],
            len: 0,
            next: 0,
            last: None,
            headroom_since: None,
            headroom_samples: 0,
            changed_at: Duration::ZERO,
            cooldown: QUALIFICATION,
            next_probe: QUALIFICATION,
            cadence_misses: 0,
            cadence_samples: 0,
        }
    }
}

impl FrameLatencyPolicy {
    /// The current recommendation, before platform-specific surface reservations are applied.
    pub fn latency(&self) -> NonZeroU32 {
        self.latency
    }

    /// Advances on caller-supplied time. `refresh` is present only for an active fixed-refresh
    /// VSync surface without a lower frame cap. `cadence` is a new acquisition/present interval
    /// at the selected depth. Incomplete work samples are ignored, including while at depth one.
    pub fn update(
        &mut self,
        now: Duration,
        refresh: Option<FrameRate>,
        cadence: Option<Duration>,
        sample: Option<FrameWorkSample>,
    ) -> NonZeroU32 {
        let Some(refresh) = refresh else {
            *self = Self::default();
            self.changed_at = now;
            self.next_probe = now.saturating_add(self.cooldown);
            return self.latency;
        };
        let interval = Duration::from_nanos(refresh.period_nanos());
        if self.latency == NonZeroU32::MIN {
            if let Some(cadence) = cadence.filter(|interval| !interval.is_zero()) {
                self.cadence_misses =
                    (self.cadence_misses << 1) | u8::from(cadence > interval.mul_f64(1.25));
                self.cadence_samples = (self.cadence_samples + 1).min(CADENCE_WINDOW);
            }
        } else {
            self.record_headroom(now, interval, sample);
        }
        let selected = select_latency(
            self.latency,
            Evidence {
                missed_cadence: self.cadence_samples == CADENCE_WINDOW
                    && self.cadence_misses.count_ones() >= MISSES_TO_ESCALATE,
                headroom: self.headroom_samples >= HEADROOM_SAMPLES
                    && self
                        .headroom_since
                        .is_some_and(|since| now.saturating_sub(since) >= QUALIFICATION),
                cooldown_finished: now >= self.next_probe,
            },
        );
        if selected != self.latency {
            if selected.get() == 2 {
                self.cooldown = if now.saturating_sub(self.changed_at) <= PROBE_WINDOW {
                    self.cooldown.saturating_mul(2).min(MAX_COOLDOWN)
                } else {
                    QUALIFICATION
                };
                self.next_probe = now.saturating_add(self.cooldown);
            }
            self.latency = selected;
            self.changed_at = now;
            self.clear_headroom();
            self.cadence_misses = 0;
            self.cadence_samples = 0;
        }
        self.latency
    }

    /// Keeps fresh work estimates without requiring uninterrupted query coverage.
    fn record_headroom(
        &mut self,
        now: Duration,
        interval: Duration,
        sample: Option<FrameWorkSample>,
    ) {
        if self
            .last
            .is_some_and(|last| now.saturating_sub(last.at) > MAX_SAMPLE_AGE)
        {
            self.clear_headroom();
        }
        let Some(sample) = sample.filter(|sample| {
            sample.at >= self.changed_at
                && sample.at <= now
                && now - sample.at <= MAX_SAMPLE_AGE
                && self.last.is_none_or(|last| sample.frame > last.frame)
        }) else {
            return;
        };
        self.last = Some(sample);
        self.samples[self.next] = sample;
        self.next = (self.next + 1) % HISTORY;
        self.len = (self.len + 1).min(HISTORY);
        if self.budget() <= interval.mul_f64(0.80) {
            self.headroom_since.get_or_insert(sample.at);
            self.headroom_samples = self.headroom_samples.saturating_add(1);
        } else {
            self.headroom_since = None;
            self.headroom_samples = 0;
        }
    }

    /// Discards only the headroom estimate, preserving depth and failed-probe backoff.
    fn clear_headroom(&mut self) {
        self.len = 0;
        self.next = 0;
        self.last = None;
        self.headroom_since = None;
        self.headroom_samples = 0;
    }

    /// Adds independent 95th-percentile CPU and coarse GPU durations, allowing missing components
    /// to bias the estimate low; delivered cadence and probe backoff contain that error.
    fn budget(&self) -> Duration {
        let mut cpu = self.samples.map(|sample| sample.render);
        let mut gpu = self.samples.map(|sample| sample.gpu);
        cpu[..self.len].sort_unstable();
        gpu[..self.len].sort_unstable();
        let rank = (self.len * 95).div_ceil(100) - 1;
        cpu[rank].saturating_add(gpu[rank])
    }
}

#[cfg(test)]
mod tests;
