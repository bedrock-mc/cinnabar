//! Present-mode and frame-rate policy shared by window setup, pacing and telemetry, independent
//! of the GPU backend.
//!
//! | Intent | Mode, best first |
//! | --- | --- |
//! | Synchronized (VSync on) | FIFO |
//! | LowLatency (VSync off) | Immediate, Mailbox, FIFO |
//! | Unpaced (hidden developer surfaces only) | Immediate, Mailbox, FIFO |
//!
//! VSync-off presentation may tear: Immediate is the only mode that cannot be backpressured by
//! the display refresh. Mailbox remains the non-tearing fallback when Immediate is unavailable.

use render_api::{FrameRateLimit, VrrPreference};

use crate::frame_pacing::FrameRate;

/// Initial variable-refresh ceiling as a share of the maximum refresh, in percent.
const VRR_CEILING_PERCENT: u64 = 97;
/// Margin added to measured completion error when sizing the variable-refresh ceiling.
const VRR_ERROR_MARGIN_NANOS: u64 = 100_000;

/// What the player asked presentation to optimise for.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PresentationIntent {
    /// Display-paced, tear-free delivery.
    Synchronized,
    /// Fresh input and the shortest queue, allowing tearing to avoid display-rate backpressure.
    LowLatency,
    /// Diagnostic only: a hidden developer surface never reaches a display, so it never waits
    /// for one. No setting selects it.
    Unpaced,
}

/// Whether the display is known to refresh when a frame arrives rather than on a fixed clock.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum VrrStatus {
    Active,
    Inactive,
    /// Treated as fixed refresh: a capability or a high refresh rate does not prove activation.
    #[default]
    Unknown,
}

/// What presentation knows about the display it targets.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct DisplayTiming {
    /// Current refresh rate; under variable refresh, its maximum.
    pub refresh: Option<FrameRate>,
    pub vrr: VrrStatus,
}

impl DisplayTiming {
    /// Applies the player's VRR declaration while retaining the observed refresh ceiling.
    #[must_use]
    pub const fn with_vrr_preference(mut self, preference: VrrPreference) -> Self {
        self.vrr = match preference {
            VrrPreference::Automatic => self.vrr,
            VrrPreference::On => VrrStatus::Active,
            VrrPreference::Off => VrrStatus::Inactive,
        };
        self
    }
}

/// A surface presentation mode, named after the backend modes it maps to one-to-one.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(u8)]
pub enum PresentModeKind {
    Fifo = 0,
    FifoRelaxed = 1,
    Mailbox = 2,
    Immediate = 3,
}

impl PresentModeKind {
    pub const ALL: [Self; 4] = [
        Self::Fifo,
        Self::FifoRelaxed,
        Self::Mailbox,
        Self::Immediate,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Fifo => "Fifo",
            Self::FifoRelaxed => "FifoRelaxed",
            Self::Mailbox => "Mailbox",
            Self::Immediate => "Immediate",
        }
    }

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// The present modes a surface advertises; FIFO is always implied, as every backend supports it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct SurfacePresentModes(u8);

impl SurfacePresentModes {
    /// Only the universally supported FIFO mode.
    pub const FIFO_ONLY: Self = Self(PresentModeKind::Fifo.bit());

    #[must_use]
    pub const fn with(self, mode: PresentModeKind) -> Self {
        Self(self.0 | mode.bit() | PresentModeKind::Fifo.bit())
    }

    #[must_use]
    pub const fn contains(self, mode: PresentModeKind) -> bool {
        matches!(mode, PresentModeKind::Fifo) || self.0 & mode.bit() != 0
    }

    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0 | PresentModeKind::Fifo.bit()
    }

    /// Inverse of [`Self::bits`]; unknown bits are dropped.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        let known = (1 << PresentModeKind::ALL.len()) - 1;
        Self((bits & known) | PresentModeKind::Fifo.bit())
    }
}

impl FromIterator<PresentModeKind> for SurfacePresentModes {
    fn from_iter<I: IntoIterator<Item = PresentModeKind>>(modes: I) -> Self {
        modes.into_iter().fold(Self::FIFO_ONLY, Self::with)
    }
}

/// Modes to try, best first; each list ends in FIFO.
const fn preference_order(intent: PresentationIntent) -> &'static [PresentModeKind] {
    match intent {
        PresentationIntent::Synchronized => &[PresentModeKind::Fifo],
        PresentationIntent::LowLatency | PresentationIntent::Unpaced => &[
            PresentModeKind::Immediate,
            PresentModeKind::Mailbox,
            PresentModeKind::Fifo,
        ],
    }
}

/// The mode to request before the surface is probed; player-facing intents start on FIFO.
#[must_use]
pub const fn initial_present_mode(intent: PresentationIntent) -> PresentModeKind {
    match intent {
        PresentationIntent::Unpaced => PresentModeKind::Immediate,
        PresentationIntent::Synchronized | PresentationIntent::LowLatency => PresentModeKind::Fifo,
    }
}

/// The mode to request; always one `supported` advertises, so no fallback applies.
#[must_use]
pub fn select_present_mode(
    intent: PresentationIntent,
    supported: SurfacePresentModes,
) -> PresentModeKind {
    preference_order(intent)
        .iter()
        .copied()
        .find(|mode| supported.contains(*mode))
        .unwrap_or(PresentModeKind::Fifo)
}

/// The admission rate for a limit; `None` leaves pacing to the display or to rendering.
#[must_use]
pub fn frame_rate_target(
    intent: PresentationIntent,
    limit: FrameRateLimit,
    display: DisplayTiming,
) -> Option<FrameRate> {
    let requested = match limit {
        FrameRateLimit::Automatic | FrameRateLimit::Unlimited => None,
        FrameRateLimit::Fixed(fps) => FrameRate::from_hz(u32::from(fps.get())),
    };
    let ceiling = (intent == PresentationIntent::LowLatency && display.vrr == VrrStatus::Active)
        .then_some(display.refresh)
        .flatten()
        .map(|refresh| vrr_ceiling(refresh, None));
    match (requested, ceiling) {
        (Some(requested), Some(ceiling)) => Some(requested.min(ceiling)),
        (requested, ceiling) => requested.or(ceiling),
    }
}

/// The highest rate that keeps variable refresh in range: 97% of the maximum, rounded down to
/// whole frames per second, lowered further when measured completion error needs more headroom.
#[must_use]
pub fn vrr_ceiling(max_refresh: FrameRate, completion_error_nanos: Option<u64>) -> FrameRate {
    let percent_hz = u64::from(max_refresh.millihertz()) * VRR_CEILING_PERCENT / 100 / 1_000;
    let initial = FrameRate::from_hz(u32::try_from(percent_hz.max(1)).unwrap_or(u32::MAX))
        .unwrap_or(max_refresh);
    let Some(error) = completion_error_nanos else {
        return initial;
    };
    let period = max_refresh
        .period_nanos()
        .saturating_add(error)
        .saturating_add(VRR_ERROR_MARGIN_NANOS);
    let measured = u32::try_from(1_000_000_000_000 / u128::from(period.max(1)))
        .ok()
        .and_then(FrameRate::from_millihertz);
    measured.map_or(initial, |measured| measured.min(initial))
}

/// The mode the renderer configures for `requested`, following its fallback order.
#[must_use]
pub fn configured_present_mode(
    requested: PresentModeKind,
    supported: SurfacePresentModes,
) -> PresentModeKind {
    let fallbacks: &[PresentModeKind] = match requested {
        PresentModeKind::Mailbox => &[PresentModeKind::Mailbox, PresentModeKind::Immediate],
        other => &[other],
    };
    fallbacks
        .iter()
        .copied()
        .find(|mode| supported.contains(*mode))
        .unwrap_or(PresentModeKind::Fifo)
}

#[cfg(test)]
#[path = "presentation/tests.rs"]
mod tests;
