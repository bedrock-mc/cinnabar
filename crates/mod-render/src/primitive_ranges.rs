//! Allocation tags refer only to ranges in the output's canonical primitive list.

use crate::Primitives;
use std::ops::Range;

/// Tags a partition of the admitted primitive list with its reservation's allocation owner.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrimitiveRanges {
    pub decals: Range<usize>,
    pub ribbons: Range<usize>,
    pub beams: Range<usize>,
    pub billboards: Range<usize>,
}

impl PrimitiveRanges {
    /// Selects every primitive in an independently validated output.
    pub fn all(primitives: &Primitives) -> Self {
        Self {
            decals: 0..primitives.decals.len(),
            ribbons: 0..primitives.ribbons.len(),
            beams: 0..primitives.beams.len(),
            billboards: 0..primitives.billboards.len(),
        }
    }

    /// Moves local tags into the admitted output, clipping tags whose primitives were dropped.
    pub(crate) fn remap(&self, admitted: &Self) -> Self {
        Self {
            decals: remap(&self.decals, &admitted.decals),
            ribbons: remap(&self.ribbons, &admitted.ribbons),
            beams: remap(&self.beams, &admitted.beams),
            billboards: remap(&self.billboards, &admitted.billboards),
        }
    }
}

/// Translates one local range into the surviving prefix of its source's admitted range.
fn remap(local: &Range<usize>, admitted: &Range<usize>) -> Range<usize> {
    let length = admitted.len();
    admitted.start + local.start.min(length)..admitted.start + local.end.min(length)
}
