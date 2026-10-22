//! Chooses when current-frame occlusion can repay its depth and compute work.

pub(super) const PROBE_INTERVAL: u64 = 64;

/// Small views keep direct drawing; inactive large views occasionally measure occlusion.
#[derive(Default)]
pub(super) struct VisibilityAdmission {
    profitable: bool,
    next_probe: u64,
    observed: u64,
}

impl VisibilityAdmission {
    /// Requests a fresh depth test, never permission to cull using an older view's bits.
    pub(super) fn select(&mut self, frame: u64, candidates: usize) -> bool {
        const MIN_CANDIDATES: usize = 64;
        if candidates < MIN_CANDIDATES {
            return false;
        }
        self.select_work(frame)
    }

    /// Admits work when candidate size is available only from completed GPU feedback.
    pub(super) fn select_work(&mut self, frame: u64) -> bool {
        if self.profitable || frame >= self.next_probe {
            self.next_probe = frame.saturating_add(PROBE_INTERVAL);
            true
        } else {
            false
        }
    }

    /// Uses completed visible-candidate counts only to admit work, never to hide geometry.
    pub(super) fn observe(&mut self, frame: u64, candidates: usize, hidden: usize) {
        if frame <= self.observed || candidates == 0 {
            return;
        }
        self.observed = frame;
        self.profitable = hidden >= candidates.div_ceil(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_views_keep_fresh_occlusion_only_when_it_hides_enough_work() {
        let mut policy = VisibilityAdmission::default();
        assert!(!policy.select(1, 8));
        assert!(policy.select(2, 128));
        policy.observe(2, 128, 100);
        for frame in 3..20 {
            assert!(policy.select(frame, 128));
        }
        policy.observe(19, 128, 8);
        assert!(!policy.select(20, 128));
        policy.observe(2, 128, 128);
        assert!(
            !policy.select(21, 128),
            "old feedback must not re-enable work"
        );
        assert!(policy.select(83, 128));
        policy.observe(83, 128, 64);
        assert!(policy.select(84, 128));
        assert!(!policy.select(85, 16));
    }
}
