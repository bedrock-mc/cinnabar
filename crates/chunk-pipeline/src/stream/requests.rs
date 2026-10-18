use super::*;

mod dispatch;

/// Outbound sub-chunk requests and the per-slot bookkeeping that answers, retries and
/// eviction reconcile against.
#[derive(Default)]
pub(super) struct SubChunkRequests {
    pub(super) queue: RequestQueue,
    pub(super) requested: BTreeMap<ChunkKey, PendingSubChunkColumn>,
    pub(super) collision_failures: BTreeSet<ChunkKey>,
    /// Retired heights keep a column incomplete until received or replaced.
    pub(super) unencodable: BTreeMap<ChunkKey, BTreeSet<i32>>,
    pub(super) deadlines: BTreeSet<(Instant, SubChunkKey)>,
    pub(super) correlated_attempts: ColumnRecords<SubChunkKey, CorrelatedSubChunkAttempts>,
    pub(super) admitted_replies: ColumnRecords<SubChunkKey, u8>,
    pub(super) deferred_retries: ColumnQueue<SubChunkKey>,
    pub(super) deferred_retry_set: HashSet<SubChunkKey>,
    pub(super) deferred_recovery: ColumnQueue<PendingSubChunkRequest>,
    pub(super) recoveries: BTreeMap<ChunkKey, BTreeMap<i32, SubChunkRecovery>>,
    /// The last recovery column visited, so spent polls resume past it.
    pub(super) recovery_cursor: Option<ChunkKey>,
    pub(super) transport: BTreeMap<u64, SubChunkRequestReceipt>,
    next_identity: u64,
    pub(super) last_player_chunk: Option<ChunkKey>,
}

impl SubChunkRequests {
    /// Clears the column failure only when no missing heights remain.
    pub(super) fn clear_recovered_collision_failure(&mut self, chunk: ChunkKey) {
        if !self.recoveries.contains_key(&chunk) && !self.unencodable.contains_key(&chunk) {
            self.collision_failures.remove(&chunk);
        }
    }

    /// Allocates identities that are never reused after a column or dimension retires.
    pub(super) fn allocate_identity(&mut self) -> u64 {
        self.next_identity = self
            .next_identity
            .checked_add(1)
            .expect("request identity space exhausted");
        self.next_identity
    }
    pub(super) fn consume_confirmed_attempt(&mut self, key: SubChunkKey) {
        let Some(pending) = self
            .requested
            .get_mut(&key.chunk())
            .and_then(|column| column.get_mut(&key.y))
        else {
            return;
        };
        if pending.confirmed_attempts != 0 {
            pending.confirmed_attempts -= 1;
        } else if pending.pending_transport_attempts != 0 {
            // Responses can commit before the successful-write event reaches the app.
            pending.answered_transport_packet = pending.latest_transport_packet;
        }
    }
    pub(super) fn clear_admitted_replies(&mut self, key: SubChunkKey) -> bool {
        self.admitted_replies.remove(&key).is_some()
    }
    pub(super) fn consume_admitted_reply(&mut self, key: SubChunkKey) -> bool {
        let Some(admitted) = self.admitted_replies.get_mut(&key) else {
            return false;
        };
        *admitted = admitted.saturating_sub(1);
        if *admitted == 0 {
            self.admitted_replies.remove(&key);
        }
        true
    }
    pub(super) fn consume_correlated_attempt(&mut self, key: SubChunkKey) -> bool {
        let Some(attempts) = self.correlated_attempts.get_mut(&key) else {
            return false;
        };
        if attempts.confirmed_attempts == 0 {
            return false;
        }
        attempts.confirmed_attempts = attempts.confirmed_attempts.saturating_sub(1);
        if attempts.confirmed_attempts == 0 && attempts.pending_transport_attempts == 0 {
            self.correlated_attempts.remove(&key);
        }
        true
    }
    pub(super) fn retry_is_queued(&self, key: SubChunkKey) -> bool {
        self.deferred_retry_set.contains(&key)
            || self.queue.ready_requests().any(|request| {
                request.chunk == key.chunk()
                    && request.base_sub_chunk_y == key.y
                    && request.count == 1
            })
    }
    pub(super) fn cancel_retry(&mut self, key: SubChunkKey) {
        self.disarm_deadline(key);
        if self.deferred_retry_set.remove(&key) {
            self.deferred_retries.retain(|pending| *pending != key);
        }
        self.queue.cancel_ready(|request| {
            request.chunk == key.chunk() && request.base_sub_chunk_y == key.y && request.count == 1
        });
    }
    pub(super) fn disarm_deadline(&mut self, key: SubChunkKey) {
        let deadline = self
            .requested
            .get_mut(&key.chunk())
            .and_then(|column| column.get_mut(&key.y))
            .and_then(|pending| pending.response_deadline.take());
        if let Some(deadline) = deadline {
            self.deadlines.remove(&(deadline, key));
        }
    }
    /// Removes request bookkeeping by column, preserving unrelated queue order.
    pub(super) fn purge_columns(&mut self, chunks: &BTreeSet<ChunkKey>) {
        for &chunk in chunks {
            if let Some(pending) = self.requested.remove(&chunk) {
                for (y, pending) in pending {
                    if let Some(deadline) = pending.response_deadline {
                        self.deadlines
                            .remove(&(deadline, SubChunkKey::from_chunk(chunk, y)));
                    }
                }
            }
        }
        self.queue.cancel_columns(chunks);
        for &chunk in chunks {
            for key in self.deferred_retries.remove_column(chunk) {
                self.deferred_retry_set.remove(&key);
            }
            self.deferred_recovery.remove_column(chunk);
            self.correlated_attempts.remove_column(chunk);
            self.admitted_replies.remove_column(chunk);
            self.recoveries.remove(&chunk);
            self.unencodable.remove(&chunk);
        }
    }
    /// Reads and resets record visits made by the shared column cleanup helpers.
    #[cfg(test)]
    fn take_cleanup_visits(&mut self) -> usize {
        self.queue.take_cleanup_visits()
            + self.deferred_retries.take_cleanup_visits()
            + self.deferred_recovery.take_cleanup_visits()
            + self.correlated_attempts.take_cleanup_visits()
            + self.admitted_replies.take_cleanup_visits()
    }
    pub(super) fn queued_retry_count(&self) -> usize {
        let outbound = self
            .queue
            .ready_requests()
            .filter(|request| {
                request.count == 1
                    && self
                        .requested
                        .get(&request.chunk)
                        .and_then(|column| column.get(&request.base_sub_chunk_y))
                        .is_some_and(|pending| pending.retry_attempts != 0)
            })
            .count();
        outbound
            .saturating_add(self.deferred_retries.len())
            .saturating_add(self.deferred_recovery.len())
    }
    pub(super) fn is_expected(&self, key: SubChunkKey) -> bool {
        self.requested
            .get(&key.chunk())
            .is_some_and(|expected| expected.contains_key(&key.y))
    }
}

impl WorldStream {
    pub fn take_requests(&mut self) -> Vec<PendingSubChunkRequest> {
        let mut ready = Vec::new();
        loop {
            self.pump_deferred_recovery_requests();
            let mut popped = false;
            while let Some(request) = self.requests.queue.pop_next(
                self.requests.last_player_chunk,
                &self.publisher.required_columns,
            ) {
                let _ = self.requests.queue.confirm_popped(&request);
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
        self.requests.queue.pop_next(
            self.requests.last_player_chunk,
            &self.publisher.required_columns,
        )
    }

    pub fn retry_request_front(
        &mut self,
        request: PendingSubChunkRequest,
    ) -> Result<(), Box<PendingSubChunkRequest>> {
        if self.requests.queue.len() >= OUTBOUND_REQUEST_CAPACITY {
            return Err(Box::new(request));
        }
        self.requests.queue.retry_front(request);
        Ok(())
    }
    pub fn pending_request_count(&self) -> usize {
        self.requests.queue.ready_requests().count()
    }
    pub fn pending_request_work_count(&self) -> usize {
        self.requests.queue.len()
    }
    pub fn outstanding_sub_chunk_count(&self) -> usize {
        self.requests
            .requested
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
        self.requests.clear_recovered_collision_failure(key);
        if count == 0 {
            if let Some(sequence) = sequence {
                self.cancel_request_reservation(sequence);
            }
            if self.requests.collision_failures.contains(&key) {
                return;
            }
            self.loaded_columns.insert(key);
            if self.authority.mark_chunk_loaded(key).is_err() {
                self.loaded_columns.remove(&key);
                self.record_normalization_error(NormalizationErrorReason::BlockMutationFailure);
            } else {
                self.note_far_column_complete(key);
            }
            return;
        }
        let demand = self.requests.allocate_identity();
        match PendingSubChunkRequest::new(key, base_sub_chunk_y, count, demand) {
            Some(request) => {
                self.stats.publication_stages.requests_constructed = self
                    .stats
                    .publication_stages
                    .requests_constructed
                    .saturating_add(1);
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
                            PendingSubChunk {
                                demand,
                                ..Default::default()
                            },
                        )
                    })
                    .collect::<PendingSubChunkColumn>();
                if expected.is_empty() {
                    self.loaded_columns.insert(key);
                    if self.authority.mark_chunk_loaded(key).is_err() {
                        self.loaded_columns.remove(&key);
                        self.record_normalization_error(
                            NormalizationErrorReason::BlockMutationFailure,
                        );
                    } else {
                        self.note_far_column_complete(key);
                    }
                } else {
                    self.requests.requested.insert(key, expected);
                }
            }
            None => {
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
        self.requests.clear_recovered_collision_failure(key);
        let range_end = range
            .base_sub_chunk_y
            .saturating_add(i32::try_from(range.sub_chunk_count).unwrap_or(i32::MAX));
        let mut missing = BTreeSet::new();
        for y in ys
            .iter()
            .copied()
            .filter(|y| *y >= range.base_sub_chunk_y && *y < range_end)
        {
            let sub_chunk = SubChunkKey::from_chunk(key, y);
            let admitted = self.requests.clear_admitted_replies(sub_chunk);
            if self.requests.is_expected(sub_chunk) {
                if admitted && self.retry_or_complete_sub_chunk(sub_chunk) {
                    self.complete_requested_sub_chunk(sub_chunk, false);
                }
            } else {
                missing.insert(y);
            }
        }

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
            let demand = self.requests.allocate_identity();
            let Some(request) = PendingSubChunkRequest::new(key, base_y, count, demand) else {
                self.record_normalization_error(NormalizationErrorReason::RequestEncodingFailure);
                continue;
            };
            self.stats.publication_stages.requests_constructed = self
                .stats
                .publication_stages
                .requests_constructed
                .saturating_add(1);
            requests.push((base_y, count, request));
        }

        if !requests.is_empty() {
            let expected = self.requests.requested.entry(key).or_default();
            for (base_y, count, request) in &requests {
                for offset in 0..*count {
                    expected
                        .entry(base_y.saturating_add(i32::try_from(offset).unwrap_or(i32::MAX)))
                        .or_insert(PendingSubChunk {
                            demand: request.demand,
                            ..Default::default()
                        });
                }
            }
        }

        let mut reservation = sequence;
        for (_, _, request) in requests {
            match reservation {
                Some(sequence) if self.requests.queue.has_reservation(sequence) => {
                    let placed = self.requests.queue.replace_reservation(sequence, request);
                    debug_assert!(placed);
                    reservation = None;
                }
                _ if self.requests.queue.len() >= OUTBOUND_REQUEST_CAPACITY => {
                    self.requests.deferred_recovery.push_back(request);
                }
                _ => {
                    self.requests.queue.push_ready(request, false);
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
            return self.requests.queue.replace_reservation(sequence, request);
        }
        if self.requests.queue.len() >= OUTBOUND_REQUEST_CAPACITY {
            return false;
        }
        self.requests.queue.push_ready(request, retry);
        true
    }
    pub(super) fn cancel_request_reservation(&mut self, sequence: u64) {
        self.requests.queue.cancel_reservation(sequence);
    }
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;

    /// Builds one valid request identity for the populated retirement fixture.
    fn request(chunk: ChunkKey) -> PendingSubChunkRequest {
        PendingSubChunkRequest {
            demand: 0,
            chunk,
            base_sub_chunk_y: 0,
            count: 1,
        }
    }

    /// A disjoint populated retirement visits only records owned by each retired column.
    #[test]
    fn column_retirement_visits_only_its_request_records() {
        let columns = 4096;
        let mut requests = SubChunkRequests::default();
        let deadline = Instant::now();
        for x in 0..columns {
            let column = ChunkKey::new(0, x, 0);
            let key = SubChunkKey::from_chunk(column, 0);
            requests.requested.insert(
                column,
                BTreeMap::from([(
                    0,
                    PendingSubChunk {
                        response_deadline: Some(deadline),
                        ..Default::default()
                    },
                )]),
            );
            requests.deadlines.insert((deadline, key));
            requests.correlated_attempts.insert(
                key,
                CorrelatedSubChunkAttempts {
                    confirmed_attempts: 1,
                    ..Default::default()
                },
            );
            requests.admitted_replies.insert(key, 1);
            requests
                .recoveries
                .insert(column, BTreeMap::from([(0, SubChunkRecovery::default())]));
            requests.deferred_recovery.push_back(request(column));
            if x < OUTBOUND_REQUEST_CAPACITY as i32 {
                requests.queue.push_ready(request(column), false);
                requests.queue.pop_next(None, &BTreeSet::new()).unwrap();
            }
        }
        for x in 0..OUTBOUND_REQUEST_CAPACITY as i32 {
            let column = ChunkKey::new(0, x, 0);
            let key = SubChunkKey::from_chunk(column, 0);
            requests.deferred_retries.push_back(key);
            requests.deferred_retry_set.insert(key);
            requests.queue.push_ready(request(column), true);
        }
        let survivor = ChunkKey::new(1, 0, 0);
        requests.queue.reserve(7);
        requests.queue.push_ready(request(survivor), false);
        let (mut total, mut max) = (0, 0);
        for x in 0..columns {
            requests.purge_columns(&BTreeSet::from([ChunkKey::new(0, x, 0)]));
            let visits = requests.take_cleanup_visits();
            total += visits;
            max = max.max(visits);
        }
        assert!(requests.requested.is_empty());
        assert!(requests.deadlines.is_empty());
        assert!(requests.recoveries.is_empty());
        assert!(requests.deferred_retries.is_empty());
        assert!(requests.deferred_retry_set.is_empty());
        assert!(requests.deferred_recovery.is_empty());
        assert_eq!(requests.correlated_attempts.len(), 0);
        assert_eq!(requests.admitted_replies.len(), 0);
        assert!(
            requests.queue.pop_next(None, &BTreeSet::new()).is_none(),
            "the reservation survives retirement"
        );
        assert!(requests.queue.replace_reservation(7, request(survivor)));
        for _ in 0..2 {
            assert_eq!(
                requests
                    .queue
                    .pop_next(None, &BTreeSet::new())
                    .unwrap()
                    .chunk,
                survivor
            );
        }
        eprintln!("request retirement: columns={columns} total_visits={total} max_visits={max}");
        assert_eq!(max, 6, "retirement visits only its own request records");
        assert_eq!(total, columns as usize * 3 + OUTBOUND_REQUEST_CAPACITY * 3);
    }
}
