//! Current admitted models take priority over speculative frames in the GPU array.

use super::*;

impl ModelCache {
    /// Publishes admitted artwork first, then cached demand and animation frames that fit.
    pub(super) fn publication_mask(&self) -> [bool; MAX_ITEM_LAYERS - 1] {
        let mut published = [false; MAX_ITEM_LAYERS - 1];
        let mut budget = self.budget;
        budget.reset();
        let admitted = |slot: usize| {
            self.residents[slot]
                .as_ref()
                .is_some_and(|entry| entry.admitted == self.frame)
        };
        for slot in (0..self.models.len())
            .filter(|&slot| admitted(slot))
            .chain((0..self.models.len()).filter(|&slot| !admitted(slot)))
        {
            let Some(entry) = self.residents[slot].as_ref() else {
                continue;
            };
            if admitted(slot) || self.demanded(slot) || entry.warmed == self.frame {
                published[slot] = budget.admit(&self.models[slot], entry.vertices).is_some();
                debug_assert!(
                    !admitted(slot) || published[slot],
                    "selected artwork must fit publication"
                );
            }
        }
        published
    }
}
