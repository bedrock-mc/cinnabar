//! How many bytes of new distant terrain one frame uploads: [`START_UPLOAD_BYTES`] at first,
//! growing toward [`MAX_UPLOAD_BYTES`] while frames are fast and shrinking back when they
//! are slow.

use super::layout::{MAX_UPLOAD_BYTES, START_UPLOAD_BYTES};

/// Frames at or under this many seconds let the budget grow.
pub(crate) const FAST_FRAME_SECONDS: f32 = 1.0 / 60.0;
/// Frames over this many seconds halve the budget.
pub(crate) const SLOW_FRAME_SECONDS: f32 = 1.0 / 40.0;
/// Bytes the budget grows by after a fast frame.
pub(crate) const UPLOAD_STEP_BYTES: u64 = 512 << 10;

/// See the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UploadBudget(u64);

impl Default for UploadBudget {
    fn default() -> Self {
        Self(START_UPLOAD_BYTES)
    }
}

impl UploadBudget {
    /// Bytes the next frame may upload.
    pub(crate) fn bytes(self) -> u64 {
        self.0
    }

    /// Adapts to a frame that took `seconds`; a frame that uploaded nothing tells nothing
    /// about upload cost, so `uploaded` false leaves a fast frame's budget unchanged.
    pub(crate) fn observe(&mut self, seconds: f32, uploaded: bool) {
        if !seconds.is_finite() || seconds <= 0.0 {
            return;
        }
        if seconds > SLOW_FRAME_SECONDS {
            self.0 = (self.0 / 2).max(START_UPLOAD_BYTES);
        } else if uploaded && seconds <= FAST_FRAME_SECONDS {
            self.0 = (self.0 + UPLOAD_STEP_BYTES).min(MAX_UPLOAD_BYTES);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_grows_on_fast_uploading_frames_and_halves_on_slow_ones() {
        let mut budget = UploadBudget::default();
        assert_eq!(budget.bytes(), START_UPLOAD_BYTES);
        budget.observe(1.0 / 120.0, false);
        assert_eq!(budget.bytes(), START_UPLOAD_BYTES, "nothing uploaded");
        for _ in 0..100 {
            budget.observe(1.0 / 120.0, true);
        }
        assert_eq!(budget.bytes(), MAX_UPLOAD_BYTES);
        budget.observe(1.0 / 20.0, true);
        assert_eq!(budget.bytes(), MAX_UPLOAD_BYTES / 2);
        for _ in 0..10 {
            budget.observe(1.0 / 20.0, false);
        }
        assert_eq!(budget.bytes(), START_UPLOAD_BYTES);
        budget.observe(f32::NAN, true);
        budget.observe(1.0 / 50.0, true);
        assert_eq!(budget.bytes(), START_UPLOAD_BYTES, "between the thresholds");
    }
}
