//! Section-local exterior fill: which see-through voxels connect to the padded border or,
//! in sections that may be drawn over resident chunk columns, to a chunk-column plane
//! inside the section.
//!
//! Faces in front of voxels the fill misses belong to pockets sealed inside the section and
//! can only be seen from inside them. Resident chunks replace distant terrain by whole chunk
//! columns, so a cut along a chunk-column plane can open a pocket of a section drawn beside
//! them; both voxels next to every such plane are seeded there. Sections of other levels
//! are never drawn over resident columns ([`super::overlaps_resident_columns`]), so only
//! their border seeds.
//!
//! A padding voxel is open when anything a neighbour region may render in front of the
//! section there is see-through: the same-level voxel, the coarser voxel or any finer child
//! touching the face. Otherwise a pocket the same-level slab closes would lose its faces
//! although a finer or coarser neighbour opens it.

use world::SUB_CHUNK_SIDE;

use crate::{
    geometry::SECTION_SIDE,
    mesh::{
        overlaps_resident_columns,
        padded::{COLUMN_BITS, PAD},
    },
};

/// Section voxels between the chunk-column planes the fill seeds at `level`; `None` at
/// levels never drawn over resident columns, which seed none.
pub(super) fn plane_spacing(level: u8) -> Option<usize> {
    overlaps_resident_columns(level).then_some(SUB_CHUNK_SIDE >> level)
}

/// True when section-local `index` (along X or Z) lies next to a chunk-column plane inside
/// the section, `spacing` voxels apart.
fn next_to_plane(index: usize, spacing: usize) -> bool {
    let offset = index % spacing;
    (offset == 0 && index > 0) || (offset == spacing - 1 && index < SECTION_SIDE - 1)
}

/// Fills `reach` with the see-through voxels connected to the padding or to a seeded
/// chunk-column plane ([`plane_spacing`]), given one mask of closed voxels per padded column
/// (`closed`, padded as [`super::padded::Masks`]).
pub(super) fn fill(closed: &[u64], level: u8, reach: &mut [u64]) {
    const BORDER_X: u64 = 1 | 1 << (PAD - 1);
    let spacing = plane_spacing(level);
    let plane_x = (0..SECTION_SIDE)
        .filter(|&x| spacing.is_some_and(|spacing| next_to_plane(x, spacing)))
        .fold(0_u64, |bits, x| bits | 1 << (x + 1));
    let open = |column: usize| !closed[column] & COLUMN_BITS;
    for y in 0..PAD {
        for z in 0..PAD {
            let column = y * PAD + z;
            let border = y == 0 || z == 0 || y == PAD - 1 || z == PAD - 1;
            let plane_z = || spacing.is_some_and(|spacing| next_to_plane(z - 1, spacing));
            let seeds = if border || plane_z() {
                COLUMN_BITS
            } else {
                BORDER_X | plane_x
            };
            reach[column] = open(column) & seeds;
        }
    }
    // Sweep forwards then backwards over the interior columns until nothing grows.
    loop {
        let mut grew = false;
        for backwards in [false, true] {
            for step in 0..SECTION_SIDE * SECTION_SIDE {
                let step = if backwards {
                    SECTION_SIDE * SECTION_SIDE - 1 - step
                } else {
                    step
                };
                let column = (step / SECTION_SIDE + 1) * PAD + step % SECTION_SIDE + 1;
                let open = open(column);
                if open == 0 {
                    continue;
                }
                let around = reach[column]
                    | reach[column - PAD]
                    | reach[column + PAD]
                    | reach[column - 1]
                    | reach[column + 1];
                let mut spread = around & open;
                loop {
                    let next = (spread | spread << 1 | spread >> 1) & open;
                    if next == spread {
                        break;
                    }
                    spread = next;
                }
                if spread != reach[column] {
                    reach[column] = spread;
                    grew = true;
                }
            }
        }
        if !grew {
            return;
        }
    }
}
