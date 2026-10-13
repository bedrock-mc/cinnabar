//! Far-terrain policy shared by settings, both menus and far terrain itself.

/// Far terrain is on unless the player turns it off.
pub const DEFAULT_ENABLED: bool = true;

/// How much far-terrain work a device can take, judged by its recommended render distance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeviceTier {
    Low,
    Mid,
    High,
}

impl DeviceTier {
    /// Below 16 recommended chunks is low end, below 32 mid range, the rest high end.
    #[must_use]
    pub const fn from_recommended_render_distance(chunks: u8) -> Self {
        if chunks < 16 {
            Self::Low
        } else if chunks < 32 {
            Self::Mid
        } else {
            Self::High
        }
    }

    /// The most chunks of cached terrain this tier draws past what the server loaded.
    #[must_use]
    pub const fn max_far_reach_chunks(self) -> u32 {
        match self {
            Self::Low => 32,
            Self::Mid => 96,
            Self::High => 255,
        }
    }
}

/// The "Full-detail distance" setting's Auto value; other values are chunks.
pub const FULL_DETAIL_AUTO: u32 = 0;

/// The full-detail distances the Advanced setting offers besides Auto, in chunks. The
/// smallest stays beyond the 12-chunk actor render distance, so entities always stand on
/// full-detail terrain.
pub const MIN_FULL_DETAIL_CHUNKS: u32 = 16;
pub const MAX_FULL_DETAIL_CHUNKS: u32 = 64;
pub const FULL_DETAIL_STEP_CHUNKS: u32 = 16;

impl DeviceTier {
    /// Chunks meshed in full detail when the setting is Auto.
    #[must_use]
    pub const fn auto_full_detail_chunks(self) -> u32 {
        match self {
            Self::Low => 24,
            Self::Mid => 32,
            Self::High => 48,
        }
    }
}

/// The radius meshed in full detail, or `None` when everything within the render distance
/// is: the setting (Auto picks by tier), capped at the render distance.
#[must_use]
pub const fn full_detail_chunks(
    setting: u32,
    render_distance: u32,
    tier: DeviceTier,
) -> Option<u32> {
    let chosen = if setting == FULL_DETAIL_AUTO {
        tier.auto_full_detail_chunks()
    } else if setting < MIN_FULL_DETAIL_CHUNKS {
        MIN_FULL_DETAIL_CHUNKS
    } else {
        setting
    };
    if chosen < render_distance {
        Some(chosen)
    } else {
        None
    }
}

/// How far past the server's loaded radius cached terrain is drawn: out to the render
/// distance, which is how far the player sees, within the device tier's budget.
#[must_use]
pub const fn far_reach_chunks(render_distance: u32, loaded_radius: u32, tier: DeviceTier) -> u32 {
    let wanted = render_distance.saturating_sub(loaded_radius);
    let cap = tier.max_far_reach_chunks();
    if wanted < cap { wanted } else { cap }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cached terrain fills only the gap between what the server sends and the render distance.
    #[test]
    fn far_reach_fills_the_gap_to_the_render_distance_within_the_tier() {
        assert_eq!(far_reach_chunks(64, 12, DeviceTier::High), 52);
        assert_eq!(far_reach_chunks(12, 12, DeviceTier::High), 0);
        assert_eq!(far_reach_chunks(8, 12, DeviceTier::High), 0);
        assert_eq!(far_reach_chunks(255, 8, DeviceTier::Low), 32);
        assert_eq!(
            DeviceTier::from_recommended_render_distance(10),
            DeviceTier::Low
        );
        assert_eq!(
            DeviceTier::from_recommended_render_distance(28),
            DeviceTier::Mid
        );
        assert_eq!(
            DeviceTier::from_recommended_render_distance(48),
            DeviceTier::High
        );
    }
}
