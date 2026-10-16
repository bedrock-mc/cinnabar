use super::*;

impl WorldStream {
    pub(super) fn accept_decode_completion(&mut self, completion: DecodeCompletion) {
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("decode.completion").entered();
        self.stats.publication_stages.decode_jobs_completed = self
            .stats
            .publication_stages
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
                self.order.announce_applied();
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
    /// Starts decode work queued by ingress now rather than at the next poll, so a fast decode
    /// can commit in the same frame's poll.
    pub fn dispatch_ingress_decode(&mut self) {
        if !self.pending_decode.is_empty() {
            self.dispatch_decode_jobs();
        }
    }

    pub(super) fn dispatch_decode_jobs(&mut self) {
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("decode.dispatch").entered();
        let budget = DECODE_DISPATCH_BUDGET_PER_POLL
            .min(MAX_IN_FLIGHT_DECODE_JOBS.saturating_sub(self.in_flight_decode_jobs));
        // Enqueueing is count-bounded; spent commit time must not idle the decode lane.
        for _ in 0..budget {
            if self.service_yield_requested() {
                break;
            }
            let Some(QueuedDecodeJob { queued_at, job }) = self.pending_decode.pop_front() else {
                break;
            };
            self.in_flight_decode_jobs += 1;
            self.stats.publication_stages.decode_jobs_dispatched = self
                .stats
                .publication_stages
                .decode_jobs_dispatched
                .saturating_add(1);
            let tx = self.decode_tx.clone();
            workers::WORKERS.spawn(workers::Lane::Decode, move || {
                #[cfg(feature = "tracy")]
                let _zone = tracing::info_span!("decode.work").entered();
                let completion = job.run(queued_at);
                #[cfg(feature = "tracy")]
                drop(_zone);
                #[cfg(feature = "tracy")]
                let _zone =
                    tracing::info_span!("decode.completion_send", sequence = completion.sequence)
                        .entered();
                workers::send_result(&tx, completion);
            });
        }
    }

    /// Snapshots the final burst and earlier packet states only where cracks need reconciliation.
    pub(super) fn snapshot_synced_block_mutation_batches(
        &mut self,
        mut packets: Vec<Vec<SyncedBlockUpdateEvent>>,
    ) -> (
        Vec<BlockMutationBatch>,
        Vec<SyncedBlockUpdateEvent>,
        Vec<BlockMutationBatch>,
    ) {
        let last_packet = packets.len().saturating_sub(1);
        let mut crack_batches = Vec::new();
        for (index, events) in packets.iter_mut().enumerate() {
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
            if index < last_packet
                && events.first().is_some_and(|event| {
                    split_block_update(event.update)
                        .is_ok_and(|(key, _)| self.block_crack_section_active(key))
                })
            {
                // Coalesced packets all address the same section.
                let updates = events.iter().map(|event| event.update).collect();
                crack_batches.extend(self.snapshot_block_mutation_batches(updates));
            }
        }
        let events = packets.into_iter().flatten().collect::<Vec<_>>();
        let batches =
            self.snapshot_block_mutation_batches(events.iter().map(|event| event.update).collect());
        (batches, events, crack_batches)
    }
}

impl WorldStream {
    /// Sequential ids of this session's server-defined blocks, which decode as known.
    pub fn set_custom_block_ids(&mut self, ids: std::ops::Range<u32>) {
        self.authority.set_custom_block_ids(ids);
    }

    /// Installs session block state identities before terrain admission or visual pack compilation.
    pub fn set_custom_block_identities(
        &mut self,
        definitions: &client_world::ingestion::CustomBlocks,
    ) {
        self.authority.set_custom_block_identities(definitions);
    }

    /// Installs identities already prepared against this stream's palette mode and custom range.
    pub fn install_custom_block_identities(
        &mut self,
        identities: client_world::PreparedBlockIdentities,
    ) {
        self.authority.install_custom_block_identities(identities);
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
