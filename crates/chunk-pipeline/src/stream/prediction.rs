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
    /// Services bounded worker results after input without admitting newer server events.
    pub fn poll_prediction_jobs(&mut self, camera: [f32; 3], budget: usize) -> WorldStreamPoll {
        let mut report = WorldStreamPoll::default();
        for _ in 0..budget {
            let Ok(completion) = self.lighting.rx.try_recv() else {
                break;
            };
            self.accept_light_completion(completion);
            report.light_results += 1;
        }
        report.light_jobs_dispatched = self.dispatch_light_jobs(camera, budget);
        for _ in 0..budget {
            if self.mesh_changes.len() >= MAX_PENDING_MESH_CHANGES {
                break;
            }
            let Ok(completion) = self.mesh_rx.try_recv() else {
                break;
            };
            self.accept_mesh_completion(completion);
            report.mesh_results += 1;
        }
        report.mesh_jobs_dispatched = self.dispatch_mesh_jobs_with_limits(camera, budget, budget);
        report
    }

    /// Returns the current local mutation generation for a loaded placement cell.
    pub fn prediction_generation(&self, position: [i32; 3]) -> Option<(SubChunkKey, u64)> {
        let key = SubChunkKey::new(
            self.current_dimension(),
            position[0].div_euclid(16),
            position[1].div_euclid(16),
            position[2].div_euclid(16),
        );
        Some((key, self.revisions.dirty(key)?.revision))
    }

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
        self.predict_blocks(&[(position, layer, block_id)])
    }

    /// Commits all placement cells together; any unavailable cell rejects the entire prediction.
    pub fn predict_blocks(&mut self, cells: &[([i32; 3], u32, u32)]) -> bool {
        if cells.is_empty() {
            return false;
        }
        let mut updates = std::collections::BTreeMap::<_, Vec<_>>::new();
        for &(position, layer, block_id) in cells {
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
            updates.entry(key).or_default().push(update);
        }
        let mut prepared = Vec::with_capacity(updates.len());
        for (&key, blocks) in &updates {
            let previous = self.authority.terrain().sub_chunk(key);
            let Ok(mutation) = ChunkStore::prepare_sub_chunk_blocks(
                key,
                previous.as_deref(),
                blocks,
                self.classifier.air_network_id(),
            ) else {
                return false;
            };
            prepared.push(mutation);
        }
        if !self.commit_block_mutations(prepared) {
            return false;
        }
        if self.order.blocking_block_updates().is_some() {
            self.predictions
                .0
                .extend(updates.iter().flat_map(|(&key, blocks)| {
                    blocks.iter().cloned().map(move |update| (key, update))
                }));
        }
        let camera = cells[0].0.map(|coordinate| coordinate as f32 + 0.5);
        // Start the bounded worker path now rather than waiting for the next world poll.
        self.dispatch_light_jobs(camera, updates.len());
        self.dispatch_mesh_jobs_with_limits(camera, updates.len(), updates.len());
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

    /// Replays a single deferred cell over a server batch that preceded the local action.
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
