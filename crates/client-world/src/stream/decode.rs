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
        if self.blocking_block_updates == Some(completion.sequence)
            && matches!(&completion.event, PreparedWorldEvent::BlockUpdates { .. })
        {
            self.blocking_block_updates = None;
            self.submitted.remove(&completion.sequence);
            self.heavy_sequences.remove(&completion.sequence);
            self.apply_prepared(completion.event);
            self.apply_ready();
            return;
        }
        if self
            .ordered
            .insert(completion.sequence, completion.event)
            .is_err()
        {
            self.heavy_sequences.remove(&completion.sequence);
            self.record_normalization_error(NormalizationErrorReason::OrderedCompletionRejection);
        }
    }
    pub(super) fn snapshot_block_mutation_batches(
        &mut self,
        events: Vec<BlockUpdateEvent>,
    ) -> Vec<BlockMutationBatch> {
        let mut grouped = BTreeMap::<SubChunkKey, Vec<BlockUpdate>>::new();
        for event in events {
            match split_block_update(event) {
                Ok((key, update)) if self.column_is_active(key.chunk()) => {
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
                previous: self.store.sub_chunk(key),
                updates,
            })
            .collect()
    }
    pub(super) fn dispatch_decode_jobs(&mut self) {
        let budget = DECODE_DISPATCH_BUDGET_PER_POLL
            .min(MAX_IN_FLIGHT_DECODE_JOBS.saturating_sub(self.in_flight_decode_jobs));
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
            rayon::spawn(move || {
                let started = Instant::now();
                let queue_wait = queue_wait(queued_at, started);
                let completion = match job {
                    DecodeJob::InlineLevelChunk {
                        sequence,
                        mut event,
                        base_sub_chunk_y,
                        count,
                        biome_storage_count,
                    } => {
                        let chunk = ChunkKey::new(event.dimension, event.x, event.z);
                        let payload = std::mem::take(&mut event.payload);
                        let decoded = DecodedLevelChunk::decode_with_biomes_and_block_entities(
                            chunk,
                            base_sub_chunk_y,
                            count,
                            base_sub_chunk_y,
                            biome_storage_count,
                            &payload,
                        );
                        DecodeCompletion {
                            sequence,
                            queue_wait,
                            event: PreparedWorldEvent::InlineLevelChunk {
                                event,
                                decoded,
                                duration: started.elapsed(),
                            },
                        }
                    }
                    DecodeJob::RequestLevelChunk {
                        sequence,
                        mut event,
                        biome_base_sub_chunk_y,
                        biome_storage_count,
                    } => {
                        let chunk = ChunkKey::new(event.dimension, event.x, event.z);
                        let payload = std::mem::take(&mut event.payload);
                        let decoded = DecodedBiomeColumn::decode(
                            biome_base_sub_chunk_y,
                            biome_storage_count,
                            &payload,
                        )
                        .and_then(|biomes| {
                            let block_entities = DecodedBlockEntities::decode_level_chunk_tail(
                                chunk,
                                &payload[biomes.bytes_consumed()..],
                            )?;
                            Ok((biomes, block_entities))
                        });
                        DecodeCompletion {
                            sequence,
                            queue_wait,
                            event: PreparedWorldEvent::RequestLevelChunk {
                                event,
                                decoded,
                                duration: started.elapsed(),
                            },
                        }
                    }
                    DecodeJob::SubChunks { sequence, batch } => {
                        let dimension = batch.dimension;
                        let entries = prepare_sub_chunks(batch);
                        DecodeCompletion {
                            sequence,
                            queue_wait,
                            event: PreparedWorldEvent::SubChunks {
                                dimension,
                                entries,
                                duration: started.elapsed(),
                            },
                        }
                    }
                    DecodeJob::BlockUpdates {
                        sequence,
                        batches,
                        air_runtime_id,
                    } => {
                        let result = batches
                            .into_iter()
                            .map(|batch| {
                                ChunkStore::prepare_sub_chunk_blocks(
                                    batch.key,
                                    batch.previous.as_deref(),
                                    &batch.updates,
                                    air_runtime_id,
                                )
                            })
                            .collect();
                        DecodeCompletion {
                            sequence,
                            queue_wait,
                            event: PreparedWorldEvent::BlockUpdates {
                                result,
                                duration: started.elapsed(),
                            },
                        }
                    }
                    DecodeJob::BlockEntityUpdate { sequence, event } => {
                        let key = BlockEntityKey::new(
                            event.dimension,
                            event.position[0],
                            event.position[1],
                            event.position[2],
                        );
                        let decoded = DecodedBlockEntities::decode_live(key, &event.nbt);
                        DecodeCompletion {
                            sequence,
                            queue_wait,
                            event: PreparedWorldEvent::BlockEntityUpdate {
                                key,
                                decoded,
                                duration: started.elapsed(),
                            },
                        }
                    }
                };
                let _ = tx.send(completion);
            });
        }
    }
}
