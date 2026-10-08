use super::*;

/// Extends `visible` for additions, or fills `replacement` and returns true for a full rebuild.
pub(crate) fn update_visible(
    camera: SubChunkKey,
    grid: &ConnectivityGrid,
    scratch: &mut CaveVisibilityScratch,
    visible: &mut CaveVisibleSet,
    replacement: &mut CaveVisibleSet,
) -> bool {
    let checkpoint = grid.checkpoint();
    if scratch.checkpoint.0 != checkpoint.0
        || scratch.checkpoint.1 != checkpoint.1
        || !grid.retains_additions(scratch.checkpoint.2)
        || scratch.camera_present != grid.contains_key(&camera)
    {
        fill_visible(camera, grid, scratch, replacement);
        return true;
    }
    scratch.added_visible.clear();
    scratch.work = CaveVisibilityWork::default();
    let additions = grid.additions_since(scratch.checkpoint.2);
    scratch.work.additions = additions.len();
    let previously_reached = scratch.touched.len();
    for &key in additions {
        if scratch.camera_present {
            seed_addition(key, grid, scratch);
        } else {
            scratch.added_visible.push(key);
        }
    }
    propagate(grid, scratch);
    if scratch.camera != Some(camera)
        && scratch.camera_present
        && !camera::same_region(camera, grid, scratch)
    {
        let attempted = scratch.work;
        fill_visible(camera, grid, scratch, replacement);
        scratch.work.explored_exits += attempted.explored_exits;
        scratch.work.proof_exits = attempted.proof_exits;
        scratch.work.additions = attempted.additions;
        return true;
    }
    scratch.camera = Some(camera);
    // Defer publication until the proof succeeds so a fallback retains the old output for diffing.
    scratch.added_visible.retain(|key| visible.insert(*key));
    // Only newly reached nodes need their shell published. Shell nodes never seed traversal.
    for index in previously_reached..scratch.touched.len() {
        let (key, _) = scratch.describe(grid, scratch.touched[index]);
        insert_visible(key, scratch, visible);
        for face in Face::ALL {
            if let Some(neighbour) = adjacent(key, face)
                && grid.contains_key(&neighbour)
            {
                insert_visible(neighbour, scratch, visible);
            }
        }
    }
    scratch.checkpoint = checkpoint;
    false
}

/// Replays only neighboring exits that could not cross into this previously absent node.
fn seed_addition(key: SubChunkKey, grid: &ConnectivityGrid, scratch: &mut CaveVisibilityScratch) {
    let Some((node, bits)) = scratch.node(grid, key) else {
        return;
    };
    for face in Face::ALL {
        let Some(neighbour) = adjacent(key, face) else {
            continue;
        };
        let state = reached_state(neighbour, grid, scratch);
        if state & REACHED != 0 {
            scratch.added_visible.push(key);
        }
        if state & (1 << opposite(face) as u8) != 0 {
            scratch.reach(
                grid.cell_count() as u32,
                node,
                (bits >> (face as u64 * 6)) & FACE_MASK,
            );
        }
    }
}

/// Reads retained traversal state without assigning IDs to unrelated overflow nodes.
pub(super) fn reached_state(
    key: SubChunkKey,
    grid: &ConnectivityGrid,
    scratch: &CaveVisibilityScratch,
) -> u8 {
    match grid.slot(key) {
        Slot::Cell(node, _) => scratch.visited[node as usize],
        Slot::Overflow(_) => scratch.overflow_ids.get(&key).map_or(0, |node| {
            scratch.overflow_visited[*node as usize - grid.cell_count()]
        }),
        Slot::Missing => 0,
    }
}

/// Records each newly visible key once for publication to rendered entities.
fn insert_visible(
    key: SubChunkKey,
    scratch: &mut CaveVisibilityScratch,
    visible: &mut CaveVisibleSet,
) {
    if visible.insert(key) {
        scratch.added_visible.push(key);
    }
}
