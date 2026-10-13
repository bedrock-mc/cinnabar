//! Caller-supplied wall-clock time at minute resolution.

/// Seconds in one [`Minute`].
pub const SECONDS_PER_MINUTE: u64 = 60;

/// Whole minutes since the Unix epoch, read by the caller from its clock. Stores persist
/// access times at this resolution; nothing in this crate reads a clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Minute(u32);

impl Minute {
    /// The Unix epoch, older than every real access.
    pub const EPOCH: Self = Self(0);

    /// The minute `minutes` after the Unix epoch.
    #[must_use]
    pub const fn new(minutes: u32) -> Self {
        Self(minutes)
    }

    /// The minute holding `seconds` after the Unix epoch, saturating far in the future.
    #[must_use]
    pub const fn from_unix_seconds(seconds: u64) -> Self {
        let minutes = seconds / SECONDS_PER_MINUTE;
        Self(if minutes > u32::MAX as u64 {
            u32::MAX
        } else {
            minutes as u32
        })
    }

    /// Minutes since the Unix epoch.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Minutes from `earlier` to this minute; zero when `earlier` is later.
    #[must_use]
    pub const fn minutes_since(self, earlier: Self) -> u32 {
        self.0.saturating_sub(earlier.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_round_down_and_saturate() {
        assert_eq!(Minute::from_unix_seconds(119), Minute::new(1));
        assert_eq!(Minute::from_unix_seconds(u64::MAX), Minute::new(u32::MAX));
        assert_eq!(Minute::new(10).minutes_since(Minute::new(4)), 6);
        assert_eq!(Minute::new(4).minutes_since(Minute::new(10)), 0);
    }
}
