//! The player's frame-rate limit, shared by settings, launcher persistence and presentation.

use std::num::NonZeroU16;

/// The player's FPS cap; presentation and variable refresh may impose a lower rate.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum FrameRateLimit {
    /// Untouched setting: unlimited on mains, display refresh on battery.
    #[default]
    Automatic,
    /// Frames per second.
    Fixed(NonZeroU16),
    Unlimited,
}
