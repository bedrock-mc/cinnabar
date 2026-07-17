use super::*;

impl WorldStream {
    pub fn take_requests(&mut self) -> Vec<PendingSubChunkRequest> {
        let mut ready = Vec::new();
        let mut reserved = VecDeque::new();
        while let Some(slot) = self.requests.pop_front() {
            match slot {
                OutboundRequestSlot::Reserved(sequence) => {
                    reserved.push_back(OutboundRequestSlot::Reserved(sequence));
                }
                OutboundRequestSlot::Ready(request) => ready.push(request),
            }
        }
        self.requests = reserved;
        ready
    }
    pub fn pop_next_request(&mut self) -> Option<PendingSubChunkRequest> {
        if !matches!(self.requests.front(), Some(OutboundRequestSlot::Ready(_))) {
            return None;
        }
        match self.requests.pop_front() {
            Some(OutboundRequestSlot::Ready(request)) => Some(request),
            Some(OutboundRequestSlot::Reserved(_)) | None => None,
        }
    }
    pub fn retry_request_front(
        &mut self,
        request: PendingSubChunkRequest,
    ) -> Result<(), Box<PendingSubChunkRequest>> {
        if self.requests.len() >= OUTBOUND_REQUEST_CAPACITY {
            return Err(Box::new(request));
        }
        self.requests
            .push_front(OutboundRequestSlot::Ready(request));
        Ok(())
    }
    pub fn record_sub_chunk_request_transport_pending(
        &mut self,
        chunk: ChunkKey,
        base_sub_chunk_y: i32,
        count: usize,
    ) {
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
                if !self.place_outbound_request(sequence, request) {
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
    pub(super) fn place_outbound_request(
        &mut self,
        sequence: Option<u64>,
        request: PendingSubChunkRequest,
    ) -> bool {
        if let Some(sequence) = sequence
            && let Some(slot) = self.requests.iter_mut().find(|slot| {
                matches!(slot, OutboundRequestSlot::Reserved(reserved) if *reserved == sequence)
            })
        {
            *slot = OutboundRequestSlot::Ready(request);
            return true;
        }
        if self.requests.len() >= OUTBOUND_REQUEST_CAPACITY {
            return false;
        }
        self.requests.push_back(OutboundRequestSlot::Ready(request));
        true
    }
    pub(super) fn cancel_request_reservation(&mut self, sequence: u64) {
        self.requests.retain(|slot| {
            !matches!(slot, OutboundRequestSlot::Reserved(reserved) if *reserved == sequence)
        });
    }
}
