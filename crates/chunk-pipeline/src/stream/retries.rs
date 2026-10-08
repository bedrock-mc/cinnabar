use super::*;

impl WorldStream {
    pub(super) fn complete_requested_sub_chunk(
        &mut self,
        key: SubChunkKey,
        mut collision_authoritative: bool,
    ) {
        self.requests.cancel_retry(key);
        let chunk = key.chunk();
        if collision_authoritative && self.authority.mark_sub_chunk_loaded(key).is_err() {
            collision_authoritative = false;
            self.record_normalization_error(NormalizationErrorReason::BlockMutationFailure);
        }
        if !collision_authoritative {
            self.requests.collision_failures.insert(chunk);
            // Vanilla leaves an unfilled slot empty, and an empty slot lights as air.
            if self.authority.terrain().sub_chunk(key).is_none() && self.record_known_air(key) {
                self.mark_changed(key, Instant::now());
            }
        }
        let (removed, completed) =
            self.requests
                .requested
                .get_mut(&chunk)
                .map_or((None, false), |expected| {
                    let removed = expected.remove(&key.y);
                    (removed, expected.is_empty())
                });
        if let Some(pending) = removed
            && (pending.pending_transport_attempts != 0 || pending.confirmed_attempts != 0)
        {
            self.requests.correlated_attempts.insert(
                key,
                CorrelatedSubChunkAttempts {
                    pending_transport_attempts: pending.pending_transport_attempts,
                    confirmed_attempts: pending.confirmed_attempts,
                },
            );
        }
        if completed {
            self.requests.queue.clear_mesh_blocker(chunk);
            self.requests.requested.remove(&chunk);
            if self.requests.collision_failures.contains(&chunk) {
                self.loaded_columns.remove(&chunk);
                return;
            }
            self.loaded_columns.insert(chunk);
            if self.authority.mark_chunk_loaded(chunk).is_err() {
                self.loaded_columns.remove(&chunk);
                self.record_normalization_error(NormalizationErrorReason::BlockMutationFailure);
            }
        }
    }
    pub(super) fn record_sub_chunk_reply_admissions(&mut self, batch: &SubChunkBatchEvent) {
        for entry in &batch.entries {
            self.record_sub_chunk_reply_admission_position(batch.dimension, entry.position);
        }
    }
    pub(super) fn record_sub_chunk_reply_admission(
        &mut self,
        admission: &SubChunkReplyAdmissionEvent,
    ) {
        for position in &admission.positions {
            self.record_sub_chunk_reply_admission_position(admission.dimension, *position);
        }
    }
    fn record_sub_chunk_reply_admission_position(&mut self, dimension: i32, position: [i32; 3]) {
        let key = SubChunkKey::new(dimension, position[0], position[1], position[2]);
        if !self.column_is_data_interesting(key.chunk()) {
            return;
        }
        let expected = self.requests.is_expected(key);
        let available = self
            .requests
            .requested
            .get(&key.chunk())
            .and_then(|column| column.get(&key.y))
            .map_or_else(
                || {
                    self.requests
                        .correlated_attempts
                        .get(&key)
                        .map_or(0, |attempts| attempts.confirmed_attempts)
                },
                |pending| pending.confirmed_attempts.max(1),
            );
        let admitted = self
            .requests
            .admitted_replies
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
                self.requests.cancel_retry(key);
            }
            self.requests
                .admitted_replies
                .insert(key, admitted.saturating_add(1));
        }
    }
    pub(super) fn retry_or_complete_sub_chunk(&mut self, key: SubChunkKey) -> bool {
        if self.requests.retry_is_queued(key) {
            return false;
        }
        let attempts = self
            .requests
            .requested
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
            true,
        )
    }
    pub(super) fn try_schedule_exact_retry(&mut self, key: SubChunkKey) -> RetrySchedule {
        if !self.requests.deferred_retries.is_empty()
            && self.requests.queue.len() < OUTBOUND_REQUEST_CAPACITY
        {
            self.pump_deferred_retries();
        }
        if !self.requests.deferred_retries.is_empty() {
            if self.requests.deferred_retries.len() >= DEFERRED_RETRY_CAPACITY {
                return RetrySchedule::CapacityFull;
            }
            self.requests.deferred_retries.push_back(key);
            self.requests.deferred_retry_set.insert(key);
            return RetrySchedule::Scheduled;
        }
        if self.requests.queue.len() < OUTBOUND_REQUEST_CAPACITY {
            return if self.enqueue_exact_retry(key) {
                RetrySchedule::Scheduled
            } else {
                RetrySchedule::EncodingFailure
            };
        }
        if self.requests.deferred_retries.len() < DEFERRED_RETRY_CAPACITY {
            self.requests.deferred_retries.push_back(key);
            self.requests.deferred_retry_set.insert(key);
            return RetrySchedule::Scheduled;
        }
        RetrySchedule::CapacityFull
    }
    pub(super) fn record_retry_scheduled(&mut self, key: SubChunkKey) {
        let pending = self
            .requests
            .requested
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
        let mut checked = 0;
        while checked == 0 || !self.poll_budget_exhausted() {
            checked += 1;
            let Some(&(deadline, key)) = self.requests.deadlines.first() else {
                break;
            };
            if deadline > now {
                break;
            }
            let Some(pending) = self
                .requests
                .requested
                .get(&key.chunk())
                .and_then(|column| column.get(&key.y))
                .copied()
            else {
                self.requests.deadlines.remove(&(deadline, key));
                continue;
            };
            if pending.response_deadline != Some(deadline) {
                self.requests.deadlines.remove(&(deadline, key));
                continue;
            }

            if pending.retry_attempts >= MAX_SUB_CHUNK_RETRIES {
                self.requests.disarm_deadline(key);
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
                    self.requests.disarm_deadline(key);
                    self.stats.sub_chunk_timeouts = self.stats.sub_chunk_timeouts.saturating_add(1);
                    self.record_retry_scheduled(key);
                }
                RetrySchedule::CapacityFull => break,
                RetrySchedule::EncodingFailure => {
                    self.requests.disarm_deadline(key);
                    self.stats.sub_chunk_timeouts = self.stats.sub_chunk_timeouts.saturating_add(1);
                    self.stats.phase2_outcomes.timed_out =
                        self.stats.phase2_outcomes.timed_out.saturating_add(1);
                    self.complete_requested_sub_chunk(key, false);
                }
            }
        }
        debug_assert!(self.requests.deadlines.len() <= self.outstanding_sub_chunk_count());
    }
    pub(super) fn pump_deferred_retries(&mut self) {
        self.pump_deferred_recovery_requests();
        let mut checked = 0;
        while self.requests.queue.len() < OUTBOUND_REQUEST_CAPACITY
            && (checked == 0 || !self.poll_budget_exhausted())
        {
            checked += 1;
            let Some(key) = self.requests.deferred_retries.pop_front() else {
                break;
            };
            self.requests.deferred_retry_set.remove(&key);
            if !self.requests.is_expected(key) {
                continue;
            }
            if !self.enqueue_exact_retry(key) {
                self.complete_requested_sub_chunk(key, false);
            }
        }
    }

    pub(super) fn pump_deferred_recovery_requests(&mut self) {
        let mut checked = 0;
        while self.requests.queue.len() < OUTBOUND_REQUEST_CAPACITY
            && (checked == 0 || !self.poll_budget_exhausted())
        {
            checked += 1;
            let Some(request) = self.requests.deferred_recovery.pop_front() else {
                break;
            };
            let has_expected = (0..request.count).any(|offset| {
                self.requests.is_expected(SubChunkKey::from_chunk(
                    request.chunk,
                    request
                        .base_sub_chunk_y
                        .saturating_add(i32::try_from(offset).unwrap_or(i32::MAX)),
                ))
            });
            if !has_expected {
                continue;
            }
            self.requests.queue.push_ready(request, true);
        }
    }
}
