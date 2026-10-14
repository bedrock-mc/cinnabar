//! Regions an undecided session stops reading through to a stored epoch.

use std::collections::HashSet;

use foldhash::fast::FixedState;
use lod::{
    epoch::RegionPos,
    geometry::{MAX_LEVEL, SectionKey},
};

/// Signature regions whose vote against one stored epoch found different terrain, kept as
/// the section columns of every level that overlap them, so checking a record's key is
/// one lookup.
#[derive(Debug, Default)]
pub(super) struct HiddenRegions {
    /// `(level, section x, section z)` of every hidden column.
    columns: HashSet<(u8, i64, i64), FixedState>,
}

impl HiddenRegions {
    /// Hides every section column at every level that overlaps `region`.
    pub(super) fn hide(&mut self, region: RegionPos) {
        for level in 0..=MAX_LEVEL {
            let [xs, zs] = region.section_columns(level);
            for x in xs {
                for z in zs.clone() {
                    self.columns.insert((level, x, z));
                }
            }
        }
    }

    /// True when the section of `key` overlaps a hidden region.
    pub(super) fn hides(&self, key: SectionKey) -> bool {
        let pos = key.pos();
        self.columns
            .contains(&(pos.level(), i64::from(pos.x()), i64::from(pos.z())))
    }
}

#[cfg(test)]
mod tests {
    use lod::geometry::SectionPos;

    use super::*;

    #[test]
    fn a_hidden_region_hides_every_section_overlapping_it_at_every_level() {
        let mut hidden = HiddenRegions::default();
        // Blocks 64..128 along X and -64..0 along Z.
        hidden.hide(RegionPos { x: 1, z: -1 });
        let hides = |level, x, z| {
            hidden.hides(
                SectionPos::new(level, x, 3, z)
                    .expect("test position")
                    .key(),
            )
        };
        for (x, z) in [(2, -2), (3, -2), (2, -1), (3, -1)] {
            assert!(hides(0, x, z), "level 0 ({x}, {z})");
        }
        assert!(!hides(0, 4, -1) && !hides(0, 1, -1) && !hides(0, 2, 0));
        assert!(hides(1, 1, -1) && !hides(1, 0, -1) && !hides(1, 1, 0));
        assert!(hides(2, 0, -1) && !hides(2, 1, -1));
        assert!(hides(MAX_LEVEL, 0, -1) && !hides(MAX_LEVEL, 0, 0));
    }
}
