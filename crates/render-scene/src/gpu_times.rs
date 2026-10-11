//! Timing observations published by the renderer.
use crate::RuntimeStage;
use std::time::Duration;
/// Converts a tick delta with the queue's nanoseconds-per-tick period.
#[must_use]
pub fn ticks_to_duration(ticks: u64, period_ns: f32) -> Duration {
    let nanos = (ticks as f64 * f64::from(period_ns)).round();
    Duration::from_nanos(if nanos >= u64::MAX as f64 {
        u64::MAX
    } else {
        nanos as u64
    })
}

/// Sums elapsed pass latencies, including overlapping work and gaps; unmeasured stages stay absent.
/// Metal samples vertex-start to fragment-end and omits whole-frame, stock-pass and per-draw costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuFrameTimes {
    stages: [Option<Duration>; RuntimeStage::GPU.len()],
}

impl Default for GpuFrameTimes {
    fn default() -> Self {
        Self {
            stages: [None; RuntimeStage::GPU.len()],
        }
    }
}

impl GpuFrameTimes {
    /// `None` when the stage is not GPU-timed or did not run in this frame.
    #[must_use]
    pub fn get(&self, stage: RuntimeStage) -> Option<Duration> {
        stage.gpu_index().and_then(|index| self.stages[index])
    }

    /// Measured stages in [`RuntimeStage::GPU`] order.
    pub fn iter(&self) -> impl Iterator<Item = (RuntimeStage, Duration)> + '_ {
        RuntimeStage::GPU
            .into_iter()
            .zip(self.stages)
            .filter_map(|(stage, duration)| duration.map(|duration| (stage, duration)))
    }

    fn add(&mut self, stage: RuntimeStage, duration: Duration) {
        if let Some(index) = stage.gpu_index() {
            let total = self.stages[index].get_or_insert(Duration::ZERO);
            *total = total.saturating_add(duration);
        }
    }
}

/// Classifies invalid samples before subtraction; Metal reports failed counters as `u64::MAX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanValidity {
    Valid,
    ZeroBegin,
    ZeroEnd,
    Reversed,
    Sentinel,
}

/// Checks both samples, with sentinel and missing-value errors taking priority over ordering.
pub fn span_validity(begin: u64, end: u64) -> SpanValidity {
    if begin == u64::MAX || end == u64::MAX {
        SpanValidity::Sentinel
    } else if begin == 0 {
        SpanValidity::ZeroBegin
    } else if end == 0 {
        SpanValidity::ZeroEnd
    } else if end < begin {
        SpanValidity::Reversed
    } else {
        SpanValidity::Valid
    }
}

/// Sums valid tick spans; frame bounds are emitted only when coverage spans the frame and no
/// span already measured the whole frame.
#[must_use]
pub fn decode_spans(
    spans: impl IntoIterator<Item = (RuntimeStage, u64, u64)>,
    period_ns: f32,
    complete_frame: bool,
) -> GpuFrameTimes {
    let mut times = GpuFrameTimes::default();
    let mut bounds: Option<(u64, u64)> = None;
    let mut complete_frame = complete_frame;
    for (stage, begin, end) in spans {
        if span_validity(begin, end) != SpanValidity::Valid {
            continue;
        }
        complete_frame &= stage != RuntimeStage::GpuFrame;
        times.add(stage, ticks_to_duration(end - begin, period_ns));
        bounds = Some(bounds.map_or((begin, end), |(first, last)| {
            (first.min(begin), last.max(end))
        }));
    }
    if complete_frame && let Some((first, last)) = bounds {
        times.add(
            RuntimeStage::GpuFrame,
            ticks_to_duration(last - first, period_ns),
        );
    }
    times
}
