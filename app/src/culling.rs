use std::collections::{HashMap, HashSet, VecDeque};

use render::{Face, FaceConnectivity};
use world::SubChunkKey;

/// Conservative face-connectivity BFS used before Bevy's per-entity frustum culling.
#[must_use]
pub fn cave_visible_sub_chunks(
    camera: SubChunkKey,
    connectivity: &HashMap<SubChunkKey, FaceConnectivity>,
) -> HashSet<SubChunkKey> {
    if !connectivity.contains_key(&camera) {
        return connectivity.keys().copied().collect();
    }

    let mut visible = HashSet::from([camera]);
    let mut visited = HashSet::from([(camera, None)]);
    let mut queue = VecDeque::from([(camera, None)]);
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
            visible.insert(next);
            let state = (next, Some(opposite(exit)));
            if visited.insert(state) {
                queue.push_back(state);
            }
        }
    }
    visible
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
    use std::collections::HashMap;

    use render::FaceConnectivity;
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
    fn sealed_middle_subchunk_stops_connectivity_bfs() {
        let first = SubChunkKey::new(0, 0, 0, 0);
        let sealed = SubChunkKey::new(0, 1, 0, 0);
        let hidden = SubChunkKey::new(0, 2, 0, 0);
        let graph = HashMap::from([
            (first, FaceConnectivity::all()),
            (sealed, FaceConnectivity::none()),
            (hidden, FaceConnectivity::all()),
        ]);

        let visible = cave_visible_sub_chunks(first, &graph);
        assert!(visible.contains(&first));
        assert!(visible.contains(&sealed));
        assert!(!visible.contains(&hidden));
    }

    #[test]
    fn missing_camera_node_falls_back_to_conservative_visibility() {
        let camera = SubChunkKey::new(0, 99, 0, 99);
        let loaded = SubChunkKey::new(0, 0, 0, 0);
        let graph = HashMap::from([(loaded, FaceConnectivity::none())]);

        assert_eq!(cave_visible_sub_chunks(camera, &graph), [loaded].into());
    }
}
