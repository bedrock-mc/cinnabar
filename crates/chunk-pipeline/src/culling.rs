mod camera;
mod grid;
mod incremental;
mod visible_set;

use std::collections::HashSet;

use hashbrown::HashMap;

use meshing::Face;
use world::SubChunkKey;

pub(crate) use grid::ConnectivityGrid;
pub(crate) use incremental::update_visible;
pub use visible_set::CaveVisibleSet;

use grid::Slot;

const FACE_MASK: u64 = 0x3f;
/// Per-node state: bits 0..6 are exits already explored, this bit marks the node reached.
const REACHED: u8 = 1 << 6;

/// Conservative face-connectivity BFS used before Bevy's per-entity frustum culling.
#[must_use]
pub(crate) fn cave_visible_sub_chunks(
    camera: SubChunkKey,
    connectivity: &ConnectivityGrid,
) -> HashSet<SubChunkKey> {
    let mut visible = CaveVisibleSet::default();
    fill_visible(
        camera,
        connectivity,
        &mut CaveVisibilityScratch::default(),
        &mut visible,
    );
    visible.iter().collect()
}

/// Deterministic work performed by the latest cave visibility update.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaveVisibilityWork {
    pub explored_exits: usize,
    pub proof_exits: usize,
    pub additions: usize,
    pub rebuilt: bool,
}

/// Reusable traversal state, retained while graph additions preserve existing paths.
#[derive(Default)]
pub struct CaveVisibilityScratch {
    visited: Vec<u8>,
    touched: Vec<u32>,
    stack: Vec<(u32, u8)>, // Node and the exits it still has to explore.
    overflow_ids: HashMap<SubChunkKey, u32>,
    overflow_nodes: Vec<(SubChunkKey, u64)>,
    overflow_visited: Vec<u8>,
    camera: Option<SubChunkKey>,
    checkpoint: (u64, u64, usize),
    camera_present: bool,
    added_visible: Vec<SubChunkKey>,
    work: CaveVisibilityWork,
}

impl CaveVisibilityScratch {
    /// Work counters exclude unchanged frames skipped by the caller.
    pub fn work(&self) -> CaveVisibilityWork {
        self.work
    }

    /// Dense id for `key`: its grid cell, or a slot past the cells for an overflow key.
    fn node(&mut self, grid: &ConnectivityGrid, key: SubChunkKey) -> Option<(u32, u64)> {
        match grid.slot(key) {
            Slot::Cell(index, bits) => Some((index, bits)),
            Slot::Overflow(value) => {
                let cells = grid.cell_count() as u32;
                let next = cells + self.overflow_nodes.len() as u32;
                let id = *self.overflow_ids.entry(key).or_insert(next);
                if id == next {
                    self.overflow_nodes.push((key, value.bits()));
                    self.overflow_visited.push(0);
                }
                Some((id, value.bits()))
            }
            Slot::Missing => None,
        }
    }

    /// Reads a resident node through its dense or overflow identity.
    fn describe(&self, grid: &ConnectivityGrid, node: u32) -> (SubChunkKey, u64) {
        let cells = grid.cell_count() as u32;
        if node < cells {
            grid.cell(node)
        } else {
            self.overflow_nodes[(node - cells) as usize]
        }
    }

    /// Reaches `node` able to leave through `exits`, queueing whichever were not yet explored.
    fn reach(&mut self, cells: u32, node: u32, exits: u64) {
        let state = if node < cells {
            &mut self.visited[node as usize]
        } else {
            &mut self.overflow_visited[(node - cells) as usize]
        };
        if *state == 0 {
            self.touched.push(node);
        }
        let fresh = exits as u8 & !*state & FACE_MASK as u8;
        *state |= REACHED | fresh;
        if fresh != 0 {
            self.stack.push((node, fresh));
        }
    }

    /// Newly visible keys from the latest incremental update, without scanning resident keys.
    pub fn added_visible(&self) -> &[SubChunkKey] {
        &self.added_visible
    }
}

/// Reuses traversal and output storage without changing portal or support-shell rules.
pub(crate) fn fill_visible(
    camera: SubChunkKey,
    grid: &ConnectivityGrid,
    scratch: &mut CaveVisibilityScratch,
    visible: &mut CaveVisibleSet,
) {
    for &node in &scratch.touched {
        if (node as usize) < scratch.visited.len() {
            scratch.visited[node as usize] = 0;
        }
    }
    scratch.touched.clear();
    scratch.added_visible.clear();
    scratch.camera = Some(camera);
    scratch.checkpoint = grid.checkpoint();
    scratch.camera_present = grid.contains_key(&camera);
    scratch.work = CaveVisibilityWork {
        rebuilt: true,
        ..Default::default()
    };
    visible.reset(camera, grid.dims());
    if scratch.visited.len() != grid.cell_count() {
        scratch.visited.clear();
        scratch.visited.resize(grid.cell_count(), 0);
    }
    scratch.overflow_ids.clear();
    scratch.overflow_nodes.clear();
    scratch.overflow_visited.clear();
    scratch.stack.clear();
    let Some((camera_node, camera_bits)) = scratch.node(grid, camera) else {
        for key in grid.keys() {
            visible.insert(key);
        }
        return;
    };
    let cells = grid.cell_count() as u32;
    scratch.reach(cells, camera_node, touched_faces(camera_bits));
    propagate(grid, scratch);
    // Visibility is per entity: retain exactly one loaded neighbour shell around reached nodes.
    for index in 0..scratch.touched.len() {
        let node = scratch.touched[index];
        let (key, _) = scratch.describe(grid, node);
        visible.insert(key);
        for face in Face::ALL {
            if let Some(neighbour) = adjacent(key, face)
                && grid.contains_key(&neighbour)
            {
                visible.insert(neighbour);
            }
        }
    }
}

/// Explores each newly reachable exit once; retained exits need no repeated traversal.
fn propagate(grid: &ConnectivityGrid, scratch: &mut CaveVisibilityScratch) {
    let cells = grid.cell_count() as u32;
    // Leaving through an exit enters the neighbour by the opposite face whatever the entry
    // was, so each (node, exit) needs exploring once; the reached set is order-independent.
    while let Some((node, exits)) = scratch.stack.pop() {
        let (key, _) = scratch.describe(grid, node);
        let mut exits = u64::from(exits);
        while exits != 0 {
            scratch.work.explored_exits += 1;
            let exit = Face::ALL[exits.trailing_zeros() as usize];
            exits &= exits - 1;
            let Some(next) = adjacent(key, exit) else {
                continue;
            };
            let Some((next_node, next_bits)) = scratch.node(grid, next) else {
                continue;
            };
            let entered = opposite(exit) as u64;
            scratch.reach(cells, next_node, (next_bits >> (entered * 6)) & FACE_MASK);
        }
    }
}

/// The camera node may leave through any face its own air touches: the matrix diagonal.
const fn touched_faces(bits: u64) -> u64 {
    let mut faces = 0;
    let mut face = 0;
    while face < 6 {
        faces |= ((bits >> (face * 7)) & 1) << face;
        face += 1;
    }
    faces
}

/// Steps to a face neighbor without overflowing world coordinates.
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

/// Maps an exit to the neighboring sub-chunk entry face.
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
mod camera_tests;
#[cfg(test)]
mod tests;
