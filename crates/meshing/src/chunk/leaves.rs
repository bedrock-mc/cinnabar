//! Native leaf render-layer and shared-plane selection, using primary storage.
//!
//! Vanilla leaves test six neighbours for depth and keep only one of the
//! two coincident non-deep leaf faces, whose material disables backface culling.

use std::cell::Cell;

use assets::{
    BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF, BLOCK_VISUAL_VARIANT_SEASONAL_LEAF,
    BLOCK_VISUAL_VARIANT_TOP_SNOW, BlockFlags,
};
use world::MeshSample;

use super::{models::PaletteResolutionContext, opaque::face_offset};
use crate::{
    Face, SIDE,
    contributors::{ResolvedPaletteEntry, resolve_palette_entry},
};

// Only the center and its one-block halo can own a tested shared plane. Each
// memoized predicate may read one further block from the bounded 3x3x3 snapshot.
const CACHE_SIDE: usize = SIDE + 2;
const CACHE_WORDS: usize = (CACHE_SIDE * CACHE_SIDE * CACHE_SIDE).div_ceil(u64::BITS as usize);

pub(crate) struct LeafOcclusion<'assets, 'chunks> {
    context: PaletteResolutionContext<'assets, 'chunks>,
    known: [Cell<u64>; CACHE_WORDS],
    deep: [Cell<u64>; CACHE_WORDS],
}

impl<'assets, 'chunks> LeafOcclusion<'assets, 'chunks> {
    pub(crate) fn new(context: PaletteResolutionContext<'assets, 'chunks>) -> Self {
        Self {
            context,
            known: std::array::from_fn(|_| Cell::new(0)),
            deep: std::array::from_fn(|_| Cell::new(0)),
        }
    }

    fn primary(&self, coordinate: [i32; 3]) -> ResolvedPaletteEntry {
        match self.context.neighbourhood.sample(0, coordinate) {
            MeshSample::Open => ResolvedPaletteEntry::AIR,
            MeshSample::Block(id) => resolve_palette_entry(
                self.context.classifier,
                self.context.visuals,
                self.context.network_id_mode,
                id,
            ),
        }
    }

    pub(crate) fn is_deep(&self, coordinate: [i32; 3]) -> bool {
        let Some(index) = cache_index(coordinate) else {
            return false;
        };
        let word = index / u64::BITS as usize;
        let bit = 1 << (index % u64::BITS as usize);
        if self.known[word].get() & bit != 0 {
            return self.deep[word].get() & bit != 0;
        }
        // Both native leaf types use this predicate; only ordinary leaves
        // add the seasonal colour material flag.
        let deep = self.primary(coordinate).variant
            & (BLOCK_VISUAL_VARIANT_SEASONAL_LEAF | BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF)
            != 0
            && Face::ALL.into_iter().all(|face| {
                let neighbour = self.primary(adjacent(coordinate, face));
                neighbour
                    .flags
                    .intersects(BlockFlags::LEAF_MODEL | BlockFlags::OCCLUDES_FULL_FACE)
                    || (face == Face::PositiveY
                        && neighbour.variant & BLOCK_VISUAL_VARIANT_TOP_SNOW != 0)
            });
        self.known[word].set(self.known[word].get() | bit);
        if deep {
            self.deep[word].set(self.deep[word].get() | bit);
        }
        deep
    }

    pub(crate) fn culls_face(&self, coordinate: [usize; 3], face: Face) -> bool {
        let source = coordinate.map(|value| value as i32);
        let neighbour = adjacent(source, face);
        if !self
            .primary(neighbour)
            .flags
            .contains(BlockFlags::LEAF_MODEL)
        {
            return false;
        }
        if self.is_deep(neighbour) {
            return true;
        }
        self.primary(source).flags.contains(BlockFlags::LEAF_MODEL)
            && !self.is_deep(source)
            && matches!(face, Face::PositiveY | Face::NegativeZ | Face::NegativeX)
    }
}

fn adjacent(coordinate: [i32; 3], face: Face) -> [i32; 3] {
    let offset = face_offset(face);
    std::array::from_fn(|axis| coordinate[axis] + i32::from(offset[axis]))
}

fn cache_index(coordinate: [i32; 3]) -> Option<usize> {
    let [x, y, z] = coordinate.map(|value| value + 1);
    if ![x, y, z]
        .into_iter()
        .all(|value| (0..CACHE_SIDE as i32).contains(&value))
    {
        return None;
    }
    Some((x as usize * CACHE_SIDE + y as usize) * CACHE_SIDE + z as usize)
}
