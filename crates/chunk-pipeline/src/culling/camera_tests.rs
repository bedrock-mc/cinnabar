use meshing::FaceConnectivity;

use super::tests::{Rng, grid, oracle, update_incrementally};
use super::*;

/// Creates a key in the test world's primary dimension.
fn key(x: i32, y: i32, z: i32) -> SubChunkKey {
    SubChunkKey::new(0, x, y, z)
}

/// Keeps the camera's two horizontal regions disconnected from each other.
fn split_horizontal() -> FaceConnectivity {
    FaceConnectivity::from_bits(
        (1 << (Face::NegativeX as u64 * 7)) | (1 << (Face::PositiveX as u64 * 7)),
    )
}

/// Builds an open horizontal resident region around the camera.
fn open_region(radius: i32) -> HashMap<SubChunkKey, FaceConnectivity> {
    (-radius..=radius)
        .flat_map(|x| (-radius..=radius).map(move |z| (key(x, 0, z), FaceConnectivity::all())))
        .collect()
}

#[test]
fn adjacent_camera_moves_do_constant_work_in_small_and_large_regions() {
    for radius in [2, 12] {
        let map = open_region(radius);
        let grid = grid(map.iter().map(|(&key, &value)| (key, value)));
        let mut scratch = CaveVisibilityScratch::default();
        let mut visible = CaveVisibleSet::default();
        let mut replacement = CaveVisibleSet::default();
        update_incrementally(
            key(0, 0, 0),
            &grid,
            &mut scratch,
            &mut visible,
            &mut replacement,
        );
        for _ in 0..8 {
            for camera in [key(1, 0, 0), key(1, 0, 1), key(0, 0, 1), key(0, 0, 0)] {
                assert!(!update_incrementally(
                    camera,
                    &grid,
                    &mut scratch,
                    &mut visible,
                    &mut replacement,
                ));
                let work = scratch.work();
                assert!(!work.rebuilt);
                assert_eq!(work.explored_exits, 0);
                assert_eq!(work.additions, 0);
                assert!(work.proof_exits <= Face::ALL.len(), "{work:?}");
                assert!(scratch.added_visible().is_empty());
                assert_eq!(visible.iter().collect::<HashSet<_>>(), oracle(camera, &map));
            }
        }
    }
}

#[test]
fn camera_flying_into_new_subchunks_only_explores_the_additions() {
    let mut map = open_region(12);
    let mut grid = grid(map.iter().map(|(&key, &value)| (key, value)));
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    let mut replacement = CaveVisibleSet::default();
    update_incrementally(
        key(12, 0, 0),
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement,
    );
    for x in 13..=24 {
        let camera = key(x, 0, 0);
        map.insert(camera, FaceConnectivity::all());
        grid.insert(camera, FaceConnectivity::all());
        assert!(!update_incrementally(
            camera,
            &grid,
            &mut scratch,
            &mut visible,
            &mut replacement,
        ));
        let work = scratch.work();
        assert!(!work.rebuilt);
        assert_eq!(work.additions, 1);
        assert_eq!(work.explored_exits, Face::ALL.len());
        assert!(work.proof_exits <= Face::ALL.len(), "{work:?}");
        assert_eq!(scratch.added_visible(), &[camera]);
        assert_eq!(visible.iter().collect::<HashSet<_>>(), oracle(camera, &map));
    }
}

#[test]
fn active_visibility_reader_survives_addition_journal_rollover() {
    let camera = key(0, 0, 0);
    let mut map = [(camera, FaceConnectivity::all())]
        .into_iter()
        .collect::<HashMap<_, _>>();
    let mut grid = grid(map.iter().map(|(&key, &value)| (key, value)));
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    let mut replacement = CaveVisibleSet::default();
    update_incrementally(camera, &grid, &mut scratch, &mut visible, &mut replacement);
    let initial_checkpoint = grid.checkpoint();
    for x in 0..32 {
        for y in 0..8 {
            for z in 0..32 {
                let addition = key(x, y, z);
                if addition == camera {
                    continue;
                }
                map.insert(addition, FaceConnectivity::all());
                grid.insert(addition, FaceConnectivity::all());
                assert!(!update_incrementally(
                    camera,
                    &grid,
                    &mut scratch,
                    &mut visible,
                    &mut replacement,
                ));
                let work = scratch.work();
                assert!(!work.rebuilt);
                assert_eq!(work.additions, 1);
                assert_eq!(work.explored_exits, Face::ALL.len());
                assert_eq!(work.proof_exits, 0);
                assert_eq!(visible.len(), grid.len());
            }
        }
    }
    assert_eq!(grid.checkpoint().1, initial_checkpoint.1);
    assert!(!grid.retains_additions(initial_checkpoint.2));
    assert_eq!(visible.iter().collect::<HashSet<_>>(), oracle(camera, &map));
}

#[test]
fn distant_camera_move_falls_back_after_the_bounded_proof() {
    let map = open_region(12);
    let grid = grid(map.iter().map(|(&key, &value)| (key, value)));
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    let mut replacement = CaveVisibleSet::default();
    update_incrementally(
        key(0, 0, 0),
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement,
    );
    let camera = key(12, 0, 12);
    assert!(update_incrementally(
        camera,
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement,
    ));
    assert_eq!(scratch.work().proof_exits, camera::CAMERA_PROOF_BUDGET);
    assert_eq!(visible.iter().collect::<HashSet<_>>(), oracle(camera, &map));
}

#[test]
fn reaching_the_previous_camera_does_not_prove_all_its_exits_reachable() {
    let mut map = (-3..=3)
        .map(|x| (key(x, 0, 0), FaceConnectivity::all()))
        .collect::<HashMap<_, _>>();
    map.insert(key(0, 0, 0), split_horizontal());
    let grid = grid(map.iter().map(|(&key, &value)| (key, value)));
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    let mut replacement = CaveVisibleSet::default();
    update_incrementally(
        key(0, 0, 0),
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement,
    );
    assert!(visible.contains(&key(3, 0, 0)));
    let camera = key(-1, 0, 0);
    assert!(update_incrementally(
        camera,
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement,
    ));
    assert!(visible.contains(&key(0, 0, 0)));
    assert!(!visible.contains(&key(2, 0, 0)));
    assert_eq!(visible.iter().collect::<HashSet<_>>(), oracle(camera, &map));
}

#[test]
fn new_camera_diagonals_can_reveal_a_previously_disconnected_region() {
    let mut map = (0..=4)
        .map(|x| (key(x, 0, 0), FaceConnectivity::all()))
        .collect::<HashMap<_, _>>();
    map.insert(key(1, 0, 0), split_horizontal());
    let grid = grid(map.iter().map(|(&key, &value)| (key, value)));
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    let mut replacement = CaveVisibleSet::default();
    update_incrementally(
        key(0, 0, 0),
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement,
    );
    assert!(!visible.contains(&key(3, 0, 0)));
    let camera = key(1, 0, 0);
    update_incrementally(camera, &grid, &mut scratch, &mut visible, &mut replacement);
    assert!(visible.contains(&key(4, 0, 0)));
    assert_eq!(visible.iter().collect::<HashSet<_>>(), oracle(camera, &map));
}

#[test]
fn shell_only_and_sealed_camera_positions_keep_exact_visibility() {
    let mut map = (-3..=3)
        .map(|x| (key(x, 0, 0), FaceConnectivity::all()))
        .collect::<HashMap<_, _>>();
    map.insert(key(1, 0, 0), FaceConnectivity::none());
    let grid = grid(map.iter().map(|(&key, &value)| (key, value)));
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    let mut replacement = CaveVisibleSet::default();
    for x in [0, 2, 1, 0, 1, 2] {
        let camera = key(x, 0, 0);
        update_incrementally(camera, &grid, &mut scratch, &mut visible, &mut replacement);
        assert_eq!(visible.iter().collect::<HashSet<_>>(), oracle(camera, &map));
        if x == 1 {
            assert_eq!(visible.len(), 3);
            assert!(!visible.contains(&key(-1, 0, 0)));
            assert!(!visible.contains(&key(3, 0, 0)));
        }
    }
}

#[test]
fn failed_camera_proof_keeps_additions_out_of_the_previous_published_set() {
    let mut map = (-3..=3)
        .map(|x| (key(x, 0, 0), FaceConnectivity::all()))
        .collect::<HashMap<_, _>>();
    map.insert(key(0, 0, 0), split_horizontal());
    let mut grid = grid(map.iter().map(|(&key, &value)| (key, value)));
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    let mut replacement = CaveVisibleSet::default();
    update_incrementally(
        key(0, 0, 0),
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement,
    );
    let previous = visible.clone();
    let addition = key(-4, 0, 0);
    map.insert(addition, FaceConnectivity::all());
    grid.insert(addition, FaceConnectivity::all());
    let camera = key(-1, 0, 0);
    assert!(update_visible(
        camera,
        &grid,
        &mut scratch,
        &mut visible,
        &mut replacement,
    ));
    assert_eq!(visible, previous);
    assert!(!visible.contains(&addition));
    assert!(replacement.contains(&addition));
    assert_eq!(
        replacement.iter().collect::<HashSet<_>>(),
        oracle(camera, &map)
    );
}

#[test]
fn random_camera_moves_and_graph_mutations_match_the_full_search() {
    let mut rng = Rng(0x974b_01dc_5ca1);
    for round in 0..12 {
        let mut map = (-3..=3)
            .flat_map(|x| (-2..=2).flat_map(move |y| (-3..=3).map(move |z| key(x, y, z))))
            .map(|key| (key, rng.connectivity()))
            .collect::<HashMap<_, _>>();
        let mut grid = grid(map.iter().map(|(&key, &value)| (key, value)));
        let mut scratch = CaveVisibilityScratch::default();
        let mut visible = CaveVisibleSet::default();
        let mut replacement = CaveVisibleSet::default();
        let mut camera = key(0, 0, 0);
        for step in 0..160 {
            if step % 3 == 0 {
                let changed = SubChunkKey::new(
                    i32::from(rng.next().is_multiple_of(13)),
                    rng.below(9) - 4 + i32::from(step % 11 == 0) * 32,
                    rng.below(7) - 3,
                    rng.below(9) - 4,
                );
                if rng.next().is_multiple_of(4) {
                    assert_eq!(grid.remove(&changed), map.remove(&changed));
                } else {
                    let value = rng.connectivity();
                    assert_eq!(grid.insert(changed, value), map.insert(changed, value));
                }
            }
            if step % 17 == 0 {
                assert_eq!(grid.remove(&camera), map.remove(&camera));
            }
            if step % 19 == 0 {
                let value = rng.connectivity();
                assert_eq!(grid.insert(camera, value), map.insert(camera, value));
            }
            camera = if step % 5 == 0 {
                SubChunkKey::new(
                    i32::from(step % 23 == 0),
                    rng.below(7) - 3,
                    rng.below(5) - 2,
                    rng.below(7) - 3,
                )
            } else {
                adjacent(
                    camera,
                    Face::ALL[rng.below(Face::ALL.len() as u64) as usize],
                )
                .unwrap()
            };
            let rebuilt =
                update_incrementally(camera, &grid, &mut scratch, &mut visible, &mut replacement);
            assert_eq!(scratch.work().rebuilt, rebuilt);
            assert!(scratch.work().proof_exits <= camera::CAMERA_PROOF_BUDGET);
            assert_eq!(
                visible.iter().collect::<HashSet<_>>(),
                oracle(camera, &map),
                "round {round}, step {step}, camera {camera:?}",
            );
            assert_eq!(visible.len(), visible.iter().collect::<HashSet<_>>().len());
        }
    }
}

#[test]
fn camera_proofs_handle_coordinate_edges_and_overflow_slots() {
    let positions = [i32::MIN, 0, 32, 64, i32::MAX - 1];
    let map = positions
        .into_iter()
        .flat_map(|x| [key(x, 0, 0), key(x + 1, 0, 0)])
        .map(|key| (key, FaceConnectivity::all()))
        .collect::<HashMap<_, _>>();
    let grid = grid(map.iter().map(|(&key, &value)| (key, value)));
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    let mut replacement = CaveVisibleSet::default();
    for x in positions {
        update_incrementally(
            key(x, 0, 0),
            &grid,
            &mut scratch,
            &mut visible,
            &mut replacement,
        );
        let camera = key(x + 1, 0, 0);
        assert!(!update_incrementally(
            camera,
            &grid,
            &mut scratch,
            &mut visible,
            &mut replacement,
        ));
        assert_eq!(scratch.work().explored_exits, 0);
        assert!(scratch.work().proof_exits <= Face::ALL.len());
        assert_eq!(visible.iter().collect::<HashSet<_>>(), oracle(camera, &map));
    }
}

#[test]
fn warmed_camera_flight_allocates_nothing() {
    let map = open_region(8);
    let grid = grid(map.iter().map(|(&key, &value)| (key, value)));
    let mut scratch = CaveVisibilityScratch::default();
    let mut visible = CaveVisibleSet::default();
    let mut replacement = CaveVisibleSet::default();
    let flight = [key(0, 0, 0), key(1, 0, 0), key(1, 0, 1), key(0, 0, 1)];
    for _ in 0..2 {
        for camera in flight {
            update_incrementally(camera, &grid, &mut scratch, &mut visible, &mut replacement);
        }
    }
    let before = crate::stream::tests::allocation_count::thread_allocations();
    for _ in 0..128 {
        for camera in flight {
            assert!(!update_incrementally(
                camera,
                &grid,
                &mut scratch,
                &mut visible,
                &mut replacement,
            ));
            assert_eq!(scratch.work().explored_exits, 0);
            assert!(scratch.work().proof_exits <= Face::ALL.len());
        }
    }
    assert_eq!(
        crate::stream::tests::allocation_count::thread_allocations(),
        before,
        "camera boundary crossings allocated after warming the traversal buffers",
    );
}
