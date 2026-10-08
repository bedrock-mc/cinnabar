//! The player's frame-rate limit, shared by settings, launcher persistence and presentation.

use std::num::NonZeroU16;

/// How fast the client may render; `Automatic` lets the presentation policy choose per display.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum FrameRateLimit {
    #[default]
    Automatic,
    /// Frames per second.
    Fixed(NonZeroU16),
    Unlimited,
}
