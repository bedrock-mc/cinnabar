//! Merging sampled columns into stored sections: the read-modify-write their single
//! writer runs.
//!
//! A level-0 section holds the sub-chunks of four columns and each coarser section eight
//! children, so a column's octants are written into the sections it shares with others,
//! then mipped up to [`LOCAL_MIP_TOP_LEVEL`]. Mips read their neighbour planes from what
//! the [`SectionSource`] and this merge hold at the same level. When a section is created,
//! or its content changes on a face, every present neighbour across that face mips its
//! facing slab again. Merging columns one at a time, in any order, therefore gives the
//! same sections as merging them together. Levels above [`LOCAL_MIP_TOP_LEVEL`] are left
//! to [`crate::mip::remip_from_children`].
//!
//! A column's sections up to [`LOCAL_MIP_TOP_LEVEL`] all lie in one [`WriterRegion`], so
//! writers that each own whole regions never write the same section. Only a neighbour
//! slab re-mip crosses a region border; [`merge_owned`] hands those to the caller as
//! [`SlabWork`] for the neighbour's owner to merge once this merge is stored, which
//! gives the same sections as one writer merging everything.

mod merge;

use std::collections::BTreeMap;

use self::merge::{LevelWork, Merge};
use crate::{
    classify::MaterialClasses,
    geometry::{Face, LEVEL_COUNT, SectionKey, SectionPos},
    mip::LOCAL_MIP_TOP_LEVEL,
    sampler::ColumnSample,
    section::Section,
};

/// Where a merge reads the sections it does not hold.
pub trait SectionSource {
    type Error;

    /// The stored section at `pos`; `None` when none is stored. A merge reads each
    /// position at most once.
    fn load(&mut self, pos: SectionPos) -> Result<Option<Section>, Self::Error>;
}

/// What one merge did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MergeStats {
    /// Sampled sub-chunks written into level-0 sections.
    pub sub_chunks: u32,
    /// Sections the merge rewrote.
    pub sections: u32,
    /// Face slabs of present neighbours mipped again.
    pub neighbour_slabs: u32,
    pub hidden_fallbacks: u32,
    pub unresolved_hidden: u32,
}

/// The sections a merge rewrote, ordered by key, and its counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    pub sections: Vec<Section>,
    pub stats: MergeStats,
    /// Slab re-mips of neighbours the merge does not own, ordered and without repeats;
    /// empty for [`merge_columns`].
    pub foreign: Vec<SlabWork>,
}

/// The column of [`LOCAL_MIP_TOP_LEVEL`] sections, in that level's section units, that
/// holds a section at or below that level, its whole local mip chain and every column
/// whose samples write it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WriterRegion {
    pub x: i32,
    pub z: i32,
}

impl WriterRegion {
    /// The region holding `pos`; `None` above [`LOCAL_MIP_TOP_LEVEL`], which local merges
    /// never write.
    #[must_use]
    pub fn of(pos: SectionPos) -> Option<Self> {
        let top = pos.ancestor(LOCAL_MIP_TOP_LEVEL.checked_sub(pos.level())?)?;
        Some(Self {
            x: top.x(),
            z: top.z(),
        })
    }

    /// The region holding chunk column `(x, z)`'s sections.
    #[must_use]
    pub fn of_chunk(x: i32, z: i32) -> Self {
        let (base, _) =
            SectionPos::of_sub_chunk(x, 0, z).expect("sub-chunk Y 0 lies in the key range");
        Self::of(base).expect("level 0 is a local level")
    }
}

/// One stored section's slab touching `face` to mip into its parent again, because the
/// same-level neighbour across that face was created or changed there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlabWork {
    pub pos: SectionPos,
    pub face: Face,
}

/// Writes every sampled sub-chunk of `samples` into its level-0 section and mips the
/// change up to [`LOCAL_MIP_TOP_LEVEL`], re-mipping the facing slabs of present
/// neighbours. Returns the rewritten sections; the caller stores them before the next
/// merge reads the same source. Fails only when `source` fails, writing nothing.
pub fn merge_columns<'a, S: SectionSource>(
    samples: impl IntoIterator<Item = &'a ColumnSample>,
    source: &mut S,
    classes: &impl MaterialClasses,
) -> Result<Merged, S::Error> {
    merge_owned(samples, [], |_| true, source, classes)
}

/// [`merge_columns`] for a writer that owns the sections `owns` accepts, whole
/// [`WriterRegion`]s at a time, which must include every section `samples` write. It also
/// re-mips `slabs` other writers handed over, for those sections that are present; and
/// instead of re-mipping the slab of a neighbour it does not own, it returns that work in
/// [`Merged::foreign`], whether or not the neighbour is present. The caller stores the
/// merge, then hands each foreign slab to its owner, which merges it next.
pub fn merge_owned<'a, S: SectionSource>(
    samples: impl IntoIterator<Item = &'a ColumnSample>,
    slabs: impl IntoIterator<Item = SlabWork>,
    owns: impl Fn(SectionPos) -> bool,
    source: &mut S,
    classes: &impl MaterialClasses,
) -> Result<Merged, S::Error> {
    let mut merge = Merge::new(source, classes, &owns);
    let mut levels: [BTreeMap<SectionKey, LevelWork>; LEVEL_COUNT] = Default::default();
    for sample in samples {
        for sub in &sample.sub_chunks {
            merge.write_sub_chunk(sub)?;
            levels[0]
                .entry(sub.section().key())
                .or_default()
                .change(sub.region());
        }
    }
    let mut handed: [Vec<SlabWork>; LEVEL_COUNT] = Default::default();
    for slab in slabs {
        debug_assert!(owns(slab.pos), "{slab:?} is handed to its owner");
        if slab.pos.level() < LOCAL_MIP_TOP_LEVEL {
            handed[usize::from(slab.pos.level())].push(slab);
        }
    }
    for level in 0..usize::from(LOCAL_MIP_TOP_LEVEL) {
        let (current, coarser) = levels.split_at_mut(level + 1);
        merge.schedule_neighbour_slabs(&mut current[level])?;
        merge.schedule_handed_slabs(&handed[level], &mut current[level])?;
        for (key, work) in std::mem::take(&mut current[level]) {
            merge.mip_into_parent(key.pos(), work, &mut coarser[0])?;
        }
    }
    Ok(merge.finish())
}

#[cfg(test)]
mod tests;
