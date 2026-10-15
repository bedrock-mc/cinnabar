//! Ordered alpha segments and the boundaries that cannot share retained depth.
use super::*;
use std::ops::Range;

/// One gamma-layer draw: its layered batch range, then an optional invert batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UiSegment {
    pub(crate) layered: Range<usize>,
    pub(crate) inverted: Option<usize>,
    /// The layer composites in the output pass instead of over the main texture.
    pub(crate) present: bool,
}

/// Ordered alpha/invert operations for one overlay, including deferred output policy.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct UiPassPlan {
    pub(crate) segments: Vec<UiSegment>,
}

/// Splits the HUD into layer and invert passes; the last layer waits for the output pass.
pub(crate) fn plan_ui_passes<T: std::borrow::Borrow<UiRenderBatch>, L>(
    batches: &[(usize, T, L)],
    deferred_present: bool,
) -> UiPassPlan {
    let mut segments = Vec::new();
    let mut start = 0;
    for (index, (_, batch, _)) in batches.iter().enumerate() {
        if batch.borrow().blend_mode == UI_BLEND_INVERT {
            segments.push(UiSegment {
                layered: start..index,
                inverted: Some(index),
                present: false,
            });
            start = index + 1;
        }
    }
    if start < batches.len() {
        segments.push(UiSegment {
            layered: start..batches.len(),
            inverted: None,
            present: deferred_present,
        });
    }
    UiPassPlan { segments }
}

impl UiPassPlan {
    /// Empty alpha ranges still preserve inversion order but need no retained color storage.
    pub(super) fn layer_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| !segment.layered.is_empty())
            .count()
    }
}

impl UiSegment {
    /// Maps filtered overlay batches back to their immutable publication range.
    pub(super) fn publication_range<T, L>(&self, batches: &[(usize, T, L)]) -> Range<usize> {
        if self.layered.is_empty() {
            let edge = self.inverted.map_or(0, |index| batches[index].0);
            return edge..edge;
        }
        batches[self.layered.start].0..batches[self.layered.end - 1].0 + 1
    }

    /// Accepted publications keep each depth scope contiguous. Only scopes crossing
    /// either segment boundary can supply depth to another alpha segment or invert.
    pub(super) fn independent_depth<T: std::borrow::Borrow<UiRenderBatch>, L>(
        &self,
        batches: &[(usize, T, L)],
    ) -> bool {
        let start = self.layered.start;
        let end = self.layered.end;
        if start == end {
            return true;
        }
        let scope = |index: usize| batches[index].1.borrow().isolated_depth_scope;
        let before = start > 0 && scope(start).is_some() && scope(start) == scope(start - 1);
        let after = end < batches.len() && scope(end - 1).is_some() && scope(end - 1) == scope(end);
        !before && !after
    }
}
