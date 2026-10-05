use super::*;

impl WorldStream {
    pub(super) fn accept_decode_completion(&mut self, completion: DecodeCompletion) {
        self.stats.phase2_stages.decode_jobs_completed = self
            .stats
            .phase2_stages
            .decode_jobs_completed
            .saturating_add(1);
        self.in_flight_decode_jobs = self.in_flight_decode_jobs.saturating_sub(1);
        self.stats.observe_decode_queue_wait(completion.queue_wait);
        match self
            .order
            .complete_decode(completion.sequence, completion.event)
        {
            Ok(DecodeCommit::Queued) => {}
            Ok(DecodeCommit::BlockUpdates(event)) => {
                self.apply_prepared(event);
                self.reapply_deferred_predictions();
                if !self.polling {
                    self.apply_ready();
                }
            }
            Err(_) => self
                .record_normalization_error(NormalizationErrorReason::OrderedCompletionRejection),
        }
    }
    pub(super) fn snapshot_block_mutation_batches(
        &mut self,
        events: Vec<BlockUpdateEvent>,
    ) -> Vec<BlockMutationBatch> {
        let mut grouped = BTreeMap::<SubChunkKey, Vec<BlockUpdate>>::new();
        for event in events {
            match split_block_update(event) {
                Ok((key, update)) if self.column_is_data_interesting(key.chunk()) => {
                    grouped.entry(key).or_default().push(update);
                }
                Ok(_) => {
                    self.record_normalization_error(NormalizationErrorReason::InactiveBlockUpdate)
                }
                Err(_) => {
                    self.record_normalization_error(NormalizationErrorReason::MalformedBlockUpdate)
                }
            }
        }
        grouped
            .into_iter()
            .map(|(key, updates)| BlockMutationBatch {
                key,
                previous: self.authority.terrain().sub_chunk(key),
                updates,
            })
            .collect()
    }
    pub(super) fn dispatch_decode_jobs(&mut self) {
        let budget = DECODE_DISPATCH_BUDGET_PER_POLL
            .min(MAX_IN_FLIGHT_DECODE_JOBS.saturating_sub(self.in_flight_decode_jobs));
        // Enqueueing is count-bounded; spent commit time must not idle the decode lane.
        for _ in 0..budget {
            let Some(QueuedDecodeJob { queued_at, job }) = self.pending_decode.pop_front() else {
                break;
            };
            self.in_flight_decode_jobs += 1;
            self.stats.phase2_stages.decode_jobs_dispatched = self
                .stats
                .phase2_stages
                .decode_jobs_dispatched
                .saturating_add(1);
            let tx = self.decode_tx.clone();
            workers::WORKERS.decode.spawn(move || {
                let completion = job.run(queued_at);
                let _ = tx.send(completion);
            });
        }
    }

    pub(super) fn snapshot_synced_block_mutation_batches(
        &mut self,
        mut events: Vec<SyncedBlockUpdateEvent>,
    ) -> (Vec<BlockMutationBatch>, Vec<SyncedBlockUpdateEvent>) {
        events.retain(|event| match split_block_update(event.update) {
            Ok((key, _))
                if event.update.layer <= 1 && self.column_is_data_interesting(key.chunk()) =>
            {
                true
            }
            Ok(_) => {
                self.record_normalization_error(NormalizationErrorReason::InactiveBlockUpdate);
                false
            }
            Err(_) => {
                self.record_normalization_error(NormalizationErrorReason::MalformedBlockUpdate);
                false
            }
        });
        let batches =
            self.snapshot_block_mutation_batches(events.iter().map(|event| event.update).collect());
        (batches, events)
    }
}

impl WorldStream {
    /// Sequential ids of this session's server-defined blocks, which decode as known.
    pub fn set_custom_block_ids(&mut self, ids: std::ops::Range<u32>) {
        self.authority.set_custom_block_ids(ids);
    }

    /// Translates sequential wire ids when custom blocks sort among vanilla names.
    pub fn set_sequential_id_remap(&mut self, remap: assets::SequentialIdRemap) {
        self.authority.set_sequential_id_remap(remap);
    }

    /// Captures registry identities from the authoritative session owner.
    pub(super) fn decode_ids(&self, dimension: i32) -> DecodeIds {
        self.authority.decode_ids(dimension)
    }
}
