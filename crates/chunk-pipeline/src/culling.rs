use std::collections::{HashSet, VecDeque};

use hashbrown::HashMap;

use meshing::{Face, FaceConnectivity};
use world::SubChunkKey;

/// Conservative face-connectivity BFS used before Bevy's per-entity frustum culling.
#[must_use]
pub fn cave_visible_sub_chunks(
    camera: SubChunkKey,
    connectivity: &HashMap<SubChunkKey, FaceConnectivity>,
) -> HashSet<SubChunkKey> {
    let mut visible = HashSet::new();
    let mut scratch = CaveVisibilityScratch {
        visited: HashMap::with_capacity(connectivity.len()),
        queue: VecDeque::with_capacity(connectivity.len()),
    };
    fill_visible(camera, connectivity, &mut scratch, &mut visible);
    visible
}

/// Reusable BFS storage; visibility remains conservative and computed atomically.
#[derive(Default)]
pub struct CaveVisibilityScratch {
    visited: HashMap<SubChunkKey, u8>,
    queue: VecDeque<(SubChunkKey, Option<Face>)>,
}

/// Reuses traversal and output storage without changing portal or support-shell rules.
pub(crate) fn fill_visible(
    camera: SubChunkKey,
    connectivity: &HashMap<SubChunkKey, FaceConnectivity>,
    scratch: &mut CaveVisibilityScratch,
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
    // Visibility is per sub-chunk entity, not per connected air region. Once
    // any portal exposes an entity, models in another region of that entity
    // are drawn too. Keep exactly one loaded neighbour shell visible so those
    // models cannot float over support geometry hidden in an adjacent entity.
    // Snapshot first: newly added shell nodes must not recursively expand.
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

fn adjacent(key: SubChunkKey, face: Face) -> Option<SubChunkKey> {
    let (x, y, z) = match face {
        Face::NegativeX => (key.x.checked_sub(1)?, key.y, key.z),
        Face::PositiveX => (key.x.checked_add(1)?, key.y, key.z),
        Face::NegativeY => (key.x, key.y.checked_sub(1)?, key.z),
        Face::PositiveY => (key.x, key.y.checked_add(1)?, key.z),
        Face::NegativeZ => (key.x, key.y, key.z.checked_sub(1)?),
        Face::PositiveZ => (key.x, key.y, key.z.checked_add(1)?),
    };
    Some(SubChunkKey::new(key.dimension, x, y, z))
}

const fn opposite(face: Face) -> Face {
    match face {
        Face::NegativeX => Face::PositiveX,
        Face::PositiveX => Face::NegativeX,
        Face::NegativeY => Face::PositiveY,
        Face::PositiveY => Face::NegativeY,
        Face::NegativeZ => Face::PositiveZ,
        Face::PositiveZ => Face::NegativeZ,
    }
}

#[cfg(test)]
mod tests {
    use hashbrown::HashMap;

    use meshing::FaceConnectivity;
    use world::SubChunkKey;

    use super::cave_visible_sub_chunks;

    #[test]
    fn all_air_connectivity_walks_the_loaded_graph() {
        let first = SubChunkKey::new(0, 0, 0, 0);
        let second = SubChunkKey::new(0, 1, 0, 0);
        let third = SubChunkKey::new(0, 2, 0, 0);
        let graph = HashMap::from([
            (first, FaceConnectivity::all()),
            (second, FaceConnectivity::all()),
            (third, FaceConnectivity::all()),
        ]);

        let visible = cave_visible_sub_chunks(first, &graph);
        assert_eq!(visible.len(), 3);
        assert!(visible.contains(&first));
        assert!(visible.contains(&second));
        assert!(visible.contains(&third));
    }

    #[test]
    fn sealed_middle_subchunk_stops_bfs_beyond_the_conservative_shell() {
        let first = SubChunkKey::new(0, 0, 0, 0);
        let sealed = SubChunkKey::new(0, 1, 0, 0);
        let shell = SubChunkKey::new(0, 2, 0, 0);
        let hidden = SubChunkKey::new(0, 3, 0, 0);
        let graph = HashMap::from([
            (first, FaceConnectivity::all()),
            (sealed, FaceConnectivity::none()),
            (shell, FaceConnectivity::all()),
            (hidden, FaceConnectivity::all()),
        ]);

        let visible = cave_visible_sub_chunks(first, &graph);
        assert!(visible.contains(&first));
        assert!(visible.contains(&sealed));
        assert!(visible.contains(&shell));
        assert!(!visible.contains(&hidden));
    }

    #[test]
    fn visible_outdoor_node_keeps_its_loaded_support_shell_visible() {
        let camera = SubChunkKey::new(0, 0, 4, 0);
        let outdoor = SubChunkKey::new(0, 1, 4, 0);
        let support = SubChunkKey::new(0, 1, 3, 0);
        let deeper_interior = SubChunkKey::new(0, 1, 2, 0);
        let graph = HashMap::from([
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
        let mut graph = HashMap::from([(camera, FaceConnectivity::none())]);
        graph.extend(
            neighbours
                .into_iter()
                .map(|key| (key, FaceConnectivity::none())),
        );
        graph.insert(second_ring, FaceConnectivity::none());

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
        let graph = HashMap::from([
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
        let graph = HashMap::from([(loaded, FaceConnectivity::none())]);

        assert_eq!(cave_visible_sub_chunks(camera, &graph), [loaded].into());
    }

    #[test]
    fn outdoor_camera_boundary_keeps_the_loaded_graph_continuously_visible() {
        let before_boundary = SubChunkKey::new(0, 0, 4, 0);
        let after_boundary = SubChunkKey::new(0, 1, 4, 0);
        let support_before = SubChunkKey::new(0, 0, 3, 0);
        let support_after = SubChunkKey::new(0, 1, 3, 0);
        let graph = HashMap::from([
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
        assert_eq!(before, graph.keys().copied().collect());
    }
}

#[cfg(test)]
mod reuse_tests {
    use super::*;

    /// Compares cold allocations with repeated traversal through retained buffers.
    #[test]
    #[ignore = "offline cave traversal timing fixture"]
    fn cave_visibility_buffer_timing() {
        use std::{hint::black_box, time::Instant};
        let graph = (0..4_096)
            .map(|x| (SubChunkKey::new(0, x, 0, 0), FaceConnectivity::all()))
            .collect::<HashMap<_, _>>();
        let camera = SubChunkKey::new(0, 0, 0, 0);
        let expected = cave_visible_sub_chunks(camera, &graph);
        let mut scratch = CaveVisibilityScratch::default();
        let mut visible = HashSet::new();
        for reuse in [false, true] {
            let mut times = Vec::new();
            for _ in 0..31 {
                let start = Instant::now();
                if reuse {
                    fill_visible(camera, &graph, &mut scratch, &mut visible);
                    black_box(&visible);
                } else {
                    black_box(cave_visible_sub_chunks(camera, &graph));
                }
                times.push(start.elapsed().as_micros());
            }
            times.remove(0);
            times.sort_unstable();
            println!(
                "cave_buffers reuse={reuse} median_us={} p95_us={}",
                times[15], times[28]
            );
        }
        assert_eq!(visible, expected);
        let capacity = scratch.visited.capacity();
        fill_visible(
            SubChunkKey::new(1, 0, 0, 0),
            &graph,
            &mut scratch,
            &mut visible,
        );
        assert_eq!(visible, expected);
        assert_eq!(scratch.visited.capacity(), capacity);
    }
}
