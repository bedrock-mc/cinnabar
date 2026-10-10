//! Timed loading art from the optional version-matched OreUI bundle.

use super::super::super::UiPresentationError;
use super::paint::{Bounds, Canvas};

impl Canvas<'_> {
    /// Draws the current loading frame, or reports that install art is unavailable.
    pub(super) fn loading_sprite(&mut self, bounds: Bounds) -> Result<bool, UiPresentationError> {
        let Some(originals) = self.originals else {
            return Ok(false);
        };
        let duration: u64 = originals
            .loading_frames
            .iter()
            .map(|(_, ms)| u64::from(*ms))
            .sum();
        if duration == 0 {
            return Ok(false);
        }
        let elapsed = if self.seconds.is_finite() && self.seconds > 0.0 {
            (self.seconds * 1000.0) % duration as f64
        } else {
            0.0
        };
        let mut end = 0;
        for (key, millis) in originals.loading_frames.iter() {
            end += u64::from(*millis);
            if elapsed < end as f64 {
                return self.sprite(key, bounds, [255; 4]);
            }
        }
        Ok(false)
    }
}
