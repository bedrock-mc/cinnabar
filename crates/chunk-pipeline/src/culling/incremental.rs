use super::{grid::GraphChange, *};

/// A superset left by removals or lost connections is replaced by a full traversal once it has
/// served this many updates, so exact visibility returns within a bounded number of frames.
pub(super) const SUPERSET_UPDATE_LIMIT: u32 = 16;

/// Extends `visible` for graph changes, or fills `replacement` and returns true for a full
/// rebuild. Additions and gained connections extend the retained traversal exactly. Removals
/// and lost connections can only shrink what is reachable, so `visible` stays a superset of the
/// exact result until a rebuild at most [`SUPERSET_UPDATE_LIMIT`] updates later, or at the next
/// camera section change, whichever comes first.
pub(crate) fn update_visible(
    camera: SubChunkKey,
    grid: &ConnectivityGrid,
    scratch: &mut CaveVisibilityScratch,
    visible: &mut CaveVisibleSet,
    replacement: &mut CaveVisibleSet,
) -> bool {
    let checkpoint = grid.checkpoint();
    // Under Sodium's rule the reached set depends on the camera's section itself, so only
    // graph additions seen from the same section extend it.
    if scratch.outward_only != scratch.built_outward_only
        || (scratch.outward_only && scratch.camera != Some(camera))
        || scratch.checkpoint.0 != checkpoint.0
        || scratch.checkpoint.1 != checkpoint.1
        || !grid.retains_changes(scratch.checkpoint.2)
        || scratch.camera_present != grid.contains_key(&camera)
    {
        fill_visible(camera, grid, scratch, replacement);
        return true;
    }
    scratch.added_visible.clear();
    scratch.work = CaveVisibilityWork::default();
    let previously_reached = scratch.touched.len();
    // Removals and promotions first bring every slot's state up to its current occupant, so
    // openings then seed against the current layout whatever order the changes came in.
    for change in grid.changes_since(scratch.checkpoint.2) {
        match *change {
            GraphChange::Opened(_) => {}
            GraphChange::Narrowed => scratch.mark_superset(),
            GraphChange::Removed(key, cell) => {
                scratch.forget(grid, key, cell);
                scratch.mark_superset();
            }
            GraphChange::Promoted(key, cell) => scratch.promote(grid, key, cell),
        }
    }
    for change in grid.changes_since(scratch.checkpoint.2) {
        let GraphChange::Opened(key) = *change else {
            continue;
        };
        scratch.work.additions += 1;
        if scratch.camera_present {
            seed_opened(key, grid, scratch);
        } else {
            scratch.added_visible.push(key);
        }
    }
    propagate(grid, scratch);
    let superset_updates = scratch.superset_updates.map(|updates| updates + 1);
    scratch.superset_updates = superset_updates;
    // A superset traversal cannot prove that a new camera section reaches the same region.
    let camera_moved = scratch.camera != Some(camera)
        && scratch.camera_present
        && (superset_updates.is_some() || !camera::same_region(camera, grid, scratch));
    if camera_moved || superset_updates.is_some_and(|updates| updates >= SUPERSET_UPDATE_LIMIT) {
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
        if scratch.built_outward_only {
            continue;
        }
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

/// Replays only the exits that could not cross into this node before it joined the graph or
/// gained connections: those entering from reached neighbours, and the camera's own.
fn seed_opened(key: SubChunkKey, grid: &ConnectivityGrid, scratch: &mut CaveVisibilityScratch) {
    let Some((node, bits)) = scratch.node(grid, key) else {
        return;
    };
    let cells = grid.cell_count() as u32;
    if scratch.camera == Some(key) {
        scratch.reach(cells, node, touched_faces(bits));
    }
    for face in Face::ALL {
        let Some(neighbour) = adjacent(key, face) else {
            continue;
        };
        let state = reached_state(neighbour, grid, scratch);
        if state & REACHED != 0 && !scratch.built_outward_only {
            // Next to a reached node: part of its shell.
            scratch.added_visible.push(key);
        }
        if state & (1 << opposite(face) as u8) != 0 {
            let exits = (bits >> (face as u64 * 6)) & FACE_MASK & scratch.allowed_exits(key);
            scratch.reach(cells, node, exits);
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
