//! Distant terrain meshes: wanted sections are meshed from the store, kept meshed as they
//! and their neighbours are written or what the store shows changes, and the colour tables
//! follow the store's materials.

use std::{collections::HashMap, sync::Arc};

use lod::{
    epoch::{ChunkSurface, SurfaceColumn},
    mesh::{cache::FreshMesh, output::SectionMesh},
    section::ChildMask,
    storage::{DenseSection, PackScratch},
    voxel::{FULL_COVERAGE, Light, MaterialId, Voxel},
};
use lod_store::store::Store;

use super::*;

/// Starts meshing `store` for `stream`.
fn enable_meshes(stream: &mut WorldStream, store: &TestStore) {
    stream
        .enable_distant_terrain_meshes(store.client(), NOW)
        .expect("the mesher thread starts");
}

/// Polls the mesher until a mesh of `pos` is handed out, and returns it; `None` when the
/// store holds no section there.
fn next_mesh(stream: &mut WorldStream, pos: SectionPos) -> Option<Arc<SectionMesh>> {
    let mut found: Option<FreshMesh> = None;
    eventually("a fresh mesh", || {
        stream.sync_distant_terrain_meshes();
        let mut fresh = Vec::new();
        stream.take_distant_terrain_meshes(&mut fresh);
        found = found
            .take()
            .or_else(|| fresh.into_iter().rev().find(|(key, _)| *key == pos.key()));
        found.is_some()
    });
    found.expect("found above").1
}

/// Ingests chunk `x` as one stone sub-chunk and waits for `merged` merges in total.
fn ingest(stream: &mut WorldStream, sequence: u64, x: i32, block: u32, merged: u64) {
    commit_column(stream, sequence, x, block, 15);
    settle(stream);
    wait_for_merges(stream, merged);
}

#[test]
fn a_wanted_section_is_meshed_from_the_store_and_again_when_written() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    enable_meshes(&mut stream, &store);
    ingest(&mut stream, 1, 1, STONE, 1);
    let section = chain(1)[0];
    stream.want_distant_terrain_meshes([section]);
    let first = next_mesh(&mut stream, section).expect("a stored section");
    assert!(!first.is_empty(), "stone shows faces against unknown space");

    // Water replaces the stone: the writer's store meshes the section again.
    ingest(&mut stream, 2, 1, WATER_ID, 2);
    let second = next_mesh(&mut stream, section).expect("still stored");
    assert_ne!(first, second);
    let stats = stream.distant_terrain_mesh_stats().expect("meshing on");
    assert!(stats.cache.jobs >= 2);
    assert_eq!(stats.cache.wanted, 1);
}

#[test]
fn a_neighbour_arriving_meshes_the_section_reading_it_again() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    enable_meshes(&mut stream, &store);
    ingest(&mut stream, 1, 1, STONE, 1);
    let section = chain(1)[0];
    stream.want_distant_terrain_meshes([section]);
    let alone = next_mesh(&mut stream, section).expect("stored");
    assert!(!alone.waiting().is_empty(), "the neighbours are unknown");

    // Chunk 2 lies in the level-0 section across chunk 1's +X face.
    ingest(&mut stream, 2, 2, STONE, 2);
    let beside = next_mesh(&mut stream, section).expect("stored");
    assert!(
        beside.quads().len() < alone.quads().len(),
        "the shared face is hidden now"
    );
}

#[test]
fn a_section_without_a_record_completes_without_a_mesh() {
    let store = TestStore::new();
    let mut stream = stream();
    enable_meshes(&mut stream, &store);
    let empty = SectionPos::new(0, 40, 0, 40).expect("pos");
    stream.want_distant_terrain_meshes([empty]);
    assert!(next_mesh(&mut stream, empty).is_none());
}

#[test]
fn a_stopped_taker_leaves_finished_meshes_for_the_next() {
    let store = TestStore::new();
    let mut stream = stream();
    enable_meshes(&mut stream, &store);
    let empty = SectionPos::new(0, 40, 0, 40).expect("pos");
    stream.want_distant_terrain_meshes([empty]);
    eventually("a finished mesh", || {
        stream.sync_distant_terrain_meshes();
        stream
            .distant_terrain_mesh_stats()
            .is_some_and(|stats| stats.ready != 0)
    });
    let shared = stream.lod_mesh_shared().expect("meshing on");
    let dimension = lod_store::identity::Dimension(stream.current_dimension());
    let mut stolen = Vec::new();
    shared.take_fresh(|| false, dimension, &mut stolen);
    assert!(stolen.is_empty(), "a stopped selection takes nothing");
    assert!(
        next_mesh(&mut stream, empty).is_none(),
        "the mesh is still handed out"
    );
}

#[test]
fn colour_tables_cover_the_stores_materials() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    enable_meshes(&mut stream, &store);
    ingest(&mut stream, 1, 1, STONE, 1);
    let materials = store.client().registry().read().expect("registry").len();
    assert!(materials > 0);
    eventually("the colour tables", || {
        stream.sync_distant_terrain_meshes();
        // Any written section wakes the mesher, which extends its tables.
        stream.want_distant_terrain_meshes([chain(1)[0]]);
        stream
            .distant_terrain_colours()
            .is_some_and(|colours| colours.materials.len() > materials && colours.biomes.is_some())
    });
}

#[test]
fn turning_meshing_off_drops_its_state() {
    let store = TestStore::new();
    let mut stream = stream();
    enable_meshes(&mut stream, &store);
    assert!(stream.distant_terrain_mesh_stats().is_some());
    stream.disable_distant_terrain_meshes();
    assert!(stream.distant_terrain_mesh_stats().is_none());
    let mut fresh = Vec::new();
    stream.take_distant_terrain_meshes(&mut fresh);
    assert!(fresh.is_empty());
}

#[test]
fn a_section_wanted_then_released_before_the_mesher_looks_stays_released() {
    let mut store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    enable_meshes(&mut stream, &store);
    ingest(&mut stream, 1, 1, STONE, 1);
    let section = chain(1)[0];
    let released = SectionPos::new(0, 40, 0, 40).expect("pos");
    {
        // The mesher waits for the store while reading the first section's snapshot, so
        // the want and release that follow reach it together.
        let _paused = store.actor.pause();
        stream.want_distant_terrain_meshes([section]);
        stream.want_distant_terrain_meshes([released]);
        stream.release_distant_terrain_meshes([released.key()]);
    }
    next_mesh(&mut stream, section).expect("stored");
    let stats = stream.distant_terrain_mesh_stats().expect("meshing on");
    assert_eq!(stats.cache.wanted, 1, "only the section still wanted");
}

#[test]
fn meshing_reports_a_stopped_store_and_stops_queueing_interest() {
    let TestStore { actor, _dir } = TestStore::new();
    let client = actor.client();
    let mut stream = stream();
    stream
        .enable_distant_terrain_meshes(client, NOW)
        .expect("the mesher thread starts");
    actor.shutdown(NOW);
    let section = SectionPos::new(0, 1, 0, 1).expect("pos");
    eventually("meshing stops with the store", || {
        stream.sync_distant_terrain_meshes();
        stream.want_distant_terrain_meshes([section]);
        stream
            .distant_terrain_mesh_stats()
            .is_some_and(|stats| stats.stopped)
    });
    let shared = stream.lod_mesh_shared().expect("meshing still installed");
    let queued = shared.queued_interest();
    for _ in 0..4 {
        stream.want_distant_terrain_meshes([section]);
        stream.release_distant_terrain_meshes([section.key()]);
    }
    assert_eq!(shared.queued_interest(), queued, "nothing drains the inbox");
}

/// World `seed`'s surface over chunk `(chunk_x, chunk_z)`: varied heights and materials,
/// so signature regions vote.
fn world_surface(seed: u32, chunk_x: i32, chunk_z: i32) -> ChunkSurface {
    let mut surface = ChunkSurface::default();
    for z in 0..16 {
        for x in 0..16 {
            let (bx, bz) = (chunk_x * 16 + x as i32, chunk_z * 16 + z as i32);
            let hash = (seed ^ (bx as u32).wrapping_mul(0x9e37_79b9)).wrapping_mul(0x85eb_ca6b)
                ^ (bz as u32).wrapping_mul(0xc2b2_ae35);
            let hash = hash ^ hash >> 15;
            let column = SurfaceColumn {
                top_y: 60 + (hash % 16) as i32,
                material: MaterialId::new(1 + hash / 16 % 8).expect("block material"),
            };
            surface.set(x, z, Some(column.signature()));
        }
    }
    surface
}

/// The chunks of signature region `(region, 0)`, four chunks a side.
fn region_chunks(region: i32) -> impl Iterator<Item = (i32, i32)> {
    (0..16).map(move |chunk| (region * 4 + chunk % 4, chunk / 4))
}

/// Signature regions of the stored world, enough to vote.
const STORED_REGIONS: i32 = 12;

/// A store holding world 1 over regions `(0..STORED_REGIONS, 0)` with a solid record at
/// each of `records`, reopened with an undecided overworld session that reads them through.
fn stored_world(records: &[SectionPos]) -> TestStore {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = StoreRoot::new(dir.path());
    let mut store: Store = root
        .open(&store_key(), NOW, StoreOptions::default())
        .expect("open store");
    store.begin_session(OVERWORLD, NOW).expect("begin");
    for region in 0..STORED_REGIONS {
        for (x, z) in region_chunks(region) {
            store
                .record_surface(OVERWORLD, x, z, world_surface(1, x, z), NOW)
                .expect("record surface");
        }
    }
    let solid = Voxel::new(MaterialId::FIRST_BLOCK, Light::DARK, FULL_COVERAGE, false);
    let (mut codec, mut scratch) = (SectionCodec::new(), PackScratch::default());
    for &pos in records {
        let section = Section::from_dense(
            pos,
            &DenseSection::filled(solid, 0),
            ChildMask::UNKNOWN,
            &mut scratch,
        )
        .expect("level-0 section");
        store
            .put(OVERWORLD, &codec.encode(&section), NOW)
            .expect("put");
    }
    assert!(store.flush());
    store.close();
    let reopened = TestStore::in_dir(dir, ActorConfig::default());
    assert!(matches!(
        reopened
            .client()
            .read(OVERWORLD, ReadTarget::Section(records[0].key()), NOW),
        Ok(ReadOutcome::Found(_))
    ));
    reopened
}

/// Polls the mesher until it took in `wanted` wanted sections and no job is queued or
/// running, collecting the meshes handed out meanwhile by section, the latest winning.
fn settled_meshes(
    stream: &mut WorldStream,
    wanted: usize,
) -> HashMap<SectionKey, Option<Arc<SectionMesh>>> {
    let mut meshes = HashMap::new();
    eventually("the mesher to settle", || {
        stream.sync_distant_terrain_meshes();
        let mut fresh = Vec::new();
        stream.take_distant_terrain_meshes(&mut fresh);
        meshes.extend(fresh);
        let stats = stream.distant_terrain_mesh_stats().expect("meshing on");
        stats.cache.wanted == wanted
            && stats.cache.dirty == 0
            && stats.cache.in_flight == 0
            && stats.ready == 0
    });
    meshes
}

/// Wants `sections`, waits until each has been meshed and returns the meshes.
fn mesh_all(
    stream: &mut WorldStream,
    sections: &[SectionPos],
) -> HashMap<SectionKey, Option<Arc<SectionMesh>>> {
    stream.want_distant_terrain_meshes(sections.iter().copied());
    let mut meshes = HashMap::new();
    eventually("every wanted mesh", || {
        meshes.extend(settled_meshes(stream, sections.len()));
        sections.iter().all(|pos| meshes.contains_key(&pos.key()))
    });
    meshes
}

/// Mesh jobs started so far.
fn jobs(stream: &WorldStream) -> u64 {
    stream
        .distant_terrain_mesh_stats()
        .expect("meshing on")
        .cache
        .jobs
}

/// Level-0 sections at Y 2: one in signature region 0, one in region 1 whose mesh reads
/// sections overlapping region 0 (its -X neighbour and the coarser section across), and
/// one far away in region 8.
fn hidden_fixture() -> [SectionPos; 3] {
    [0, 2, 16].map(|x| SectionPos::new(0, x, 2, 0).expect("pos"))
}

#[test]
fn a_region_hidden_from_read_through_meshes_again_only_what_overlaps_or_reads_it() {
    let sections = hidden_fixture();
    let [inside, reader, far] = sections;
    let mut store = stored_world(&sections);
    let mut stream = stream();
    enable_meshes(&mut stream, &store);
    let first = mesh_all(&mut stream, &sections);
    assert!(
        sections
            .iter()
            .all(|pos| matches!(first.get(&pos.key()), Some(Some(_)))),
        "read through to the stored epoch"
    );
    let before = jobs(&stream);

    // Another world in region 0: its vote mismatches and hides the region at once.
    for (x, z) in region_chunks(0) {
        store
            .actor
            .try_submit(Request::Surface {
                dimension: OVERWORLD,
                chunk_x: x,
                chunk_z: z,
                surface: Box::new(world_surface(2, x, z)),
                now: NOW,
            })
            .expect("queued");
    }
    let mut again = HashMap::new();
    eventually("the hidden region's meshes", || {
        again.extend(settled_meshes(&mut stream, sections.len()));
        again.contains_key(&inside.key()) && again.contains_key(&reader.key())
    });
    assert!(again[&inside.key()].is_none(), "its record no longer shows");
    assert!(again[&reader.key()].is_some());
    assert!(
        !again.contains_key(&far.key()),
        "untouched sections keep their meshes"
    );
    assert_eq!(jobs(&stream), before + 2);
}

#[test]
fn a_session_change_meshes_every_wanted_section_again() {
    let sections = hidden_fixture();
    let mut store = stored_world(&sections);
    let mut stream = stream();
    enable_meshes(&mut stream, &store);
    mesh_all(&mut stream, &sections);
    let before = jobs(&stream);
    store
        .actor
        .try_submit(Request::Begin {
            dimension: OVERWORLD,
            now: NOW,
        })
        .expect("queued");
    let mut again = HashMap::new();
    eventually("every mesh again", || {
        again.extend(settled_meshes(&mut stream, sections.len()));
        sections.iter().all(|pos| again.contains_key(&pos.key()))
    });
    assert!(
        sections.iter().all(|pos| again[&pos.key()].is_some()),
        "the new session reads them through too"
    );
    assert_eq!(jobs(&stream), before + sections.len() as u64);
}

#[test]
fn wanting_another_dimension_drops_every_mesh() {
    let sections = hidden_fixture();
    let mut store = stored_world(&sections);
    let mut stream = stream();
    enable_meshes(&mut stream, &store);
    mesh_all(&mut stream, &sections);
    let stats = stream.distant_terrain_mesh_stats().expect("meshing on");
    assert_eq!(stats.cache.wanted, sections.len());
    assert!(stats.cache.held_bytes > 0);

    store
        .actor
        .try_submit(Request::Begin {
            dimension: Dimension(1),
            now: NOW,
        })
        .expect("queued");
    stream
        .submit(
            1,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0, 64.0, 0.0],
                ..Default::default()
            }),
        )
        .expect("change dimension");
    let meshes = mesh_all(&mut stream, &sections[..1]);
    assert!(
        matches!(meshes.get(&sections[0].key()), Some(None)),
        "the other dimension stores nothing there"
    );
    let stats = stream.distant_terrain_mesh_stats().expect("meshing on");
    assert_eq!(stats.cache.wanted, 1, "the overworld's wants are gone");
    assert_eq!(stats.cache.held_bytes, 0, "and so are their meshes");
}

#[test]
fn meshes_finished_before_a_dimension_change_are_not_handed_out_after_it() {
    let sections = hidden_fixture();
    let store = stored_world(&sections);
    let mut stream = stream();
    enable_meshes(&mut stream, &store);
    stream.want_distant_terrain_meshes(sections);
    eventually("overworld meshes waiting to be taken", || {
        stream.sync_distant_terrain_meshes();
        stream
            .distant_terrain_mesh_stats()
            .is_some_and(|stats| stats.ready > 0)
    });
    stream
        .submit(
            1,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0, 64.0, 0.0],
                ..Default::default()
            }),
        )
        .expect("change dimension");
    let mut fresh = Vec::new();
    stream.take_distant_terrain_meshes(&mut fresh);
    assert!(fresh.is_empty(), "{} overworld meshes leaked", fresh.len());
}
