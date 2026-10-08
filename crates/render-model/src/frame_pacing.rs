//! Frame admission cadence and the frame rate each window state allows.

use std::num::NonZeroU32;

const NANOS_PER_SECOND_MILLI: u128 = 1_000_000_000_000;

/// Cadence for a visible window without focus: still animated, never competing with the
/// foreground application for the CPU or GPU.
pub const UNFOCUSED_FRAME_RATE: FrameRate = FrameRate::from_hz_const(30);
/// Cadence for an occluded window: one update per 20 Hz authoritative tick keeps input,
/// network and simulation serviced while presenting as little as possible.
pub const OCCLUDED_FRAME_RATE: FrameRate = FrameRate::from_hz_const(20);

/// A frame rate in millihertz, the unit displays report refresh rates in.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct FrameRate(NonZeroU32);

impl FrameRate {
    /// `None` for zero; rates above about 4.29 MHz saturate.
    #[must_use]
    pub const fn from_hz(hz: u32) -> Option<Self> {
        Self::from_millihertz(hz.saturating_mul(1_000))
    }

    #[must_use]
    pub const fn from_millihertz(millihertz: u32) -> Option<Self> {
        match NonZeroU32::new(millihertz) {
            Some(rate) => Some(Self(rate)),
            None => None,
        }
    }

    pub(crate) const fn from_hz_const(hz: u32) -> Self {
        match Self::from_hz(hz) {
            Some(rate) => rate,
            None => panic!("frame rate must be positive"),
        }
    }

    #[must_use]
    pub const fn millihertz(self) -> u32 {
        self.0.get()
    }

    /// Period rounded down to whole nanoseconds; slot times use the exact ratio instead.
    #[must_use]
    pub const fn period_nanos(self) -> u64 {
        (NANOS_PER_SECOND_MILLI / self.0.get() as u128) as u64
    }
}

/// What the primary window currently allows presentation to spend.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WindowActivity {
    Focused,
    Unfocused,
    Occluded,
}

/// The admission rate for a window state: the requested rate, lowered when in the background.
#[must_use]
pub fn effective_frame_rate(
    requested: Option<FrameRate>,
    activity: WindowActivity,
) -> Option<FrameRate> {
    let background = match activity {
        WindowActivity::Focused => return requested,
        WindowActivity::Unfocused => UNFOCUSED_FRAME_RATE,
        WindowActivity::Occluded => OCCLUDED_FRAME_RATE,
    };
    Some(requested.map_or(background, |rate| rate.min(background)))
}

/// Admission slots at `epoch + k * period`, each computed from `k` exactly so error never
/// accumulates, with at most one admitted frame per slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cadence {
    rate: FrameRate,
    epoch_nanos: u64,
    next_slot: u64,
}

impl Cadence {
    /// Starts with slot 0 at `epoch_nanos`.
    #[must_use]
    pub const fn new(rate: FrameRate, epoch_nanos: u64) -> Self {
        Self {
            rate,
            epoch_nanos,
            next_slot: 0,
        }
    }

    #[must_use]
    pub const fn rate(&self) -> FrameRate {
        self.rate
    }

    /// When slot `slot` opens, saturating far beyond any process lifetime.
    #[must_use]
    pub fn slot_nanos(&self, slot: u64) -> u64 {
        let offset = u128::from(slot) * NANOS_PER_SECOND_MILLI / u128::from(self.rate.millihertz());
        u64::try_from(offset)
            .unwrap_or(u64::MAX)
            .saturating_add(self.epoch_nanos)
    }

    /// The earliest time the next frame may sample input.
    #[must_use]
    pub fn next_admission_nanos(&self) -> u64 {
        self.slot_nanos(self.next_slot)
    }

    /// Records a frame that sampled input at `sampled_nanos`. A wake up to half a period late
    /// keeps the epoch's phase, so the rate never drifts; a later frame restarts the cadence
    /// from itself, so the next frame follows a full period later rather than in a burst or
    /// after a skipped slot.
    pub fn admit(&mut self, sampled_nanos: u64) {
        let opened = self.slot_nanos(self.next_slot);
        if sampled_nanos > opened.saturating_add(self.rate.period_nanos() / 2) {
            self.epoch_nanos = sampled_nanos;
            self.next_slot = 1;
        } else {
            self.next_slot = self.next_slot.saturating_add(1);
        }
    }
}

#[cfg(test)]
#[path = "frame_pacing/tests.rs"]
mod tests;
