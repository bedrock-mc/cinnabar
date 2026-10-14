use std::{collections::HashMap, convert::Infallible};

use world::{ChunkKey, DecodedBiomeColumn};

use super::*;
use crate::{
    fixtures::{biome_column, fixture_classifier, terrain_column},
    geometry::Face,
    sampler::{ColumnSnapshot, sample_column},
    section::ChildState,
};

/// Sections stored in memory, as a store would hold them between merges.
#[derive(Debug, Default, Clone, PartialEq)]
struct Memory {
    sections: HashMap<SectionKey, Section>,
    loads: usize,
}

impl SectionSource for Memory {
    type Error = Infallible;

    fn load(&mut self, pos: SectionPos) -> Result<Option<Section>, Infallible> {
        self.loads += 1;
        Ok(self.sections.get(&pos.key()).cloned())
    }
}

impl Memory {
    /// Merges `columns` and stores what the merge rewrote.
    fn merge(&mut self, columns: &[ColumnSample]) -> MergeStats {
        let classes = fixture_classifier();
        let Ok(merged) = merge_columns(columns, self, &classes);
        for section in merged.sections {
            self.sections.insert(section.pos().key(), section);
        }
        merged.stats
    }
}

/// The fixture terrain of chunk `(x, z)`, sampled without solved light.
fn column(x: i32, z: i32) -> ColumnSample {
    let sub_chunks = terrain_column(x, z, 0..8);
    let biomes: DecodedBiomeColumn = biome_column(0, &[1, 1, 2, 2, 3, 3, 4, 4]);
    let snapshot = ColumnSnapshot {
        chunk: ChunkKey::new(0, x, z),
        sub_chunks: &sub_chunks,
        biomes: &biomes,
        default_biome: 0,
        light: None,
    };
    sample_column(&snapshot, &mut fixture_classifier()).expect("fixture columns have keys")
}

/// The stored sections after merging `columns` one at a time in order.
fn one_at_a_time(columns: &[ColumnSample]) -> HashMap<SectionKey, Section> {
    let mut memory = Memory::default();
    for column in columns {
        memory.merge(std::slice::from_ref(column));
    }
    memory.sections
}

#[test]
fn neighbouring_columns_give_the_same_sections_in_either_order() {
    // Chunks 0 and 1 share level-0 sections; chunks 1 and 2 meet across a section face.
    for (a, b) in [((0, 0), (1, 0)), ((1, 0), (2, 0)), ((3, 1), (3, 2))] {
        let first = column(a.0, a.1);
        let second = column(b.0, b.1);
        let forward = one_at_a_time(&[first.clone(), second.clone()]);
        let backward = one_at_a_time(&[second.clone(), first.clone()]);
        let mut together = Memory::default();
        together.merge(&[first, second]);
        assert_eq!(forward, backward, "{a:?} then {b:?} against the reverse");
        assert_eq!(forward, together.sections, "{a:?} and {b:?} merged at once");
    }
}

#[test]
fn a_merge_writes_every_local_level_and_records_child_states() {
    let mut memory = Memory::default();
    let stats = memory.merge(&[column(5, -3)]);
    assert_eq!(stats.sub_chunks, 8);
    assert_eq!(stats.sections, memory.sections.len() as u32);
    for level in 0..=LOCAL_MIP_TOP_LEVEL {
        assert!(
            memory.sections.keys().any(|key| key.pos().level() == level),
            "level {level} is written"
        );
    }
    assert!(
        memory
            .sections
            .keys()
            .all(|key| key.pos().level() <= LOCAL_MIP_TOP_LEVEL),
        "coarser levels are left to the full re-mip"
    );
    let base = SectionPos::of_sub_chunk(5, 2, -3).expect("pos").0;
    let parent = base.parent().expect("level 1");
    assert_eq!(
        memory.sections[&parent.key()]
            .children()
            .get(base.octant_in_parent()),
        ChildState::NonEmpty,
        "the terrain section is a known, non-empty child"
    );
    let sibling = base
        .neighbour(Face::PosX)
        .expect("representable")
        .octant_in_parent();
    assert_eq!(
        memory.sections[&parent.key()].children().get(sibling),
        ChildState::Unknown,
        "unsampled columns stay unknown"
    );
}

#[test]
fn landing_next_to_a_stored_column_re_mips_its_facing_slab() {
    // Chunks 3 and 4 lie in different level-1 sections, so only the slab re-mip can
    // rewrite chunk 3's level-1 section when chunk 4 lands.
    let mut memory = Memory::default();
    memory.merge(&[column(3, 0)]);
    let stored_parent = SectionPos::of_sub_chunk(3, 2, 0)
        .expect("pos")
        .0
        .parent()
        .expect("level 1");
    let classes = fixture_classifier();
    let Ok(merged) = merge_columns(&[column(4, 0)], &mut memory, &classes);
    assert!(merged.stats.neighbour_slabs > 0);
    assert!(
        merged
            .sections
            .iter()
            .any(|section| section.pos() == stored_parent),
        "the stored neighbour's parent is mipped again"
    );
}

#[test]
fn merging_an_unchanged_column_again_rewrites_nothing_new() {
    let mut memory = Memory::default();
    let sample = column(-2, 4);
    memory.merge(std::slice::from_ref(&sample));
    let before = memory.sections.clone();
    memory.merge(std::slice::from_ref(&sample));
    assert_eq!(memory.sections, before);
}

#[test]
fn a_failing_source_writes_nothing() {
    struct Failing;
    impl SectionSource for Failing {
        type Error = &'static str;
        fn load(&mut self, _: SectionPos) -> Result<Option<Section>, &'static str> {
            Err("unreadable")
        }
    }
    let classes = fixture_classifier();
    assert_eq!(
        merge_columns(&[column(0, 0)], &mut Failing, &classes),
        Err("unreadable")
    );
}

/// Merges `columns` as writers that each own one [`WriterRegion`] would, one column at a
/// time in order, every writer merging the slabs handed to it before its next column, until
/// no work is left.
fn by_region_writers(columns: &[(i32, i32)]) -> HashMap<SectionKey, Section> {
    let classes = fixture_classifier();
    let mut memory = Memory::default();
    let mut handed: HashMap<WriterRegion, Vec<SlabWork>> = HashMap::new();
    let merge = |memory: &mut Memory,
                 region: WriterRegion,
                 samples: &[ColumnSample],
                 slabs: Vec<SlabWork>,
                 handed: &mut HashMap<WriterRegion, Vec<SlabWork>>| {
        let owns = |pos: SectionPos| WriterRegion::of(pos) == Some(region);
        let Ok(merged) = merge_owned(samples, slabs, owns, memory, &classes);
        for section in merged.sections {
            assert!(owns(section.pos()), "a writer writes its own sections only");
            memory.sections.insert(section.pos().key(), section);
        }
        for slab in merged.foreign {
            let owner = WriterRegion::of(slab.pos).expect("local level");
            assert_ne!(owner, region);
            handed.entry(owner).or_default().push(slab);
        }
    };
    for &(x, z) in columns {
        let region = WriterRegion::of_chunk(x, z);
        merge(
            &mut memory,
            region,
            &[column(x, z)],
            Vec::new(),
            &mut handed,
        );
        while let Some(owner) = handed.keys().next().copied() {
            let slabs = handed.remove(&owner).expect("listed");
            merge(&mut memory, owner, &[], slabs, &mut handed);
        }
    }
    memory.sections
}

#[test]
fn region_writers_handing_over_border_slabs_store_what_one_writer_stores() {
    // Chunks 31 and 32 meet across a writer region border at every local level; chunks
    // (31, 31) and (32, 32) meet only at a corner.
    for pair in [[(31, 0), (32, 0)], [(0, 31), (0, 32)], [(31, 31), (32, 32)]] {
        assert_ne!(
            WriterRegion::of_chunk(pair[0].0, pair[0].1),
            WriterRegion::of_chunk(pair[1].0, pair[1].1)
        );
        let samples = pair.map(|(x, z)| column(x, z));
        let single = one_at_a_time(&samples);
        let reversed = [pair[1], pair[0]];
        assert_eq!(by_region_writers(&pair), single, "{pair:?}");
        assert_eq!(by_region_writers(&reversed), single, "{reversed:?}");
    }
}

#[test]
fn writer_regions_hold_a_columns_whole_local_chain() {
    for (x, z) in [(0, 0), (31, -1), (-32, 63), (-1, -33)] {
        let region = WriterRegion::of_chunk(x, z);
        let base = SectionPos::of_sub_chunk(x, 5, z).expect("pos").0;
        for level in 0..=LOCAL_MIP_TOP_LEVEL {
            let pos = base.ancestor(level).expect("local level");
            assert_eq!(
                WriterRegion::of(pos),
                Some(region),
                "({x}, {z}) level {level}"
            );
        }
        assert_eq!(
            WriterRegion::of(base.ancestor(LOCAL_MIP_TOP_LEVEL + 1).expect("level 5")),
            None
        );
    }
    assert_ne!(WriterRegion::of_chunk(31, 0), WriterRegion::of_chunk(32, 0));
    assert_eq!(
        WriterRegion::of_chunk(-32, 0),
        WriterRegion::of_chunk(-1, 0)
    );
}
