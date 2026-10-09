//! Portal surfaces follow the primary block palette, including chunks without NBT.

use bevy::prelude::Vec3;
use render::{BlockEntityKind, BlockEntitySubmission};
use world::{Chunk, ChunkKey, SUB_CHUNK_SIDE, SubChunk};

/// One portal or gateway surface cell.
pub(super) type PortalCell = ([i32; 3], BlockEntityKind);

/// Every portal cell of `chunk`, by ascending sub-chunk and then x, z, y within each.
pub(super) fn column_cells(
    key: ChunkKey,
    chunk: &Chunk,
    mut classify: impl FnMut(u32) -> Option<BlockEntityKind>,
) -> Vec<PortalCell> {
    let mut cells = Vec::new();
    for (y, sub_chunk) in chunk.sub_chunks() {
        cells.extend(sub_chunk_cells(
            [key.x, y, key.z],
            &sub_chunk,
            &mut classify,
        ));
    }
    cells
}

/// The portal cells of the sub-chunk at `grid`, read from its primary palette.
fn sub_chunk_cells(
    grid: [i32; 3],
    sub_chunk: &SubChunk,
    classify: &mut impl FnMut(u32) -> Option<BlockEntityKind>,
) -> Vec<PortalCell> {
    let side = SUB_CHUNK_SIDE as i32;
    let [Some(bx), Some(by), Some(bz)] = grid.map(|value| value.checked_mul(side)) else {
        return Vec::new();
    };
    let Some(primary) = sub_chunk.storages().first() else {
        return Vec::new();
    };
    // Ordinary sub-chunks never need a per-block scan. Resolve just their small palette.
    let portals: Vec<_> = primary
        .palette()
        .values()
        .iter()
        .filter_map(|&id| classify(id).map(|kind| (id, kind)))
        .collect();
    if portals.is_empty() {
        return Vec::new();
    }
    let mut cells = Vec::new();
    for x in 0..SUB_CHUNK_SIDE as u8 {
        for z in 0..SUB_CHUNK_SIDE as u8 {
            for y in 0..SUB_CHUNK_SIDE as u8 {
                if let Some((_, kind)) = primary
                    .runtime_id(x, y, z)
                    .and_then(|id| portals.iter().find(|(portal_id, _)| *portal_id == id))
                {
                    let block = [bx + i32::from(x), by + i32::from(y), bz + i32::from(z)];
                    cells.push((block, kind.clone()));
                }
            }
        }
    }
    cells
}

/// Submits the `cells` within the scan radius of `eye`, stopping at the submission limit.
pub(super) fn submit(
    submissions: &mut Vec<BlockEntitySubmission>,
    cells: &[PortalCell],
    eye: Vec3,
) {
    for (block, kind) in cells {
        if submissions.len() >= super::MAX_SUBMISSIONS {
            return;
        }
        let center = Vec3::from_array(block.map(|value| value as f32 + 0.5));
        if center.distance_squared(eye) <= super::SCAN_RADIUS_BLOCKS * super::SCAN_RADIUS_BLOCKS {
            submissions.push(BlockEntitySubmission {
                block: *block,
                light: 1.0.into(),
                kind: kind.clone(),
            });
        }
    }
}

#[cfg(test)]
#[path = "portals_tests.rs"]
mod tests;
