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

mod merge;

use std::collections::BTreeMap;

use self::merge::{LevelWork, Merge};
use crate::{
    classify::MaterialClasses,
    geometry::{LEVEL_COUNT, SectionKey, SectionPos},
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
    let mut merge = Merge::new(source, classes);
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
    for level in 0..usize::from(LOCAL_MIP_TOP_LEVEL) {
        let (current, coarser) = levels.split_at_mut(level + 1);
        merge.schedule_neighbour_slabs(&mut current[level])?;
        for (key, work) in std::mem::take(&mut current[level]) {
            merge.mip_into_parent(key.pos(), work, &mut coarser[0])?;
        }
    }
    Ok(merge.finish())
}

#[cfg(test)]
mod tests;
