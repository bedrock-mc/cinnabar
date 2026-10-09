//! Bounded frame counters for local mutations and their mesh handoff.

use world::SubChunkKey;

#[derive(Debug, Default)]
pub(crate) struct PredictionFrames {
    pub(crate) committed_frame: Option<u32>,
    pub(crate) staged_frame: Option<u32>,
    pub(crate) upload_acknowledged_frame: Option<u32>,
    pending: Vec<(SubChunkKey, u64, bool, bool)>,
}

impl PredictionFrames {
    /// Bounds late worker service to the latest placement's initial publication.
    pub(crate) fn publication_budget(&self, frame: u32) -> usize {
        if self.staged_frame.is_none()
            && self
                .committed_frame
                .is_some_and(|committed| frame.wrapping_sub(committed) < 64)
        {
            self.pending.len()
        } else {
            0
        }
    }

    /// Starts one receipt for all sub-chunks touched by the latest local placement.
    pub(crate) fn committed(&mut self, frame: u32, keys: impl Iterator<Item = (SubChunkKey, u64)>) {
        *self = Self {
            committed_frame: Some(frame),
            ..Self::default()
        };
        for (key, generation) in keys {
            if !self.pending.iter().any(|cell| cell.0 == key) {
                self.pending.push((key, generation, false, false));
            }
        }
    }

    /// Records the frame that handed every predicted generation to the render queue.
    pub(crate) fn staged(&mut self, frame: u32, key: SubChunkKey, generation: u64) {
        if let Some(cell) = self
            .pending
            .iter_mut()
            .find(|cell| cell.0 == key && cell.1 == generation)
        {
            cell.2 = true;
            if self.staged_frame.is_none() && self.pending.iter().all(|cell| cell.2) {
                self.staged_frame = Some(frame);
            }
        }
    }

    /// Records the main frame that received acknowledgements for every predicted upload.
    pub(crate) fn uploaded(&mut self, frame: u32, key: SubChunkKey, generation: u64) {
        if let Some(cell) = self
            .pending
            .iter_mut()
            .find(|cell| cell.0 == key && cell.1 == generation)
        {
            cell.3 = true;
            if self.upload_acknowledged_frame.is_none() && self.pending.iter().all(|cell| cell.3) {
                self.upload_acknowledged_frame = Some(frame);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_receipt_requires_both_generations_and_ignores_corrections() {
        let keys = [SubChunkKey::new(0, 0, 0, 0), SubChunkKey::new(0, 0, 1, 0)];
        let mut receipt = PredictionFrames::default();
        receipt.committed(10, [(keys[0], 1), (keys[1], 2)].into_iter());
        assert_eq!(receipt.publication_budget(10), 2);
        receipt.staged(10, keys[0], 1);
        receipt.staged(11, keys[1], 3);
        assert_eq!(receipt.staged_frame, None);
        receipt.staged(12, keys[1], 2);
        assert_eq!(receipt.staged_frame, Some(12));
        assert_eq!(receipt.publication_budget(12), 0);
        receipt.uploaded(13, keys[0], 1);
        assert_eq!(receipt.upload_acknowledged_frame, None);
        receipt.uploaded(14, keys[1], 2);
        assert_eq!(receipt.upload_acknowledged_frame, Some(14));
    }
}
