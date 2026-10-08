//! The camera's sleep fade: a dark tint while asleep, eased out on waking.

use super::{HudFrame, HudLayout, UiPresentationError};

/// 26.30's `PlayerSleepFadeEffectSystemUtil` fade: 5 s in, 0.5 s out, to
/// RGB (16, 16, 32).
const FADE_IN_MILLIS: u64 = 5_000;
const FADE_OUT_MILLIS: u64 = 500;
const TINT: [u8; 3] = [16, 16, 32];
/// Peak opacity; the fade's strength argument (0.863) is not confirmed as this.
const PEAK_ALPHA: f32 = 0.7;

/// Tint strength over time, derived from the local sleeping flag.
#[derive(Clone, Copy, Debug, Default)]
pub struct SleepTimeline {
    asleep_since: Option<u64>,
    /// Wake time and the strength it started fading from.
    fading: Option<(u64, f32)>,
}

impl SleepTimeline {
    pub fn observe(&mut self, sleeping: bool, now_millis: u64) {
        match (sleeping, self.asleep_since) {
            (true, None) => {
                self.asleep_since = Some(now_millis);
                self.fading = None;
            }
            (false, Some(_)) => {
                self.fading = Some((now_millis, self.strength(now_millis)));
                self.asleep_since = None;
            }
            _ => {}
        }
    }

    /// Milliseconds since the player lay down, while asleep.
    pub fn asleep_for(&self, now_millis: u64) -> Option<u64> {
        self.asleep_since
            .map(|since| now_millis.saturating_sub(since))
    }

    /// Tint opacity factor in `0.0..=1.0`.
    pub fn strength(&self, now_millis: u64) -> f32 {
        if let Some(since) = self.asleep_since {
            let elapsed = now_millis.saturating_sub(since);
            return (elapsed as f32 / FADE_IN_MILLIS as f32).min(1.0);
        }
        self.fading.map_or(0.0, |(woke, from)| {
            let elapsed = now_millis.saturating_sub(woke);
            from * (1.0 - (elapsed as f32 / FADE_OUT_MILLIS as f32).min(1.0))
        })
    }
}

impl HudLayout<'_> {
    /// Draws the tint under the HUD.
    pub(super) fn sleep_overlay(&mut self, frame: &HudFrame) -> Result<(), UiPresentationError> {
        let g = self.geometry;
        let strength = frame.sleep.strength(frame.now_millis);
        let alpha = (PEAK_ALPHA * strength * 255.0).round() as u8;
        if alpha > 0 {
            self.solid_gui(
                [0.0, 0.0],
                [g.gui_width, g.gui_height],
                [TINT[0], TINT[1], TINT[2], alpha],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tint_fades_in_while_asleep_and_out_after_waking() {
        let mut timeline = SleepTimeline::default();
        assert_eq!(timeline.strength(0), 0.0);
        timeline.observe(true, 1_000);
        assert_eq!(timeline.strength(1_000), 0.0);
        assert!((timeline.strength(3_500) - 0.5).abs() < 1e-6);
        assert_eq!(timeline.strength(60_000), 1.0);
        timeline.observe(false, 6_000);
        assert!(timeline.strength(6_250) > 0.0 && timeline.strength(6_250) < 1.0);
        assert_eq!(timeline.strength(6_500), 0.0);
        assert!(timeline.asleep_for(6_500).is_none());
    }

    #[test]
    fn waking_early_fades_from_the_reached_strength() {
        let mut timeline = SleepTimeline::default();
        timeline.observe(true, 0);
        timeline.observe(false, 2_500);
        assert!((timeline.strength(2_500) - 0.5).abs() < 1e-6);
        assert!((timeline.strength(2_750) - 0.25).abs() < 1e-6);
    }
}
