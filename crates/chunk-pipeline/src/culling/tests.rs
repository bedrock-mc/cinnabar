use std::collections::{HashSet, VecDeque};

use hashbrown::HashMap;

use meshing::{Face, FaceConnectivity};
use world::SubChunkKey;

use super::*;

/// Builds the same connectivity grid used by the retained traversal.
pub(super) fn grid(
    entries: impl IntoIterator<Item = (SubChunkKey, FaceConnectivity)>,
) -> ConnectivityGrid {
    entries.into_iter().collect()
}

#[test]
fn all_air_connectivity_walks_the_loaded_graph() {
    let first = SubChunkKey::new(0, 0, 0, 0);
    let second = SubChunkKey::new(0, 1, 0, 0);
    let third = SubChunkKey::new(0, 2, 0, 0);
    let graph = grid([
        (first, FaceConnectivity::all()),
        (second, FaceConnectivity::all()),
        (third, FaceConnectivity::all()),
    ]);

    let visible = cave_visible_sub_chunks(first, &graph);
    assert_eq!(visible, [first, second, third].into());
}

#[test]
fn sealed_middle_subchunk_stops_bfs_beyond_the_conservative_shell() {
    let first = SubChunkKey::new(0, 0, 0, 0);
    let sealed = SubChunkKey::new(0, 1, 0, 0);
    let shell = SubChunkKey::new(0, 2, 0, 0);
    let hidden = SubChunkKey::new(0, 3, 0, 0);
    let graph = grid([
        (first, FaceConnectivity::all()),
        (sealed, FaceConnectivity::none()),
        (shell, FaceConnectivity::all()),
        (hidden, FaceConnectivity::all()),
    ]);

    let visible = cave_visible_sub_chunks(first, &graph);
    assert_eq!(visible, [first, sealed, shell].into());
}

#[test]
fn visible_outdoor_node_keeps_its_loaded_support_shell_visible() {
    let camera = SubChunkKey::new(0, 0, 4, 0);
    let outdoor = SubChunkKey::new(0, 1, 4, 0);
    let support = SubChunkKey::new(0, 1, 3, 0);
    let deeper_interior = SubChunkKey::new(0, 1, 2, 0);
    let graph = grid([
        (camera, FaceConnectivity::all()),
        // The whole outdoor sub-chunk is rendered after the camera-side
        // portal reaches it, even when its downward region is disconnected.
        (outdoor, FaceConnectivity::none()),
        (support, FaceConnectivity::none()),
        (deeper_interior, FaceConnectivity::none()),
    ]);

    let visible = cave_visible_sub_chunks(camera, &graph);
    assert!(visible.contains(&outdoor));
    assert!(
        visible.contains(&support),
        "a rendered outdoor/model sub-chunk must not float over a hidden loaded support shell"
    );
    assert!(
        !visible.contains(&deeper_interior),
        "the conservative shell must stay one sub-chunk deep"
    );
}

#[test]
fn conservative_shell_adds_at_most_the_six_loaded_face_neighbours() {
    let camera = SubChunkKey::new(0, 0, 0, 0);
    let neighbours = [
        SubChunkKey::new(0, -1, 0, 0),
        SubChunkKey::new(0, 1, 0, 0),
        SubChunkKey::new(0, 0, -1, 0),
        SubChunkKey::new(0, 0, 1, 0),
        SubChunkKey::new(0, 0, 0, -1),
        SubChunkKey::new(0, 0, 0, 1),
    ];
    let second_ring = SubChunkKey::new(0, 2, 0, 0);
    let graph = grid(
        neighbours
            .into_iter()
            .chain([camera, second_ring])
            .map(|key| (key, FaceConnectivity::none())),
    );

    let visible = cave_visible_sub_chunks(camera, &graph);
    assert_eq!(visible.len(), 7);
    assert!(neighbours.into_iter().all(|key| visible.contains(&key)));
    assert!(!visible.contains(&second_ring));
}

#[test]
fn conservative_shell_stays_in_dimension_and_handles_coordinate_limits() {
    let camera = SubChunkKey::new(7, i32::MAX, 0, 0);
    let loaded_neighbour = SubChunkKey::new(7, i32::MAX - 1, 0, 0);
    let other_dimension = SubChunkKey::new(8, i32::MAX, 0, 0);
    let graph = grid([
        (camera, FaceConnectivity::none()),
        (loaded_neighbour, FaceConnectivity::none()),
        (other_dimension, FaceConnectivity::none()),
    ]);

    let visible = cave_visible_sub_chunks(camera, &graph);
    assert!(visible.contains(&loaded_neighbour));
    assert!(!visible.contains(&other_dimension));
}

#[test]
fn missing_camera_node_falls_back_to_conservative_visibility() {
    let camera = SubChunkKey::new(0, 99, 0, 99);
    let loaded = SubChunkKey::new(0, 0, 0, 0);
    let graph = grid([(loaded, FaceConnectivity::none())]);

    assert_eq!(cave_visible_sub_chunks(camera, &graph), [loaded].into());
}

#[test]
fn outdoor_camera_boundary_keeps_the_loaded_graph_continuously_visible() {
    let before_boundary = SubChunkKey::new(0, 0, 4, 0);
    let after_boundary = SubChunkKey::new(0, 1, 4, 0);
    let support_before = SubChunkKey::new(0, 0, 3, 0);
    let support_after = SubChunkKey::new(0, 1, 3, 0);
    let graph = grid([
        (before_boundary, FaceConnectivity::all()),
        (after_boundary, FaceConnectivity::all()),
        (support_before, FaceConnectivity::none()),
        (support_after, FaceConnectivity::none()),
    ]);

    let before = cave_visible_sub_chunks(before_boundary, &graph);
    let after = cave_visible_sub_chunks(after_boundary, &graph);

    assert_eq!(
        before, after,
        "crossing an outdoor sub-chunk boundary must not hide a loaded entity for one frame"
    );
    assert_eq!(before, graph.keys().collect());
}

/// The hash-map traversal the dense grid replaced, kept verbatim as the output oracle.
#[derive(Default)]
struct OracleScratch {
    visited: HashMap<SubChunkKey, u8>,
    queue: VecDeque<(SubChunkKey, Option<Face>)>,
}

fn oracle_fill(
    camera: SubChunkKey,
    connectivity: &HashMap<SubChunkKey, FaceConnectivity>,
    scratch: &mut OracleScratch,
    visible: &mut HashSet<SubChunkKey>,
) {
    visible.clear();
    scratch.visited.clear();
    scratch.queue.clear();
    if !connectivity.contains_key(&camera) {
        visible.extend(connectivity.keys().copied());
        return;
    }
    let visited = &mut scratch.visited;
    visited.insert(camera, 1 << 6);
    let queue = &mut scratch.queue;
    queue.push_back((camera, None));
    while let Some((key, entered_from)) = queue.pop_front() {
        let Some(connections) = connectivity.get(&key).copied() else {
            continue;
        };
        for exit in Face::ALL {
            let can_exit = entered_from.map_or_else(
                || connections.is_connected(exit, exit),
                |entry| connections.is_connected(entry, exit),
            );
            if !can_exit {
                continue;
            }
            let Some(next) = adjacent(key, exit) else {
                continue;
            };
            if !connectivity.contains_key(&next) {
                continue;
            }
            let entered_bit = 1_u8 << (opposite(exit) as u8);
            let visited_faces = visited.entry(next).or_default();
            if *visited_faces & entered_bit == 0 {
                *visited_faces |= entered_bit;
                queue.push_back((next, Some(opposite(exit))));
            }
        }
    }
    visible.extend(visited.keys().copied());
    for &key in visited.keys() {
        for face in Face::ALL {
            let Some(neighbour) = adjacent(key, face) else {
                continue;
            };
            if connectivity.contains_key(&neighbour) {
                visible.insert(neighbour);
            }
        }
    }
}

/// Computes the reference visible set without retaining state between updates.
pub(super) fn oracle(
    camera: SubChunkKey,
    map: &HashMap<SubChunkKey, FaceConnectivity>,
) -> HashSet<SubChunkKey> {
    let mut visible = HashSet::new();
    oracle_fill(camera, map, &mut OracleScratch::default(), &mut visible);
    visible
}

/// Deterministic xorshift so failures reproduce without a rand dependency.
pub(super) struct Rng(pub(super) u64);

impl Rng {
    /// Returns the next reproducible random value.
    pub(super) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Selects an integer below the nonzero bound.
    pub(super) fn below(&mut self, bound: u64) -> i32 {
        (self.next() % bound) as i32
    }

    /// Mixes open, sealed and arbitrary directed face matrices.
    pub(super) fn connectivity(&mut self) -> FaceConnectivity {
        match self.next() % 4 {
            0 => FaceConnectivity::all(),
            1 => FaceConnectivity::none(),
            // Sparse matrices keep many portals closed so traversal order matters.
            _ => FaceConnectivity::from_bits(self.next() & self.next() & self.next()),
        }
    }
}

/// Terrain-like columns: open sky above a surface, cave-riddled rock below.
fn fixture_world(radius: i32, seed: u64) -> HashMap<SubChunkKey, FaceConnectivity> {
    let mut rng = Rng(seed);
    let mut map = HashMap::new();
    for x in -radius..=radius {
        for z in -radius..=radius {
            if x * x + z * z > radius * radius {
                continue;
            }
            let surface = 3 + (x + 2 * z).rem_euclid(3);
            for y in -4..20 {
                let value = if y > surface {
                    FaceConnectivity::all()
                } else if rng.next().is_multiple_of(3) {
                    FaceConnectivity::none()
                } else {
                    rng.connectivity()
                };
                map.insert(SubChunkKey::new(0, x, y, z), value);
            }
        }
    }
    map
}

fn assert_matches_oracle(
    camera: SubChunkKey,
    map: &HashMap<SubChunkKey, FaceConnectivity>,
    grid: &ConnectivityGrid,
    scratch: &mut CaveVisibilityScratch,
    visible: &mut CaveVisibleSet,
) {
    fill_visible(camera, grid, scratch, visible);
    let expected = oracle(camera, map);
    let actual = visible.iter().collect::<HashSet<_>>();
    assert_eq!(
        actual.len(),
        visible.len(),
        "duplicate members for {camera:?}"
    );
    assert_eq!(
        actual, expected,
        "visible set diverged for camera {camera:?}"
    );
    for key in map.keys() {
        assert_eq!(visible.contains(key), expected.contains(key), "{key:?}");
    }
}

/// Dense traversal reproduces the hash-map oracle on terrain fixtures at both gate radii.
#[test]
fn dense_traversal_matches_the_oracle_on_fixture_worlds() {
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    for (radius, seed) in [(8, 0x9e37_79b9), (12, 0x1234_5678_9abc)] {
        let map = fixture_world(radius, seed);
        let grid = grid(map.iter().map(|(key, value)| (*key, *value)));
        for camera in [
            SubChunkKey::new(0, 0, 10, 0),
            SubChunkKey::new(0, 0, 1, 0),
            SubChunkKey::new(0, 3, -2, -4),
            SubChunkKey::new(0, -radius, 0, 0),
            SubChunkKey::new(0, 0, 40, 0),
        ] {
            assert_matches_oracle(camera, &map, &grid, &mut scratch, &mut visible);
        }
    }
}

/// Random graphs, including toroidal collisions, other dimensions and edits between calls.
#[test]
fn dense_traversal_matches_the_oracle_on_random_worlds() {
    let mut rng = Rng(0xdead_beef_cafe);
    for round in 0..40 {
        // Spans past the initial 32-cell axes force overflow keys and grid growth.
        let span = [6, 20, 40, 90][round % 4];
        let mut map = HashMap::new();
        for _ in 0..rng.below(3_000) + 1 {
            let key = SubChunkKey::new(
                i32::from(rng.next().is_multiple_of(8)),
                rng.below(span) - span as i32 / 2,
                rng.below(40) - 8,
                rng.below(span) - span as i32 / 2,
            );
            map.insert(key, rng.connectivity());
        }
        let mut grid = grid(map.iter().map(|(key, value)| (*key, *value)));
        let mut scratch = CaveVisibilityScratch::default();
        let mut visible = CaveVisibleSet::default();
        for _ in 0..12 {
            let keys = map.keys().copied().collect::<Vec<_>>();
            if keys.is_empty() {
                break;
            }
            let camera = keys[rng.below(keys.len() as u64) as usize];
            assert_matches_oracle(camera, &map, &grid, &mut scratch, &mut visible);
            for _ in 0..50 {
                let key = keys[rng.below(keys.len() as u64) as usize];
                if rng.next().is_multiple_of(2) {
                    assert_eq!(grid.remove(&key), map.remove(&key));
                } else {
                    let value = rng.connectivity();
                    assert_eq!(grid.insert(key, value), map.insert(key, value));
                }
            }
            assert_eq!(grid.len(), map.len());
        }
        map.retain(|key, _| key.x % 3 != 0);
        grid.retain(|key| key.x % 3 != 0);
        assert_eq!(
            grid.keys().collect::<HashSet<_>>(),
            map.keys().copied().collect()
        );
        let camera = map
            .keys()
            .next()
            .copied()
            .unwrap_or(SubChunkKey::new(0, 0, 0, 0));
        assert_matches_oracle(camera, &map, &grid, &mut scratch, &mut visible);
    }
}

/// Colliding keys keep exact lookups through removal, promotion and retain.
#[test]
fn toroidal_collisions_stay_exact() {
    let home = SubChunkKey::new(0, 1, 2, 3);
    let wrapped = SubChunkKey::new(0, 1 + 32, 2, 3 - 64);
    let other_dimension = SubChunkKey::new(1, 1, 2, 3);
    let mut grid = grid([
        (home, FaceConnectivity::all()),
        (wrapped, FaceConnectivity::none()),
        (other_dimension, FaceConnectivity::all()),
    ]);
    assert_eq!(grid.get(&wrapped), Some(FaceConnectivity::none()));
    assert_eq!(grid.remove(&home), Some(FaceConnectivity::all()));
    assert_eq!(grid.get(&home), None);
    assert_eq!(grid.get(&wrapped), Some(FaceConnectivity::none()));
    assert_eq!(grid.get(&other_dimension), Some(FaceConnectivity::all()));
    grid.retain(|key| key.dimension == 0);
    assert_eq!(grid.keys().collect::<Vec<_>>(), [wrapped]);
    assert_eq!(grid.len(), 1);
}

/// Set equality ignores the window position, and keys outside the window still count.
#[test]
fn visible_sets_compare_by_members() {
    let near = SubChunkKey::new(0, 0, 0, 0);
    let far = SubChunkKey::new(0, 500, 0, 0);
    let mut left = CaveVisibleSet::default();
    let mut right = CaveVisibleSet::default();
    left.reset(near, (5, 5));
    right.reset(SubChunkKey::new(0, 3, 0, 0), (5, 5));
    for key in [near, far] {
        assert!(left.insert(key));
        assert!(right.insert(key));
    }
    assert!(!left.insert(near));
    assert_eq!(left, right);
    assert!(right.insert(SubChunkKey::new(0, 1, 0, 0)));
    assert_ne!(left, right);
    left.reset(near, (5, 5));
    assert!(left.is_empty() && !left.contains(&near) && !left.contains(&far));
}

/// Applies either publication path while keeping the same retained traversal state.
pub(super) fn update_incrementally(
    camera: SubChunkKey,
    grid: &ConnectivityGrid,
    scratch: &mut CaveVisibilityScratch,
    visible: &mut CaveVisibleSet,
    replacement: &mut CaveVisibleSet,
) -> bool {
    let rebuilt = update_visible(camera, grid, scratch, visible, replacement);
    if rebuilt {
        std::mem::swap(visible, replacement);
    }
    rebuilt
}

#[test]
fn incremental_addition_does_not_revisit_the_resident_graph() {
    let mut map = fixture_world(12, 0x5eed);
    let mut grid = grid(map.iter().map(|(key, value)| (*key, *value)));
    let camera = SubChunkKey::new(0, 0, 10, 0);
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    let mut replacement = CaveVisibleSet::default();
    assert!(update_incrementally(
        camera,
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement
    ));
    assert!(scratch.work.explored_exits > 10_000);

    let fresh = SubChunkKey::new(0, 13, 10, 0);
    map.insert(fresh, FaceConnectivity::all());
    grid.insert(fresh, FaceConnectivity::all());
    assert!(!update_incrementally(
        camera,
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement
    ));
    assert_eq!(scratch.work.explored_exits, 6);
    assert_eq!(scratch.added_visible(), &[fresh]);
    assert_eq!(visible.iter().collect::<HashSet<_>>(), oracle(camera, &map));

    grid.insert(fresh, FaceConnectivity::all());
    assert!(!update_incrementally(
        camera,
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement
    ));
    assert_eq!(scratch.work.explored_exits, 0);
    assert!(scratch.added_visible().is_empty());
}

#[test]
fn incremental_additions_keep_exactly_one_support_shell() {
    let key = |x| SubChunkKey::new(0, x, 0, 0);
    let mut grid = grid([(key(0), FaceConnectivity::all())]);
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    let mut replacement = CaveVisibleSet::default();
    update_incrementally(key(0), &grid, &mut scratch, &mut visible, &mut replacement);
    // Reverse insertion order ensures traversal, not journal order, exposes the support shell.
    grid.insert(key(3), FaceConnectivity::all());
    grid.insert(key(2), FaceConnectivity::all());
    grid.insert(key(1), FaceConnectivity::none());
    assert!(!update_incrementally(
        key(0),
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement
    ));
    assert_eq!(
        visible.iter().collect::<HashSet<_>>(),
        [key(0), key(1), key(2)].into()
    );
    grid.insert(key(4), FaceConnectivity::all());
    assert!(!update_incrementally(
        key(0),
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement
    ));
    assert!(!visible.contains(&key(3)) && !visible.contains(&key(4)));
    assert_eq!(scratch.work.explored_exits, 0);
}

#[test]
fn incremental_updates_match_full_traversal_through_mutations() {
    let mut rng = Rng(0x51ea_cafe);
    let mut map = fixture_world(3, 0x5eed);
    let mut grid = grid(map.iter().map(|(key, value)| (*key, *value)));
    let mut camera = SubChunkKey::new(0, 0, 10, 0);
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    let mut replacement = CaveVisibleSet::default();
    for step in 0..300 {
        for _ in 0..8 {
            let key = SubChunkKey::new(
                i32::from(rng.next().is_multiple_of(11)),
                rng.below(12) - 6,
                rng.below(80) - 20,
                rng.below(12) - 6,
            );
            let value = rng.connectivity();
            map.insert(key, value);
            grid.insert(key, value);
        }
        if step % 13 == 0 {
            let key = SubChunkKey::new(0, 0, 9, 0);
            let value = rng.connectivity();
            map.insert(key, value);
            grid.insert(key, value);
        }
        if step % 17 == 0 {
            grid.remove(&camera);
            map.remove(&camera);
        }
        if step % 19 == 0 {
            map.insert(camera, FaceConnectivity::all());
            grid.insert(camera, FaceConnectivity::all());
        }
        if step % 23 == 0 {
            grid.retain(|key| key.x != -5);
            map.retain(|key, _| key.x != -5);
        }
        if step % 31 == 0 {
            camera = SubChunkKey::new(0, rng.below(6) - 3, rng.below(20), 0);
        }
        update_incrementally(camera, &grid, &mut scratch, &mut visible, &mut replacement);
        assert_eq!(
            visible.iter().collect::<HashSet<_>>(),
            oracle(camera, &map),
            "step {step}"
        );
        assert_eq!(visible.len(), visible.iter().collect::<HashSet<_>>().len());
    }
}

#[test]
fn incremental_history_rollover_and_grid_replacement_rebuild_exactly() {
    let camera = SubChunkKey::new(0, 0, 0, 0);
    let mut grid = grid([(camera, FaceConnectivity::all())]);
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    let mut replacement = CaveVisibleSet::default();
    update_incrementally(camera, &grid, &mut scratch, &mut visible, &mut replacement);
    for x in 0..32 {
        for y in 0..8 {
            for z in 0..32 {
                grid.insert(SubChunkKey::new(0, x, y, z), FaceConnectivity::all());
            }
        }
    }
    assert!(update_incrementally(
        camera,
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement
    ));
    assert_eq!(visible.len(), grid.len());
    grid = [(camera, FaceConnectivity::none())].into_iter().collect();
    assert!(update_incrementally(
        camera,
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement
    ));
    assert_eq!(visible.iter().collect::<Vec<_>>(), [camera]);
}

/// Release timing: `cargo test --release -p chunk-pipeline cave_visibility_bench -- --ignored --nocapture`.
#[test]
#[ignore = "offline cave traversal timing fixture"]
fn cave_visibility_bench() {
    use std::{hint::black_box, time::Instant};
    fn spread(mut samples: Vec<u128>) -> [f64; 3] {
        samples.sort_unstable();
        [0, samples.len() / 2, samples.len() * 95 / 100].map(|index| samples[index] as f64 / 1e3)
    }
    for radius in [8, 12] {
        let map = fixture_world(radius, 0x5eed);
        let grid = grid(map.iter().map(|(key, value)| (*key, *value)));
        for (label, camera) in [
            ("surface", SubChunkKey::new(0, 0, 10, 0)),
            ("cave", SubChunkKey::new(0, 0, 1, 0)),
        ] {
            let mut oracle_scratch = OracleScratch::default();
            let mut oracle_visible = HashSet::new();
            let mut scratch = CaveVisibilityScratch::default();
            let mut visible = CaveVisibleSet::default();
            let (mut old, mut new) = (Vec::new(), Vec::new());
            for _ in 0..1001 {
                let started = Instant::now();
                oracle_fill(camera, &map, &mut oracle_scratch, &mut oracle_visible);
                black_box(&oracle_visible);
                old.push(started.elapsed().as_nanos());
                let started = Instant::now();
                fill_visible(camera, &grid, &mut scratch, &mut visible);
                black_box(&visible);
                new.push(started.elapsed().as_nanos());
            }
            assert_eq!(visible.iter().collect::<HashSet<_>>(), oracle_visible);
            let (old, new) = (spread(old), spread(new));
            println!(
                "cave_visibility radius={radius} camera={label} nodes={} visible={} old_us(min/p50/p95)={old:.1?} new_us={new:.1?} speedup_p50={:.1}x",
                map.len(),
                visible.len(),
                old[1] / new[1],
            );
        }
    }
}

/// Crossing an open boundary preserves the existing reachable region.
#[test]
fn camera_boundary_preserves_the_existing_search() {
    let first = SubChunkKey::new(0, 0, 0, 0);
    let next = SubChunkKey::new(0, 1, 0, 0);
    let grid = grid([
        (first, FaceConnectivity::all()),
        (next, FaceConnectivity::all()),
    ]);
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    let mut replacement = CaveVisibleSet::default();
    update_incrementally(first, &grid, &mut scratch, &mut visible, &mut replacement);
    assert!(!update_incrementally(
        next,
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement
    ));
}
