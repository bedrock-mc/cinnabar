use super::*;

// The intent budget also bounds flattening work before transport encoding.
const _: () =
    assert!(OUTBOUND_REQUEST_CAPACITY * MAX_SUB_CHUNK_COLUMN_HEIGHT <= MAX_SUB_CHUNK_REQUESTS);

impl WorldStream {
    /// Prepares the oldest admitted column intents without crossing a reservation.
    pub fn prepare_sub_chunk_request(
        &mut self,
        center: [i32; 3],
        intent_budget: usize,
    ) -> Option<SubChunkRequestBatch> {
        if self.requests.transport.len() >= OUTBOUND_REQUEST_CAPACITY {
            return None;
        }
        self.pump_deferred_recovery_requests();
        let mut intents = Vec::new();
        for _ in 0..intent_budget.min(OUTBOUND_REQUEST_CAPACITY) {
            let Some(request) = self.requests.queue.pop_next(
                self.requests.last_player_chunk,
                &self.publisher.required_columns,
            ) else {
                break;
            };
            intents.push(request);
        }
        self.collect_request_intents(center, intents)
    }

    /// Collects current slots once; rollback keeps the compact column intents.
    fn collect_request_intents(
        &mut self,
        center: [i32; 3],
        intents: Vec<PendingSubChunkRequest>,
    ) -> Option<SubChunkRequestBatch> {
        let dimension = self.current_dimension();
        let mut members = Vec::new();
        let mut included = HashSet::new();
        let mut retained = Vec::new();
        for intent in intents {
            let before = members.len();
            for key in intent.keys() {
                if !self.requests.matches_demand(key, intent.demand) {
                    continue;
                }
                let position = [key.x, key.y, key.z];
                if key.dimension != dimension
                    || sub_chunk_request_offset(center, position).is_none()
                {
                    self.retire_unsent_request(key, intent.demand);
                } else if !self.requests.admitted_replies.contains_key(&key) && included.insert(key)
                {
                    members.push(SubChunkRequestMember {
                        position,
                        demand: intent.demand,
                    });
                }
            }
            if members.len() == before {
                let _ = self.requests.queue.confirm_popped(&intent);
            } else {
                retained.push(intent);
            }
        }
        if members.is_empty() {
            return None;
        }
        let receipt = SubChunkRequestReceipt {
            owner: self.terrain_generation(),
            id: self.requests.allocate_identity(),
            dimension,
            members: members.into(),
        };
        Some(SubChunkRequestBatch {
            receipt,
            intents: retained,
        })
    }

    /// Restores an unsent batch in its original order, omitting retired demands.
    pub fn retry_sub_chunk_request_front(
        &mut self,
        batch: SubChunkRequestBatch,
    ) -> Result<(), SubChunkRequestBatch> {
        if batch.receipt.owner != self.terrain_generation() {
            return Ok(());
        }
        let live = |intent: &PendingSubChunkRequest| {
            intent
                .keys()
                .any(|key| self.requests.matches_demand(key, intent.demand))
        };
        if self.requests.queue.len() + batch.intents.iter().filter(|intent| live(intent)).count()
            > OUTBOUND_REQUEST_CAPACITY
        {
            return Err(batch);
        }
        for intent in batch.intents.into_iter().rev() {
            if intent
                .keys()
                .any(|key| self.requests.matches_demand(key, intent.demand))
            {
                self.requests.queue.retry_front(intent);
            } else {
                let _ = self.requests.queue.confirm_popped(&intent);
            }
        }
        Ok(())
    }

    /// Records one admitted packet and the exact member attempts awaiting a write receipt.
    pub fn record_sub_chunk_request_transport_pending(&mut self, batch: &SubChunkRequestBatch) {
        let receipt = &batch.receipt;
        if receipt.owner != self.terrain_generation()
            || self.requests.transport.contains_key(&receipt.id)
        {
            return;
        }
        for intent in &batch.intents {
            if let Some(retry) = self.requests.queue.confirm_popped(intent) {
                self.publisher.local_reset.record_dispatch(Some(
                    super::super::request_queue::request_class(
                        retry,
                        intent.chunk,
                        self.requests.last_player_chunk,
                        &self.publisher.required_columns,
                    ),
                ));
            }
        }
        for member in receipt.members.iter() {
            let key = SubChunkKey::new(
                receipt.dimension,
                member.position[0],
                member.position[1],
                member.position[2],
            );
            if let Some(pending) = self
                .requests
                .requested
                .get_mut(&key.chunk())
                .and_then(|column| column.get_mut(&key.y))
                .filter(|pending| pending.demand == member.demand)
            {
                pending.latest_transport_packet = pending.latest_transport_packet.max(receipt.id);
                pending.pending_transport_attempts = pending
                    .pending_transport_attempts
                    .saturating_add(1)
                    .min(MAX_SUB_CHUNK_RETRIES.saturating_add(1));
            }
        }
        self.requests.transport.insert(receipt.id, receipt.clone());
    }

    /// Confirms each packet once and arms only the demands that owned its encoded members.
    pub fn acknowledge_sub_chunk_request_sent(
        &mut self,
        receipt: &SubChunkRequestReceipt,
        sent_at: Instant,
    ) {
        if self.requests.transport.get(&receipt.id) != Some(receipt) {
            return;
        }
        self.requests.transport.remove(&receipt.id);
        if receipt.owner != self.terrain_generation() {
            return;
        }
        self.stats.publication_stages.requests_sent = self
            .stats
            .publication_stages
            .requests_sent
            .saturating_add(1);
        let deadline = sent_at
            .checked_add(SUB_CHUNK_RESPONSE_TIMEOUT)
            .unwrap_or(sent_at);
        for member in receipt.members.iter() {
            let key = SubChunkKey::new(
                receipt.dimension,
                member.position[0],
                member.position[1],
                member.position[2],
            );
            let pending = self
                .requests
                .requested
                .get_mut(&key.chunk())
                .and_then(|column| column.get_mut(&key.y))
                .filter(|pending| pending.demand == member.demand);
            let Some(pending) = pending else {
                if let Some(correlated) =
                    self.requests
                        .correlated_attempts
                        .get_mut(&key)
                        .filter(|attempt| {
                            attempt.demand == member.demand
                                && attempt.pending_transport_attempts != 0
                        })
                {
                    correlated.pending_transport_attempts -= 1;
                    if receipt.id > correlated.answered_transport_packet {
                        correlated.confirmed_attempts = correlated
                            .confirmed_attempts
                            .saturating_add(1)
                            .min(MAX_SUB_CHUNK_RETRIES.saturating_add(1));
                    }
                    if correlated.pending_transport_attempts == 0
                        && correlated.confirmed_attempts == 0
                    {
                        self.requests.correlated_attempts.remove(&key);
                    }
                }
                continue;
            };
            pending.pending_transport_attempts =
                pending.pending_transport_attempts.saturating_sub(1);
            if receipt.id <= pending.answered_transport_packet {
                continue;
            }
            pending.confirmed_attempts = pending
                .confirmed_attempts
                .saturating_add(1)
                .min(MAX_SUB_CHUNK_RETRIES.saturating_add(1));
            if self.requests.admitted_replies.contains_key(&key) {
                if let Some(previous) = pending.response_deadline.take() {
                    self.requests.deadlines.remove(&(previous, key));
                }
                continue;
            }
            if let Some(previous) = pending.response_deadline.replace(deadline) {
                self.requests.deadlines.remove(&(previous, key));
            }
            self.requests.deadlines.insert((deadline, key));
        }
        debug_assert!(self.requests.deadlines.len() <= self.outstanding_sub_chunk_count());
    }

    /// Retires an unencodable demand without publishing air or completing terrain.
    fn retire_unsent_request(&mut self, key: SubChunkKey, demand: u64) {
        if !self.requests.matches_demand(key, demand) {
            return;
        }
        self.requests.collision_failures.insert(key.chunk());
        self.requests
            .unencodable
            .entry(key.chunk())
            .or_default()
            .insert(key.y);
        self.requests.cancel_retry(key);
        if let Some(column) = self.requests.requested.get_mut(&key.chunk()) {
            column.remove(&key.y);
            if column.is_empty() {
                self.requests.requested.remove(&key.chunk());
            }
        }
        self.requests.admitted_replies.remove(&key);
        self.requests.correlated_attempts.remove(&key);
        if let Some(column) = self.requests.recoveries.get_mut(&key.chunk()) {
            column.remove(&key.y);
            if column.is_empty() {
                self.requests.recoveries.remove(&key.chunk());
            }
        }
    }
}

impl SubChunkRequests {
    /// Tests the demand generation as well as its coordinates after retirement or reuse.
    fn matches_demand(&self, key: SubChunkKey, demand: u64) -> bool {
        self.requested
            .get(&key.chunk())
            .and_then(|column| column.get(&key.y))
            .is_some_and(|pending| pending.demand == demand)
    }
}
