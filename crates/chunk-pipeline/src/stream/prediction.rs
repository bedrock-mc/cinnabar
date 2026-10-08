//! Local block predictions. Each is committed to the store at once and is
//! replaced by any later authoritative update, as vanilla's local destroy and
//! placement are; the server's corrections are the only rollback.

use super::*;

/// Predictions made while a server batch prepared from older sub-chunks decodes.
#[derive(Debug, Default)]
pub(super) struct DeferredPredictions(Vec<(SubChunkKey, BlockUpdate)>);

impl DeferredPredictions {
    pub(super) fn begin_server_batch(&mut self) {
        self.0.clear();
    }
}

impl WorldStream {
    /// Encodes a store identity for the server's block palette.
    pub fn block_network_id(&self, internal_id: u32) -> Option<u32> {
        self.authority.block_network_id(internal_id)
    }

    /// The store id a wire block runtime id decodes to.
    pub fn resolve_block_network_id(&self, network_id: u32) -> u32 {
        BlockIds::resolve(
            &self.decode_ids(self.authority.current_dimension()),
            network_id,
        )
    }

    /// The store id of air.
    pub fn air_block_id(&self) -> u32 {
        self.classifier.air_network_id()
    }

    /// Sets one current-dimension block to a store id now; false when its
    /// sub-chunk holds no authoritative data.
    pub fn predict_block(&mut self, position: [i32; 3], layer: u32, block_id: u32) -> bool {
        let Ok((key, update)) = split_block_update(BlockUpdateEvent {
            dimension: self.authority.current_dimension(),
            position,
            layer: layer as usize,
            network_id: block_id,
        }) else {
            return false;
        };
        let in_range = self
            .authority
            .dimension_range(key.dimension)
            .is_some_and(|range| {
                (range.base_sub_chunk_y..range.base_sub_chunk_y + range.sub_chunk_count as i32)
                    .contains(&key.y)
            });
        if !in_range
            || !self.authority.terrain().is_sub_chunk_loaded(key)
            || !self.column_is_data_interesting(key.chunk())
        {
            return false;
        }
        if !self.commit_prediction(key, update) {
            return false;
        }
        if self.order.blocking_block_updates().is_some() {
            self.predictions.0.push((key, update));
        }
        self.dispatch_urgent_work();
        true
    }

    /// Restores deferred predictions over the committed server batch, which
    /// was received first and so is older than each of them.
    pub(super) fn reapply_deferred_predictions(&mut self) {
        for (key, update) in std::mem::take(&mut self.predictions.0) {
            if key.dimension == self.authority.current_dimension()
                && self.authority.terrain().is_sub_chunk_loaded(key)
            {
                self.commit_prediction(key, update);
            }
        }
    }

    fn commit_prediction(&mut self, key: SubChunkKey, update: BlockUpdate) -> bool {
        let previous = self.authority.terrain().sub_chunk(key);
        ChunkStore::prepare_sub_chunk_blocks(
            key,
            previous.as_deref(),
            &[update],
            self.classifier.air_network_id(),
        )
        .is_ok_and(|prepared| self.commit_block_mutations(vec![prepared]))
    }
}
