//! Portal surfaces follow the primary block palette, including chunks without NBT.

use bevy::prelude::Vec3;
use render::{BlockEntityKind, BlockEntitySubmission};
use world::{Chunk, ChunkKey, SUB_CHUNK_SIDE, SubChunk};

pub(super) fn submit(
    submissions: &mut Vec<BlockEntitySubmission>,
    key: ChunkKey,
    chunk: &Chunk,
    eye: Vec3,
    mut classify: impl FnMut(u32) -> Option<BlockEntityKind>,
) {
    for (y, sub_chunk) in chunk.sub_chunks() {
        submit_sub_chunk(
            submissions,
            [key.x, y, key.z],
            &sub_chunk,
            eye,
            &mut classify,
        );
        if submissions.len() >= super::MAX_SUBMISSIONS {
            break;
        }
    }
}

fn submit_sub_chunk(
    submissions: &mut Vec<BlockEntitySubmission>,
    grid: [i32; 3],
    sub_chunk: &SubChunk,
    eye: Vec3,
    classify: &mut impl FnMut(u32) -> Option<BlockEntityKind>,
) {
    let side = SUB_CHUNK_SIDE as i32;
    let [Some(bx), Some(by), Some(bz)] = grid.map(|value| value.checked_mul(side)) else {
        return;
    };
    if by as f32 > eye.y + super::SCAN_RADIUS_BLOCKS
        || by as f32 + (side as f32) < eye.y - super::SCAN_RADIUS_BLOCKS
    {
        return;
    }
    let Some(primary) = sub_chunk.storages().first() else {
        return;
    };
    // Ordinary sub-chunks never need a per-block scan. Resolve just their small palette.
    let portals: Vec<_> = primary
        .palette()
        .values()
        .iter()
        .filter_map(|&id| classify(id).map(|kind| (id, kind)))
        .collect();
    if portals.is_empty() {
        return;
    }
    for x in 0..SUB_CHUNK_SIDE as u8 {
        for z in 0..SUB_CHUNK_SIDE as u8 {
            for y in 0..SUB_CHUNK_SIDE as u8 {
                if submissions.len() >= super::MAX_SUBMISSIONS {
                    return;
                }
                let Some((_, kind)) = primary
                    .runtime_id(x, y, z)
                    .and_then(|id| portals.iter().find(|(portal_id, _)| *portal_id == id))
                else {
                    continue;
                };
                let block = [bx + i32::from(x), by + i32::from(y), bz + i32::from(z)];
                let center = Vec3::from_array(block.map(|value| value as f32 + 0.5));
                if center.distance_squared(eye)
                    <= super::SCAN_RADIUS_BLOCKS * super::SCAN_RADIUS_BLOCKS
                {
                    submissions.push(BlockEntitySubmission {
                        block,
                        light: 1.0.into(),
                        kind: kind.clone(),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "portals_tests.rs"]
mod tests;
