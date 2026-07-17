use super::*;

impl WorldStream {
    pub(super) fn complete_requested_sub_chunk(
        &mut self,
        key: SubChunkKey,
        collision_authoritative: bool,
    ) {
        self.cancel_sub_chunk_retry(key);
        let chunk = key.chunk();
        if !collision_authoritative {
            self.request_collision_failures.insert(chunk);
        }
        let (removed, completed) =
            self.requested_sub_chunks
                .get_mut(&chunk)
                .map_or((None, false), |expected| {
                    let removed = expected.remove(&key.y);
                    (removed, expected.is_empty())
                });
        if let Some(pending) = removed
            && (pending.pending_transport_attempts != 0 || pending.confirmed_attempts != 0)
        {
            self.correlated_sub_chunk_attempts.insert(
                key,
                CorrelatedSubChunkAttempts {
                    pending_transport_attempts: pending.pending_transport_attempts,
                    confirmed_attempts: pending.confirmed_attempts,
                },
            );
        }
        if completed {
            self.requested_sub_chunks.remove(&chunk);
            self.loaded_columns.insert(chunk);
            if !self.request_collision_failures.remove(&chunk) {
                self.store.mark_chunk_loaded(chunk);
            }
        }
    }
    pub(super) fn consume_confirmed_sub_chunk_attempt(&mut self, key: SubChunkKey) {
        let Some(pending) = self
            .requested_sub_chunks
            .get_mut(&key.chunk())
            .and_then(|column| column.get_mut(&key.y))
        else {
            return;
        };
        pending.confirmed_attempts = pending.confirmed_attempts.saturating_sub(1);
    }
    pub(super) fn record_sub_chunk_reply_admissions(&mut self, batch: &SubChunkBatchEvent) {
        for entry in &batch.entries {
            let key = SubChunkKey::new(
                batch.dimension,
                entry.position[0],
                entry.position[1],
                entry.position[2],
            );
            if !self.column_is_active(key.chunk()) {
                continue;
            }
            let expected = self.is_expected_sub_chunk(key);
            let available = self
                .requested_sub_chunks
                .get(&key.chunk())
                .and_then(|column| column.get(&key.y))
                .map_or_else(
                    || {
                        self.correlated_sub_chunk_attempts
                            .get(&key)
                            .map_or(0, |attempts| attempts.confirmed_attempts)
                    },
                    |pending| pending.confirmed_attempts.max(1),
                );
            let admitted = self
                .admitted_sub_chunk_replies
                .get(&key)
                .copied()
                .unwrap_or(0);
            if admitted < available {
                self.stats.phase2_stages.responses_admitted = self
                    .stats
                    .phase2_stages
                    .responses_admitted
                    .saturating_add(1);
                if expected {
                    self.cancel_sub_chunk_retry(key);
                }
                self.admitted_sub_chunk_replies
                    .insert(key, admitted.saturating_add(1));
            }
        }
    }
    pub(super) fn consume_admitted_sub_chunk_reply(&mut self, key: SubChunkKey) -> bool {
        let Some(admitted) = self.admitted_sub_chunk_replies.get_mut(&key) else {
            return false;
        };
        *admitted = admitted.saturating_sub(1);
        if *admitted == 0 {
            self.admitted_sub_chunk_replies.remove(&key);
        }
        true
    }
    pub(super) fn consume_correlated_sub_chunk_attempt(&mut self, key: SubChunkKey) -> bool {
        let Some(attempts) = self.correlated_sub_chunk_attempts.get_mut(&key) else {
            return false;
        };
        if attempts.confirmed_attempts == 0 {
            return false;
        }
        attempts.confirmed_attempts = attempts.confirmed_attempts.saturating_sub(1);
        if attempts.confirmed_attempts == 0 && attempts.pending_transport_attempts == 0 {
            self.correlated_sub_chunk_attempts.remove(&key);
        }
        true
    }
    pub(super) fn retry_or_complete_sub_chunk(&mut self, key: SubChunkKey) -> bool {
        if self.retry_is_queued(key) {
            return false;
        }
        let attempts = self
            .requested_sub_chunks
            .get(&key.chunk())
            .and_then(|column| column.get(&key.y))
            .map_or(0, |pending| pending.retry_attempts);
        if attempts >= MAX_SUB_CHUNK_RETRIES {
            self.stats.sub_chunk_retry_exhaustions =
                self.stats.sub_chunk_retry_exhaustions.saturating_add(1);
            return true;
        }
        match self.try_schedule_exact_retry(key) {
            RetrySchedule::Scheduled => {
                self.record_retry_scheduled(key);
                false
            }
            RetrySchedule::CapacityFull => {
                self.record_normalization_error(
                    NormalizationErrorReason::DeferredRetryCapacityFailure,
                );
                true
            }
            RetrySchedule::EncodingFailure => true,
        }
    }
    pub(super) fn retry_is_queued(&self, key: SubChunkKey) -> bool {
        self.deferred_retry_set.contains(&key)
            || self.requests.iter().any(|slot| {
                matches!(slot, OutboundRequestSlot::Ready(request)
                    if request.chunk == key.chunk()
                        && request.base_sub_chunk_y == key.y
                        && request.count == 1)
            })
    }
    pub(super) fn enqueue_exact_retry(&mut self, key: SubChunkKey) -> bool {
        let Ok(packet) = request_sub_chunk_column(key.dimension, key.x, key.z, key.y, 1) else {
            self.record_normalization_error(NormalizationErrorReason::RetryRequestEncodingFailure);
            return false;
        };
        self.stats.phase2_stages.requests_constructed = self
            .stats
            .phase2_stages
            .requests_constructed
            .saturating_add(1);
        self.place_outbound_request(
            None,
            PendingSubChunkRequest {
                packet,
                dimension: key.dimension,
                chunk: key.chunk(),
                base_sub_chunk_y: key.y,
                count: 1,
            },
        )
    }
    pub(super) fn try_schedule_exact_retry(&mut self, key: SubChunkKey) -> RetrySchedule {
        if !self.deferred_retries.is_empty() && self.requests.len() < OUTBOUND_REQUEST_CAPACITY {
            self.pump_deferred_retries();
        }
        if !self.deferred_retries.is_empty() {
            if self.deferred_retries.len() >= DEFERRED_RETRY_CAPACITY {
                return RetrySchedule::CapacityFull;
            }
            self.deferred_retries.push_back(key);
            self.deferred_retry_set.insert(key);
            return RetrySchedule::Scheduled;
        }
        if self.requests.len() < OUTBOUND_REQUEST_CAPACITY {
            return if self.enqueue_exact_retry(key) {
                RetrySchedule::Scheduled
            } else {
                RetrySchedule::EncodingFailure
            };
        }
        if self.deferred_retries.len() < DEFERRED_RETRY_CAPACITY {
            self.deferred_retries.push_back(key);
            self.deferred_retry_set.insert(key);
            return RetrySchedule::Scheduled;
        }
        RetrySchedule::CapacityFull
    }
    pub(super) fn record_retry_scheduled(&mut self, key: SubChunkKey) {
        let pending = self
            .requested_sub_chunks
            .get_mut(&key.chunk())
            .and_then(|column| column.get_mut(&key.y))
            .expect("only an expected SubChunk Y may schedule a retry");
        pending.retry_attempts = pending.retry_attempts.saturating_add(1);
        self.stats.sub_chunk_retries_scheduled =
            self.stats.sub_chunk_retries_scheduled.saturating_add(1);
    }
    pub(super) fn expire_sub_chunk_deadlines(&mut self, now: Instant) {
        // Older deferred retries own newly free outbound slots. Expirations
        // observed in this pass must never bypass that FIFO.
        self.pump_deferred_retries();
        loop {
            let Some(&(deadline, key)) = self.sub_chunk_deadlines.first() else {
                break;
            };
            if deadline > now {
                break;
            }
            let Some(pending) = self
                .requested_sub_chunks
                .get(&key.chunk())
                .and_then(|column| column.get(&key.y))
                .copied()
            else {
                self.sub_chunk_deadlines.remove(&(deadline, key));
                continue;
            };
            if pending.response_deadline != Some(deadline) {
                self.sub_chunk_deadlines.remove(&(deadline, key));
                continue;
            }

            if pending.retry_attempts >= MAX_SUB_CHUNK_RETRIES {
                self.disarm_sub_chunk_deadline(key);
                self.stats.sub_chunk_timeouts = self.stats.sub_chunk_timeouts.saturating_add(1);
                self.stats.phase2_outcomes.timed_out =
                    self.stats.phase2_outcomes.timed_out.saturating_add(1);
                self.stats.sub_chunk_retry_exhaustions =
                    self.stats.sub_chunk_retry_exhaustions.saturating_add(1);
                self.complete_requested_sub_chunk(key, false);
                continue;
            }

            match self.try_schedule_exact_retry(key) {
                RetrySchedule::Scheduled => {
                    self.disarm_sub_chunk_deadline(key);
                    self.stats.sub_chunk_timeouts = self.stats.sub_chunk_timeouts.saturating_add(1);
                    self.record_retry_scheduled(key);
                }
                RetrySchedule::CapacityFull => break,
                RetrySchedule::EncodingFailure => {
                    self.disarm_sub_chunk_deadline(key);
                    self.stats.sub_chunk_timeouts = self.stats.sub_chunk_timeouts.saturating_add(1);
                    self.stats.phase2_outcomes.timed_out =
                        self.stats.phase2_outcomes.timed_out.saturating_add(1);
                    self.complete_requested_sub_chunk(key, false);
                }
            }
        }
        debug_assert!(self.sub_chunk_deadlines.len() <= self.outstanding_sub_chunk_count());
    }
    pub(super) fn pump_deferred_retries(&mut self) {
        while self.requests.len() < OUTBOUND_REQUEST_CAPACITY {
            let Some(key) = self.deferred_retries.pop_front() else {
                break;
            };
            self.deferred_retry_set.remove(&key);
            if !self.is_expected_sub_chunk(key) {
                continue;
            }
            if !self.enqueue_exact_retry(key) {
                self.complete_requested_sub_chunk(key, false);
            }
        }
    }
    pub(super) fn cancel_sub_chunk_retry(&mut self, key: SubChunkKey) {
        self.disarm_sub_chunk_deadline(key);
        if self.deferred_retry_set.remove(&key) {
            self.deferred_retries.retain(|pending| *pending != key);
        }
        self.requests.retain(|slot| {
            !matches!(slot, OutboundRequestSlot::Ready(request)
                if request.chunk == key.chunk()
                    && request.base_sub_chunk_y == key.y
                    && request.count == 1)
        });
    }
    pub(super) fn disarm_sub_chunk_deadline(&mut self, key: SubChunkKey) {
        let deadline = self
            .requested_sub_chunks
            .get_mut(&key.chunk())
            .and_then(|column| column.get_mut(&key.y))
            .and_then(|pending| pending.response_deadline.take());
        if let Some(deadline) = deadline {
            self.sub_chunk_deadlines.remove(&(deadline, key));
        }
    }
    pub(super) fn purge_sub_chunk_column_state(&mut self, chunk: ChunkKey) {
        if let Some(pending) = self.requested_sub_chunks.remove(&chunk) {
            for (y, pending) in pending {
                if let Some(deadline) = pending.response_deadline {
                    self.sub_chunk_deadlines
                        .remove(&(deadline, SubChunkKey::from_chunk(chunk, y)));
                }
            }
        }
        self.requests.retain(|slot| match slot {
            OutboundRequestSlot::Reserved(_) => true,
            OutboundRequestSlot::Ready(request) => request.chunk != chunk,
        });
        self.deferred_retries
            .retain(|sub_chunk| sub_chunk.chunk() != chunk);
        self.deferred_retry_set
            .retain(|sub_chunk| sub_chunk.chunk() != chunk);
        self.correlated_sub_chunk_attempts
            .retain(|sub_chunk, _| sub_chunk.chunk() != chunk);
        self.admitted_sub_chunk_replies
            .retain(|sub_chunk, _| sub_chunk.chunk() != chunk);
    }
    pub(super) fn queued_retry_request_count(&self) -> usize {
        let outbound = self
            .requests
            .iter()
            .filter(|slot| {
                let OutboundRequestSlot::Ready(request) = slot else {
                    return false;
                };
                request.count == 1
                    && self
                        .requested_sub_chunks
                        .get(&request.chunk)
                        .and_then(|column| column.get(&request.base_sub_chunk_y))
                        .is_some_and(|pending| pending.retry_attempts != 0)
            })
            .count();
        outbound.saturating_add(self.deferred_retries.len())
    }
}
