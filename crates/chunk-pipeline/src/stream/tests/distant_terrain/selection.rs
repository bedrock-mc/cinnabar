//! Distant terrain selection: walks read the stored tree, refine over resident columns to
//! level 0, want meshes from the mesher and publish what to draw, one update at a time.

use std::time::Duration;

use lod::{
    geometry::SectionKey,
    select::{focal_px, shown::DrawUpdate},
};

use super::*;
use crate::DistantTerrainView;

/// Starts meshing and selection of `store` for `stream`.
fn enable_selection(stream: &mut WorldStream, store: &TestStore) {
    stream
        .enable_distant_terrain_meshes(store.client(), NOW)
        .expect("the mesher thread starts");
    stream
        .enable_distant_terrain_selection()
        .expect("the selection thread starts");
}

/// A medium-quality view from `camera` reaching 32 chunks.
fn view(camera: [f64; 3]) -> DistantTerrainView {
    DistantTerrainView {
        camera,
        focal_px: focal_px(1080.0, 70_f64.to_radians()),
        quality: render_api::DistantTerrainQuality::Medium,
        distance_chunks: 32,
    }
}

/// What a renderer applying every update so far draws: section keys with their levels.
#[derive(Default)]
struct Drawn {
    sections: BTreeSet<SectionKey>,
    updates: Vec<DrawUpdate>,
}

impl Drawn {
    fn apply(&mut self, update: DrawUpdate) {
        for key in &update.removed {
            assert!(self.sections.remove(key), "removed {key:?} was drawn");
        }
        for section in &update.upserts {
            assert!(!section.mesh.is_empty(), "drawn sections have quads");
            assert!(section.mesh.biomes().is_some());
            self.sections.insert(section.pos.key());
        }
        let shown: BTreeSet<SectionKey> = update.shown.iter().map(|s| s.key).collect();
        assert_eq!(shown, self.sections, "the full set matches the changes");
        self.updates.push(update);
    }
}

/// Hands `stream` the view from `camera` every simulated 50 ms and applies updates until
/// exactly `expected` is drawn.
fn draw_until(
    stream: &mut WorldStream,
    drawn: &mut Drawn,
    uptime: &mut Duration,
    camera: [f64; 3],
    expected: &[SectionPos],
) {
    let expected: BTreeSet<SectionKey> = expected.iter().map(|pos| pos.key()).collect();
    eventually("the expected sections to be drawn", || {
        *uptime += Duration::from_millis(50);
        stream.sync_distant_terrain_meshes();
        stream.set_distant_terrain_view(view(camera), *uptime);
        while let Some(update) = stream.take_distant_terrain_update() {
            drawn.apply(update);
        }
        drawn.sections == expected
    });
}

#[test]
fn a_stored_section_beside_a_resident_column_is_drawn_at_level_zero() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    enable_selection(&mut stream, &store);
    ingest_one(&mut stream);
    // The level-0 section spans chunks 0 and 1 on X and Z; only chunk (1, 0) is resident,
    // so it overlaps resident columns without being covered: drawn at level 0.
    let mut drawn = Drawn::default();
    let mut uptime = Duration::ZERO;
    let camera = [24.0, -40.0, 8.0];
    draw_until(&mut stream, &mut drawn, &mut uptime, camera, &[chain(1)[0]]);
    let stats = stream
        .distant_terrain_selection_stats()
        .expect("selection on");
    assert!(stats.walks >= 1 && stats.header_reads >= 1);
    assert_eq!(stats.shown.drawn, 1);
    let meshes = stream.distant_terrain_mesh_stats().expect("meshing on");
    assert!(meshes.cache.wanted >= 1, "selection wants what it draws");
}

#[test]
fn evicting_the_resident_column_lets_a_far_camera_draw_the_coarse_root() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    enable_selection(&mut stream, &store);
    ingest_one(&mut stream);
    // High above, the measure alone keeps the root whole, but the resident column forces
    // level 0 under it.
    let camera = [24.0, 6_000.0, 8.0];
    let mut drawn = Drawn::default();
    let mut uptime = Duration::ZERO;
    draw_until(&mut stream, &mut drawn, &mut uptime, camera, &[chain(1)[0]]);
    stream.evict_column(ChunkKey::new(0, 1, 0));
    let root = *chain(1).last().expect("root");
    draw_until(&mut stream, &mut drawn, &mut uptime, camera, &[root]);
    let swap = drawn
        .updates
        .iter()
        .find(|update| update.upserts.iter().any(|section| section.pos == root))
        .expect("the root was upserted");
    assert_eq!(
        swap.removed,
        vec![chain(1)[0].key()],
        "the level-0 section went in the same update, leaving no hole"
    );
}

#[test]
fn selection_needs_meshing_and_stops_with_it() {
    let store = TestStore::new();
    let mut stream = stream();
    assert!(stream.enable_distant_terrain_selection().is_err());
    enable_selection(&mut stream, &store);
    assert!(stream.distant_terrain_selection_stats().is_some());
    stream.disable_distant_terrain_meshes();
    assert!(stream.distant_terrain_selection_stats().is_none());
    assert!(stream.take_distant_terrain_update().is_none());
}

#[test]
fn a_stopped_store_stops_meshing_with_selection() {
    let TestStore { actor, _dir } = TestStore::new();
    let mut stream = stream();
    stream
        .enable_distant_terrain_meshes(actor.client(), NOW)
        .expect("the mesher thread starts");
    stream
        .enable_distant_terrain_selection()
        .expect("the selection thread starts");
    actor.shutdown(NOW);
    let mut uptime = Duration::ZERO;
    eventually("selection and meshing to stop with the store", || {
        uptime += Duration::from_millis(50);
        stream.set_distant_terrain_view(view([0.0, 64.0, 0.0]), uptime);
        let selection = stream.distant_terrain_selection_stats();
        let meshes = stream.distant_terrain_mesh_stats();
        selection.is_some_and(|stats| stats.stopped) && meshes.is_some_and(|stats| stats.stopped)
    });
}

/// Ingests chunk 1 as one stone sub-chunk and waits for its merge.
fn ingest_one(stream: &mut WorldStream) {
    commit_column(stream, 1, 1, STONE, 15);
    settle(stream);
    wait_for_merges(stream, 1);
}

#[test]
fn a_new_selection_releases_what_the_old_one_wanted() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    enable_selection(&mut stream, &store);
    ingest_one(&mut stream);
    let mut drawn = Drawn::default();
    let mut uptime = Duration::ZERO;
    draw_until(
        &mut stream,
        &mut drawn,
        &mut uptime,
        [24.0, -40.0, 8.0],
        &[chain(1)[0]],
    );
    stream
        .enable_distant_terrain_selection()
        .expect("the selection thread starts");
    eventually("the old selection's wants to be released", || {
        stream.sync_distant_terrain_meshes();
        stream
            .distant_terrain_mesh_stats()
            .is_some_and(|stats| stats.cache.wanted == 0)
    });
}
