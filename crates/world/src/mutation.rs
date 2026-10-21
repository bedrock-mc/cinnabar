use crate::{MAX_STORAGE_COUNT, MutationError};

/// One packet-neutral block change inside a 16x16x16 sub-chunk.
///
/// `layer` maps directly to `UpdateBlock.Layer`. Callers handling an
/// `UpdateSubChunkBlocks` packet use layer zero for `Blocks` and layer one for
/// `Extra`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockUpdate {
    pub x: u8,
    pub y: u8,
    pub z: u8,
    pub layer: u32,
    /// Raw network runtime ID or block-state hash.
    pub runtime_id: u32,
}

/// Which neighbouring sub-chunks a set of changed cells can affect. Light reads one cell across a
/// face, edge or corner. Meshes read one cell for face culling, ambient occlusion, smooth light,
/// connected shapes and liquid heights, and two for leaf depth, which checks a neighbouring
/// leaf's own neighbours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeighbourReach {
    light: u32,
    mesh: u32,
}

impl NeighbourReach {
    /// Every neighbour, for changes whose cells are unknown.
    pub const ALL: Self = Self {
        light: (1 << 27) - 1,
        mesh: (1 << 27) - 1,
    };

    /// The neighbours `updates` reach.
    #[must_use]
    pub fn of_updates(updates: &[BlockUpdate]) -> Self {
        updates
            .iter()
            .fold(Self { light: 0, mesh: 0 }, |reach, update| {
                reach.union(Self::of_cell(update.x, update.y, update.z))
            })
    }

    /// The neighbours one cell at local `x`, `y`, `z` reaches.
    #[must_use]
    pub fn of_cell(x: u8, y: u8, z: u8) -> Self {
        Self {
            light: offsets_within([x, y, z], 0),
            mesh: offsets_within([x, y, z], 1),
        }
    }

    /// Combines the reach of two sets of changed cells.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self {
            light: self.light | other.light,
            mesh: self.mesh | other.mesh,
        }
    }

    /// Whether the light of the neighbour at sub-chunk `offset` reads the changed cells; offsets
    /// beyond one sub-chunk count as reached.
    #[must_use]
    pub const fn light_reaches(self, offset: [i64; 3]) -> bool {
        offset_bit(self.light, offset)
    }

    /// Whether the mesh of the neighbour at sub-chunk `offset` reads the changed cells; offsets
    /// beyond one sub-chunk count as reached.
    #[must_use]
    pub const fn mesh_reaches(self, offset: [i64; 3]) -> bool {
        offset_bit(self.mesh, offset)
    }
}

/// Neighbour offsets, one bit each, of a cell within `margin` cells of each boundary it nears.
fn offsets_within(cell: [u8; 3], margin: u8) -> u32 {
    // Offsets -1, 0 and +1 along each axis the cell's position allows.
    let [ax, ay, az] = cell.map(|c| [c <= margin, true, c >= 15 - margin]);
    let mut bits = 0;
    for (dx, reaches_x) in ax.into_iter().enumerate() {
        for (dy, reaches_y) in ay.into_iter().enumerate() {
            for (dz, reaches_z) in az.into_iter().enumerate() {
                if reaches_x && reaches_y && reaches_z {
                    bits |= 1 << (dx * 9 + dy * 3 + dz);
                }
            }
        }
    }
    bits
}

const fn offset_bit(bits: u32, offset: [i64; 3]) -> bool {
    let [dx, dy, dz] = offset;
    if dx < -1 || dx > 1 || dy < -1 || dy > 1 || dz < -1 || dz > 1 {
        return true;
    }
    let bit = (dx + 1) * 9 + (dy + 1) * 3 + (dz + 1);
    bits & (1 << bit) != 0
}

impl BlockUpdate {
    #[must_use]
    pub const fn new(x: u8, y: u8, z: u8, layer: u32, runtime_id: u32) -> Self {
        Self {
            x,
            y,
            z,
            layer,
            runtime_id,
        }
    }

    /// Rejects coordinates and layers outside one supported sub-chunk.
    pub fn validate(self) -> Result<(), MutationError> {
        if self.x >= 16 || self.y >= 16 || self.z >= 16 {
            return Err(MutationError::LocalCoordinatesOutOfBounds {
                x: self.x,
                y: self.y,
                z: self.z,
            });
        }
        if self.layer >= MAX_STORAGE_COUNT as u32 {
            return Err(MutationError::LayerOutOfBounds {
                layer: self.layer,
                max: MAX_STORAGE_COUNT,
            });
        }
        Ok(())
    }
}
