use super::*;

impl WorldStream {
    pub fn take_requests(&mut self) -> Vec<PendingSubChunkRequest> {
        let mut ready = Vec::new();
        loop {
            self.pump_deferred_recovery_requests();
            let mut popped = false;
            while let Some(request) = self
                .requests
                .pop_next(self.last_request_player_chunk, &self.required_columns)
            {
                self.requests.confirm_popped(&request);
                ready.push(request);
                popped = true;
            }
            if !popped {
                break;
            }
        }
        ready
    }
    pub fn pop_next_request(&mut self) -> Option<PendingSubChunkRequest> {
        self.pump_deferred_recovery_requests();
        self.requests
            .pop_next(self.last_request_player_chunk, &self.required_columns)
    }

    fn record_local_reset_dispatch(&mut self) {
        if !self.local_reset_dispatch_active {
            return;
        }
        let Some(class) = self.requests.last_popped_class() else {
            return;
        };
        self.local_reset_dispatch_total = self.local_reset_dispatch_total.saturating_add(1);
        if matches!(
            class,
            RequestClass::PlayerInitial | RequestClass::PlayerRetry
        ) {
            self.local_reset_dispatch_active = false;
        }
        if usize::from(self.local_reset_dispatch_count) >= MAX_LOCAL_RESET_DISPATCH_EVIDENCE {
            return;
        }
        let index = usize::from(self.local_reset_dispatch_count);
        self.local_reset_dispatch_classes[index] = Some(class);
        self.local_reset_dispatch_count = self.local_reset_dispatch_count.saturating_add(1);
    }
    pub fn retry_request_front(
        &mut self,
        request: PendingSubChunkRequest,
    ) -> Result<(), Box<PendingSubChunkRequest>> {
        if self.requests.len() >= OUTBOUND_REQUEST_CAPACITY {
            return Err(Box::new(request));
        }
        self.requests.retry_front(request);
        Ok(())
    }
    pub fn record_sub_chunk_request_transport_pending(
        &mut self,
        chunk: ChunkKey,
        base_sub_chunk_y: i32,
        count: usize,
    ) {
        self.record_local_reset_dispatch();
        self.transport_pending_requests = self.transport_pending_requests.saturating_add(1);
        self.requests
            .confirm_popped_identity(chunk, base_sub_chunk_y, count);
        for offset in 0..count {
            let y = base_sub_chunk_y.saturating_add(offset as i32);
            if let Some(pending) = self
                .requested_sub_chunks
                .get_mut(&chunk)
                .and_then(|column| column.get_mut(&y))
            {
                pending.pending_transport_attempts = pending
                    .pending_transport_attempts
                    .saturating_add(1)
                    .min(MAX_SUB_CHUNK_RETRIES.saturating_add(1));
            }
        }
    }
    pub fn acknowledge_sub_chunk_request_sent(
        &mut self,
        chunk: ChunkKey,
        base_sub_chunk_y: i32,
        count: usize,
        sent_at: Instant,
    ) {
        self.transport_pending_requests = self.transport_pending_requests.saturating_sub(1);
        self.stats.phase2_stages.requests_sent =
            self.stats.phase2_stages.requests_sent.saturating_add(1);
        let deadline = sent_at
            .checked_add(SUB_CHUNK_RESPONSE_TIMEOUT)
            .unwrap_or(sent_at);
        for offset in 0..count {
            let y = base_sub_chunk_y.saturating_add(offset as i32);
            let key = SubChunkKey::from_chunk(chunk, y);
            let reply_admitted = self
                .admitted_sub_chunk_replies
                .get(&key)
                .is_some_and(|admitted| *admitted != 0);
            let pending = self
                .requested_sub_chunks
                .get_mut(&chunk)
                .and_then(|column| column.get_mut(&y));
            let Some(pending) = pending else {
                if let Some(correlated) = self.correlated_sub_chunk_attempts.get_mut(&key)
                    && correlated.pending_transport_attempts != 0
                {
                    correlated.pending_transport_attempts =
                        correlated.pending_transport_attempts.saturating_sub(1);
                    correlated.confirmed_attempts = correlated
                        .confirmed_attempts
                        .saturating_add(1)
                        .min(MAX_SUB_CHUNK_RETRIES.saturating_add(1));
                }
                continue;
            };
            pending.pending_transport_attempts =
                pending.pending_transport_attempts.saturating_sub(1);
            pending.confirmed_attempts = pending
                .confirmed_attempts
                .saturating_add(1)
                .min(MAX_SUB_CHUNK_RETRIES.saturating_add(1));
            if reply_admitted {
                if let Some(previous) = pending.response_deadline.take() {
                    self.sub_chunk_deadlines.remove(&(previous, key));
                }
                continue;
            }
            let previous = pending.response_deadline.replace(deadline);
            if let Some(previous) = previous {
                self.sub_chunk_deadlines.remove(&(previous, key));
            }
            self.sub_chunk_deadlines.insert((deadline, key));
        }
        debug_assert!(self.sub_chunk_deadlines.len() <= self.outstanding_sub_chunk_count());
    }
    pub fn pending_request_count(&self) -> usize {
        self.requests
            .iter()
            .filter(|slot| matches!(slot, OutboundRequestSlot::Ready(_)))
            .count()
    }
    pub fn pending_request_work_count(&self) -> usize {
        self.requests.len()
    }
    pub fn outstanding_sub_chunk_count(&self) -> usize {
        self.requested_sub_chunks
            .values()
            .fold(0, |total, pending| total.saturating_add(pending.len()))
    }
    pub(super) fn enqueue_request(
        &mut self,
        key: ChunkKey,
        base_sub_chunk_y: i32,
        count: usize,
        sequence: Option<u64>,
    ) {
        self.request_collision_failures.remove(&key);
        if count == 0 {
            if let Some(sequence) = sequence {
                self.cancel_request_reservation(sequence);
            }
            self.loaded_columns.insert(key);
            if self.store.mark_chunk_loaded(key).is_err() {
                self.loaded_columns.remove(&key);
                self.record_normalization_error(NormalizationErrorReason::BlockMutationFailure);
            }
            return;
        }
        match request_sub_chunk_column(key.dimension, key.x, key.z, base_sub_chunk_y, count) {
            Ok(packet) => {
                self.stats.phase2_stages.requests_constructed = self
                    .stats
                    .phase2_stages
                    .requests_constructed
                    .saturating_add(1);
                let request = PendingSubChunkRequest {
                    packet,
                    dimension: key.dimension,
                    chunk: key,
                    base_sub_chunk_y,
                    count,
                };
                if !self.place_outbound_request(sequence, request, false) {
                    self.record_normalization_error(
                        NormalizationErrorReason::OutboundRequestPlacementFailure,
                    );
                    return;
                }
                let expected = (0..count)
                    .map(|offset| {
                        (
                            base_sub_chunk_y.saturating_add(offset as i32),
                            PendingSubChunk::default(),
                        )
                    })
                    .collect::<PendingSubChunkColumn>();
                if expected.is_empty() {
                    self.loaded_columns.insert(key);
                    if self.store.mark_chunk_loaded(key).is_err() {
                        self.loaded_columns.remove(&key);
                        self.record_normalization_error(
                            NormalizationErrorReason::BlockMutationFailure,
                        );
                    }
                } else {
                    self.requested_sub_chunks.insert(key, expected);
                }
            }
            Err(_) => {
                self.record_normalization_error(NormalizationErrorReason::RequestEncodingFailure)
            }
        }
    }

    pub(super) fn enqueue_exact_recovery_requests(
        &mut self,
        key: ChunkKey,
        range: DimensionRange,
        ys: &[i32],
        sequence: Option<u64>,
    ) {
        self.request_collision_failures.remove(&key);
        let range_end = range
            .base_sub_chunk_y
            .saturating_add(i32::try_from(range.sub_chunk_count).unwrap_or(i32::MAX));
        let missing = ys
            .iter()
            .copied()
            .filter(|y| {
                *y >= range.base_sub_chunk_y
                    && *y < range_end
                    && !self.is_expected_sub_chunk(SubChunkKey::from_chunk(key, *y))
            })
            .collect::<BTreeSet<_>>();

        let mut ranges: Vec<(i32, i32)> = Vec::new();
        let mut start: Option<i32> = None;
        let mut previous: Option<i32> = None;
        for y in missing {
            match (start, previous) {
                (Some(start_y), Some(previous_y)) if y == previous_y.saturating_add(1) => {
                    start = Some(start_y);
                    previous = Some(y);
                }
                (Some(start_y), Some(previous_y)) => {
                    ranges.push((start_y, previous_y));
                    start = Some(y);
                    previous = Some(y);
                }
                _ => {
                    start = Some(y);
                    previous = Some(y);
                }
            }
        }
        if let (Some(start_y), Some(previous_y)) = (start, previous) {
            ranges.push((start_y, previous_y));
        }

        let mut requests = Vec::with_capacity(ranges.len());
        for (base_y, end_y) in ranges {
            let count = end_y
                .checked_sub(base_y)
                .and_then(|distance| usize::try_from(distance).ok())
                .and_then(|distance| distance.checked_add(1))
                .unwrap_or(0);
            if count == 0 {
                continue;
            }
            let Ok(packet) = request_sub_chunk_column(key.dimension, key.x, key.z, base_y, count)
            else {
                self.record_normalization_error(NormalizationErrorReason::RequestEncodingFailure);
                continue;
            };
            self.stats.phase2_stages.requests_constructed = self
                .stats
                .phase2_stages
                .requests_constructed
                .saturating_add(1);
            requests.push((
                base_y,
                count,
                PendingSubChunkRequest {
                    packet,
                    dimension: key.dimension,
                    chunk: key,
                    base_sub_chunk_y: base_y,
                    count,
                },
            ));
        }

        if !requests.is_empty() {
            let expected = self.requested_sub_chunks.entry(key).or_default();
            for (base_y, count, _) in &requests {
                for offset in 0..*count {
                    expected
                        .entry(base_y.saturating_add(i32::try_from(offset).unwrap_or(i32::MAX)))
                        .or_default();
                }
            }
        }

        let mut reservation = sequence;
        for (_, _, request) in requests {
            match reservation {
                Some(sequence) if self.requests.has_reservation(sequence) => {
                    let placed = self.requests.replace_reservation(sequence, request);
                    debug_assert!(placed);
                    reservation = None;
                }
                _ if self.requests.len() >= OUTBOUND_REQUEST_CAPACITY => {
                    self.deferred_recovery_requests.push_back(request);
                }
                _ => {
                    self.requests.push_ready(request, false);
                    reservation = None;
                }
            }
        }
        if let Some(sequence) = reservation {
            self.cancel_request_reservation(sequence);
        }
    }

    pub(super) fn place_outbound_request(
        &mut self,
        sequence: Option<u64>,
        request: PendingSubChunkRequest,
        retry: bool,
    ) -> bool {
        if let Some(sequence) = sequence {
            return self.requests.replace_reservation(sequence, request);
        }
        if self.requests.len() >= OUTBOUND_REQUEST_CAPACITY {
            return false;
        }
        self.requests.push_ready(request, retry);
        true
    }
    pub(super) fn cancel_request_reservation(&mut self, sequence: u64) {
        self.requests.cancel_reservation(sequence);
    }
}
