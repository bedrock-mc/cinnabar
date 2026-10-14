use std::sync::Arc;

use lod::{
    draw::{
        NeighbourLevel, NeighbourLevels,
        records::{CLUSTER_RECORD_WORDS, ClusterRecord, SECTION_RECORD_WORDS},
    },
    geometry::{Face, SectionPos},
    select::shown::{DrawUpdate, DrawnSection, ShownSection},
};

use super::*;
use crate::distant_terrain::{
    layout::{Capacity, MAX_COMMANDS},
    staging::Staging,
    test_meshes::ground,
};

/// CPU copies of the scatter targets, indexed by [`Target`].
type Targets = [Vec<u32>; 5];

fn capacity(pool_bytes: u64) -> Capacity {
    let limits = wgpu::Limits {
        max_storage_buffer_binding_size: u64::from(u32::MAX),
        max_buffer_size: u64::MAX,
        ..wgpu::Limits::default()
    };
    Capacity::new(pool_bytes, &limits).expect("capacity")
}

fn pos(x: i32) -> SectionPos {
    SectionPos::new(0, x, 0, 0).expect("position")
}

fn drawn(at: SectionPos, height: usize) -> DrawnSection {
    DrawnSection {
        pos: at,
        mesh: ground(at, height, 0),
        neighbours: NeighbourLevels::NONE.with(Face::PosX, NeighbourLevel::Same),
    }
}

fn update(upserts: Vec<DrawnSection>, removed: Vec<SectionKey>) -> DrawUpdate {
    DrawUpdate {
        upserts,
        removed,
        relinked: Vec::new(),
        shown: Arc::from([]),
    }
}

/// Runs one frame: frees what the last commit freed, steps and applies the staged writes.
fn frame(table: &mut SectionTable, targets: &mut Targets, budget: u64) -> Step {
    let mut staging = Staging::new(table.capacity.staging_words());
    table.begin_frame();
    let step = table.step(&mut staging, budget);
    staging.apply(targets);
    step
}

fn words(targets: &Targets, target: Target, start: u32, len: usize) -> Vec<u32> {
    let words = &targets[target as usize];
    (0..len)
        .map(|index| words.get(start as usize + index).copied().unwrap_or(0))
        .collect()
}

/// Asserts that `key`'s records on the "GPU" match its mesh.
fn assert_uploaded(table: &SectionTable, targets: &Targets, section: &DrawnSection) {
    let (slot, record, clusters) = table.record(section.pos.key()).expect("drawn");
    assert_eq!(
        words(
            targets,
            Target::Sections,
            slot * SECTION_RECORD_WORDS as u32,
            SECTION_RECORD_WORDS
        ),
        record.words()
    );
    assert_eq!(record.pos(), Some(section.pos));
    let biome_words = words(targets, Target::Pool, record.biomes() * 2, 512);
    assert_eq!(&biome_words[..], section.mesh.biomes().expect("biomes"));
    let mesh = &section.mesh;
    assert_eq!(clusters.len(), mesh.clusters().len());
    for (index, cluster) in mesh.clusters().iter().enumerate() {
        let at = (clusters.start + index as u32) * CLUSTER_RECORD_WORDS as u32;
        let stored = words(targets, Target::Clusters, at, CLUSTER_RECORD_WORDS);
        let quad_base = record.biomes() + BIOME_UNITS;
        let expected = ClusterRecord::new(cluster, slot, quad_base).expect("record");
        assert_eq!(&stored[..], expected.words());
        for quad in 0..cluster.count {
            let pool = words(targets, Target::Pool, (expected.first_quad() + quad) * 2, 2);
            let source = mesh.quads()[(cluster.first + quad) as usize];
            assert_eq!(pool, [source.geometry(), source.payload()]);
        }
        let history = targets[Target::Flags as usize]
            .get((table.capacity.history_word() + clusters.start + index as u32) as usize);
        assert_eq!(history.copied().unwrap_or(0), 0);
    }
}

fn live(targets: &Targets, slot: u32) -> bool {
    let info = words(
        targets,
        Target::Sections,
        slot * SECTION_RECORD_WORDS as u32 + lod::draw::records::SECTION_INFO_WORD as u32,
        1,
    )[0];
    info != 0
}

#[test]
fn an_update_shows_only_once_every_mesh_is_uploaded() {
    let mut table = SectionTable::new(capacity(64 << 20));
    let mut targets = Targets::default();
    let sections = [drawn(pos(0), 4), drawn(pos(1), 6), drawn(pos(2), 8)];
    table.start(update(sections.to_vec(), Vec::new()));
    // One byte a frame still uploads one section a frame.
    for uploaded in 1..sections.len() {
        let step = frame(&mut table, &mut targets, 1);
        assert!(!step.committed, "after {uploaded} sections");
        assert!(step.uploaded_bytes > 0);
        assert!(table.is_applying());
        assert_eq!(table.sections(), 0, "nothing drawn before the commit");
        for slot in 0..8 {
            assert!(!live(&targets, slot), "slot {slot} stays dead");
        }
    }
    let step = frame(&mut table, &mut targets, 1);
    assert!(step.committed);
    assert_eq!(table.sections(), 3);
    for section in &sections {
        assert_uploaded(&table, &targets, section);
    }
    assert!(table.pool_used_bytes() > 0);
    assert_eq!(
        table.clusters_used() as usize,
        sections
            .iter()
            .map(|s| s.mesh.clusters().len())
            .sum::<usize>()
    );
}

#[test]
fn removals_and_replacements_die_at_the_commit_and_free_space_a_frame_later() {
    let mut table = SectionTable::new(capacity(64 << 20));
    let mut targets = Targets::default();
    let first = [drawn(pos(0), 4), drawn(pos(1), 6)];
    table.start(update(first.to_vec(), Vec::new()));
    assert!(frame(&mut table, &mut targets, u64::MAX).committed);
    let (removed_slot, _, removed_clusters) = table.record(pos(0).key()).expect("drawn");
    let (replaced_slot, _, replaced_clusters) = table.record(pos(1).key()).expect("drawn");

    let replacement = drawn(pos(1), 12);
    let added = drawn(pos(5), 3);
    table.start(update(
        vec![replacement.clone(), added.clone()],
        vec![pos(0).key()],
    ));
    assert!(frame(&mut table, &mut targets, u64::MAX).committed);
    assert!(!live(&targets, removed_slot));
    assert!(!live(&targets, replaced_slot));
    for cluster in removed_clusters.clone().chain(replaced_clusters.clone()) {
        let record = words(&targets, Target::Clusters, cluster * 4, 4);
        assert_eq!(record, [0; 4], "a dead cluster names dead slot 0");
    }
    assert_uploaded(&table, &targets, &replacement);
    assert_uploaded(&table, &targets, &added);
    let (new_slot, _, new_clusters) = table.record(pos(1).key()).expect("drawn");
    assert_ne!(new_slot, replaced_slot, "the replacement had its own slot");
    assert!(
        new_clusters.end <= removed_clusters.start || new_clusters.start >= replaced_clusters.end,
        "space freed in a frame is not reused in it"
    );

    // The next frame may reuse what the commit freed.
    let reused = drawn(pos(9), 4);
    table.start(update(vec![reused.clone()], Vec::new()));
    assert!(frame(&mut table, &mut targets, u64::MAX).committed);
    let (slot, _, _) = table.record(pos(9).key()).expect("drawn");
    assert!(slot == removed_slot || slot == replaced_slot, "slot {slot}");
    assert_uploaded(&table, &targets, &reused);
}

#[test]
fn relinks_rewrite_neighbour_levels_and_full_tables_drop_sections() {
    let mut table = SectionTable::new(capacity(64 << 20));
    let mut targets = Targets::default();
    table.start(update(vec![drawn(pos(0), 4)], Vec::new()));
    assert!(frame(&mut table, &mut targets, u64::MAX).committed);
    let neighbours = NeighbourLevels::NONE.with(Face::NegZ, NeighbourLevel::Finer);
    table.start(DrawUpdate {
        upserts: Vec::new(),
        removed: Vec::new(),
        relinked: vec![ShownSection {
            key: pos(0).key(),
            neighbours,
        }],
        shown: Arc::from([]),
    });
    assert!(frame(&mut table, &mut targets, u64::MAX).committed);
    let (slot, record, _) = table.record(pos(0).key()).expect("drawn");
    assert_eq!(record.neighbours(), neighbours);
    assert_eq!(
        words(
            &targets,
            Target::Sections,
            slot * SECTION_RECORD_WORDS as u32,
            SECTION_RECORD_WORDS
        ),
        record.words()
    );

    // A pool that holds one ground section's biome grid and quads but not two.
    let one = BIOME_UNITS + drawn(pos(0), 4).mesh.quads().len() as u32;
    let mut small = SectionTable::new(Capacity {
        pool_units: one * 3 / 2,
        clusters: 1024,
    });
    let mut targets = Targets::default();
    small.start(update(vec![drawn(pos(0), 4), drawn(pos(1), 4)], Vec::new()));
    assert!(frame(&mut small, &mut targets, u64::MAX).committed);
    assert_eq!(small.sections(), 1);
    assert_eq!(small.dropped(), 1);
}

#[test]
fn translucent_sections_are_listed_and_changes_reported() {
    let mut table = SectionTable::new(capacity(64 << 20));
    let mut targets = Targets::default();
    let lake = DrawnSection {
        pos: pos(3),
        mesh: ground(pos(3), 4, 8),
        neighbours: NeighbourLevels::NONE,
    };
    table.start(update(vec![lake, drawn(pos(4), 4)], Vec::new()));
    assert!(frame(&mut table, &mut targets, u64::MAX).committed);
    assert!(table.take_translucent_changed());
    assert!(!table.take_translucent_changed());
    let listed: Vec<_> = table.translucent().collect();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].pos, pos(3));
    assert!(!listed[0].clusters.is_empty());
}

#[test]
fn removing_more_sections_than_one_frame_has_commands_for_still_commits() {
    let mut table = SectionTable::new(capacity(128 << 20));
    let mut targets = Targets::default();
    let mesh = ground(pos(0), 4, 0);
    let count = MAX_COMMANDS as i32 + 100;
    let sections: Vec<DrawnSection> = (0..count)
        .map(|index| DrawnSection {
            pos: SectionPos::new(0, index % 256, 0, index / 256).expect("position"),
            mesh: Arc::clone(&mesh),
            neighbours: NeighbourLevels::NONE,
        })
        .collect();
    let keys: Vec<SectionKey> = sections.iter().map(|section| section.pos.key()).collect();
    table.start(update(sections, Vec::new()));
    let mut frames = 0;
    while !frame(&mut table, &mut targets, u64::MAX).committed {
        frames += 1;
        assert!(frames < 64, "the uploads finish");
    }
    assert_eq!(table.sections(), count as u32);
    let slots = table.section_slots();

    table.start(update(Vec::new(), keys));
    assert!(
        frame(&mut table, &mut targets, u64::MAX).committed,
        "the removal commits in its first frame"
    );
    assert_eq!(table.sections(), 0);
    assert!((0..slots).all(|slot| !live(&targets, slot)));
    let mut frames = 0;
    while table.pool_used_bytes() != 0 {
        frame(&mut table, &mut targets, u64::MAX);
        frames += 1;
        assert!(frames < 8, "dead sections are zeroed and freed");
    }
    assert!(
        targets[Target::Clusters as usize]
            .iter()
            .all(|&word| word == 0)
    );
    assert_eq!(table.clusters_used(), 0);
}

#[test]
fn a_section_that_finds_the_pool_full_is_uploaded_once_space_is_freed() {
    // A pool that holds one ground section's biome grid and quads but not two.
    let one = BIOME_UNITS + drawn(pos(0), 4).mesh.quads().len() as u32;
    let mut table = SectionTable::new(Capacity {
        pool_units: one * 3 / 2,
        clusters: 1024,
    });
    let mut targets = Targets::default();
    table.start(update(vec![drawn(pos(0), 4)], Vec::new()));
    assert!(frame(&mut table, &mut targets, u64::MAX).committed);

    // A refinement replaces the drawn section by another the pool holds only once the
    // first is freed: it commits without it and uploads it once the space is free.
    let child = drawn(pos(1), 4);
    table.start(update(vec![child.clone()], vec![pos(0).key()]));
    assert!(frame(&mut table, &mut targets, u64::MAX).committed);
    assert_eq!(table.sections(), 0);
    assert_eq!(table.retrying(), 1);
    let mut frames = 0;
    while table.record(pos(1).key()).is_none() {
        let step = frame(&mut table, &mut targets, u64::MAX);
        assert!(!step.committed, "a retry is not the frame's update");
        frames += 1;
        assert!(frames < 8, "the section is uploaded again");
    }
    assert_uploaded(&table, &targets, &child);
    assert_eq!(table.retrying(), 0);
    assert_eq!(table.dropped(), 1, "counted once");

    // A later update removing a section still waiting for space ends its retries.
    table.start(update(vec![drawn(pos(2), 4)], Vec::new()));
    assert!(frame(&mut table, &mut targets, u64::MAX).committed);
    assert_eq!(table.retrying(), 1);
    table.start(update(Vec::new(), vec![pos(2).key()]));
    assert!(frame(&mut table, &mut targets, u64::MAX).committed);
    assert_eq!(table.retrying(), 0);
    assert_eq!(table.sections(), 1);
}
