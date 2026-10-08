//! Present-mode selection shared by window setup and telemetry, independent of the GPU backend.

/// What the player asked presentation to optimise for.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PresentationIntent {
    /// Display-paced, tear-free delivery.
    Synchronized,
    /// Present as soon as a frame is ready, preferring modes that do not wait for the display.
    LowLatency,
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

/// Modes to try for `intent`, best first; each list ends in FIFO.
const fn preference_order(intent: PresentationIntent) -> &'static [PresentModeKind] {
    match intent {
        PresentationIntent::Synchronized => &[PresentModeKind::Fifo],
        PresentationIntent::LowLatency => &[
            PresentModeKind::Immediate,
            PresentModeKind::Mailbox,
            PresentModeKind::Fifo,
        ],
    }
}

/// The mode to request before the surface has been probed; the renderer falls back to FIFO.
#[must_use]
pub const fn initial_present_mode(intent: PresentationIntent) -> PresentModeKind {
    preference_order(intent)[0]
}

/// The mode to request for `intent`; always one `supported` advertises, so no fallback applies.
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
