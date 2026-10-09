//! Per-column scan results, rebuilt only when the column's blocks or block entities change.

use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

use assets::BlockEntityRouteKind;
use world::{BlockEntityKey, BlockEntityNbt, Chunk, SubChunk};

use super::{super::describe::Template, portals::PortalCell};

/// A column's portal cells and drawable block entities, valid while its maps are unchanged.
pub(super) struct ColumnScan {
    sub_chunks: Weak<BTreeMap<i32, Arc<SubChunk>>>,
    block_entities: Weak<BTreeMap<BlockEntityKey, Arc<BlockEntityNbt>>>,
    pub(super) portals: Vec<PortalCell>,
    /// Block entities routed to a model or text overlay, in key order.
    pub(super) entities: Vec<ColumnEntity>,
    /// The frame that last used this scan; older scans are dropped after each frame.
    pub(super) seen_frame: u64,
}

/// One routed block entity and the template its NBT and block describe.
pub(super) struct ColumnEntity {
    pub(super) key: BlockEntityKey,
    nbt: Arc<BlockEntityNbt>,
    runtime_id: u32,
    pub(super) template: Option<Template>,
}

impl ColumnScan {
    /// Records `chunk`'s current maps so a later edit to either invalidates this scan.
    pub(super) fn new(
        chunk: &Chunk,
        portals: Vec<PortalCell>,
        entities: Vec<ColumnEntity>,
    ) -> Self {
        Self {
            sub_chunks: Arc::downgrade(&chunk.shared_sub_chunks()),
            block_entities: Arc::downgrade(&chunk.shared_block_entities()),
            portals,
            entities,
            seen_frame: 0,
        }
    }

    /// Whether `chunk` still holds the maps this scan was built from.
    pub(super) fn is_current(&self, chunk: &Chunk) -> bool {
        same(&self.sub_chunks, &chunk.shared_sub_chunks())
            && same(&self.block_entities, &chunk.shared_block_entities())
    }
}

/// Column rescans one frame may run beyond the player's own columns. Joining a lobby streams
/// in every nearby column at once, and each rescan reads all of a column's sub-chunk palettes
/// and new block entities.
pub(super) const MAX_COLUMN_RESCANS_PER_FRAME: usize = 8;
/// Columns within this many columns of the eye's rescan on every change outside the budget,
/// so blocks the player edits within reach never wait for streaming elsewhere.
const NEAR_COLUMN_REACH: i32 = 1;

/// Whether column (`chunk_x`, `chunk_z`) is one of the eye column's near neighbours.
pub(super) fn is_near_column(eye_column: [i32; 2], chunk_x: i32, chunk_z: i32) -> bool {
    (chunk_x - eye_column[0]).abs() <= NEAR_COLUMN_REACH
        && (chunk_z - eye_column[1]).abs() <= NEAR_COLUMN_REACH
}

/// This frame's scan of `chunk`: the cached scan while current, a fresh one from `rescan` for a
/// near column or while the frame's budget lasts, else the stale previous scan until a later
/// frame rescans it, or `None` for a column not yet scanned.
pub(super) fn frame_scan(
    previous: Option<ColumnScan>,
    chunk: &Chunk,
    near: bool,
    rescans_left: &mut usize,
    rescan: impl FnOnce(Option<ColumnScan>) -> ColumnScan,
) -> Option<ColumnScan> {
    match previous {
        Some(scan) if scan.is_current(chunk) => Some(scan),
        previous if near => Some(rescan(previous)),
        previous if *rescans_left > 0 => {
            *rescans_left -= 1;
            Some(rescan(previous))
        }
        stale => stale,
    }
}

/// An edit replaces the map or detaches its weak references, so either check sees it.
fn same<T>(weak: &Weak<T>, current: &Arc<T>) -> bool {
    weak.upgrade()
        .is_some_and(|previous| Arc::ptr_eq(&previous, current))
}

/// `chunk`'s block entities that route to a model or text overlay and sit on a loaded block.
/// An entity whose NBT and block are unchanged keeps its template from `previous`;
/// `describe` builds the rest from `(id, runtime id, NBT, block position)`.
pub(super) fn routed_entities(
    chunk: &Chunk,
    previous: Option<ColumnScan>,
    mut describe: impl FnMut(&str, u32, &BlockEntityNbt, [i32; 3]) -> Option<Template>,
) -> Vec<ColumnEntity> {
    let mut previous = previous.map_or_else(Vec::new, |scan| scan.entities);
    let sub_chunks = chunk.shared_sub_chunks();
    let mut entities = Vec::new();
    for (key, nbt) in chunk.shared_block_entities().iter() {
        let Some(id) = nbt.id() else {
            continue;
        };
        if !matches!(
            assets::block_entity_route(id),
            Some(BlockEntityRouteKind::Model | BlockEntityRouteKind::TextOverlay)
        ) {
            continue;
        }
        let [x, y, z] = key.position();
        let Some(runtime_id) = sub_chunks.get(&key.sub_chunk().y).and_then(|sub_chunk| {
            sub_chunk.runtime_id(0, (x & 15) as u8, (y & 15) as u8, (z & 15) as u8)
        }) else {
            continue;
        };
        let unchanged = previous
            .binary_search_by(|entity| entity.key.cmp(key))
            .ok()
            .filter(|&index| {
                Arc::ptr_eq(&previous[index].nbt, nbt) && previous[index].runtime_id == runtime_id
            });
        let template = match unchanged {
            Some(index) => previous[index].template.take(),
            None => describe(id, runtime_id, nbt, [x, y, z]),
        };
        entities.push(ColumnEntity {
            key: *key,
            nbt: Arc::clone(nbt),
            runtime_id,
            template,
        });
    }
    entities
}

#[cfg(test)]
#[path = "columns_tests.rs"]
mod tests;
