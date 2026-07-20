use super::*;

const MAX_PRIORITY_BYPASSES: u8 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RequestIdentity {
    dimension: i32,
    chunk: ChunkKey,
    base_sub_chunk_y: i32,
    count: usize,
}

impl From<&PendingSubChunkRequest> for RequestIdentity {
    fn from(request: &PendingSubChunkRequest) -> Self {
        Self {
            dimension: request.dimension,
            chunk: request.chunk,
            base_sub_chunk_y: request.base_sub_chunk_y,
            count: request.count,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RequestPriority {
    sequence: u64,
    retry: bool,
    transport_retry: bool,
    bypasses: u8,
}

#[derive(Default)]
pub(super) struct RequestQueue {
    slots: VecDeque<OutboundRequestSlot>,
    priorities: HashMap<RequestIdentity, RequestPriority>,
    popped: HashMap<RequestIdentity, RequestPriority>,
    reservations: HashMap<u64, u64>,
    next_sequence: u64,
    last_popped_class: Option<RequestClass>,
}

impl std::ops::Deref for RequestQueue {
    type Target = VecDeque<OutboundRequestSlot>;

    fn deref(&self) -> &Self::Target {
        &self.slots
    }
}

impl std::ops::DerefMut for RequestQueue {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.slots
    }
}

impl RequestQueue {
    pub(super) const fn last_popped_class(&self) -> Option<RequestClass> {
        self.last_popped_class
    }

    pub(super) fn evidence(
        &self,
        player_chunk: Option<ChunkKey>,
        required_columns: &BTreeSet<ChunkKey>,
    ) -> RequestQueueEvidence {
        #[derive(Clone, Copy)]
        struct Candidate {
            class: RequestClass,
            sequence: u64,
            distance: u128,
            transport_retry: bool,
            starved: bool,
        }

        let barrier = self
            .slots
            .iter()
            .position(|slot| matches!(slot, OutboundRequestSlot::Reserved(_)))
            .unwrap_or(self.slots.len());
        let mut evidence = RequestQueueEvidence {
            reservations: self
                .slots
                .iter()
                .filter(|slot| matches!(slot, OutboundRequestSlot::Reserved(_)))
                .count(),
            ..Default::default()
        };
        let mut candidates = Vec::with_capacity(OUTBOUND_REQUEST_CAPACITY);
        for (index, slot) in self.slots.iter().enumerate() {
            let OutboundRequestSlot::Ready(request) = slot else {
                continue;
            };
            let identity = RequestIdentity::from(request);
            let priority = self.priorities.get(&identity);
            let class = request_class(
                priority.is_some_and(|priority| priority.retry),
                request.chunk,
                player_chunk,
                required_columns,
            );
            evidence.class_depths[class.index()].ready += 1;
            if index < barrier {
                evidence.class_depths[class.index()].eligible += 1;
                candidates.push(Candidate {
                    class,
                    sequence: priority.map_or(u64::MAX, |priority| priority.sequence),
                    distance: horizontal_distance_squared(request.chunk, player_chunk),
                    transport_retry: priority.is_some_and(|priority| priority.transport_retry),
                    starved: priority
                        .is_some_and(|priority| priority.bypasses >= MAX_PRIORITY_BYPASSES),
                });
            } else {
                evidence.ready_blocked_by_reservation += 1;
            }
        }

        let transport_retry = candidates
            .iter()
            .filter(|candidate| candidate.transport_retry)
            .min_by_key(|candidate| candidate.sequence);
        let starved = candidates
            .iter()
            .filter(|candidate| candidate.starved)
            .min_by_key(|candidate| candidate.sequence);
        let (next, transport_selected, starved_selected) = if let Some(next) = transport_retry {
            (Some(next), true, false)
        } else if let Some(next) = starved {
            (Some(next), false, true)
        } else {
            (
                candidates.iter().min_by_key(|candidate| {
                    (candidate.class, candidate.distance, candidate.sequence)
                }),
                false,
                false,
            )
        };
        evidence.next_class = next.map(|candidate| candidate.class);
        evidence.next_is_transport_retry = transport_selected;
        evidence.next_is_starved = starved_selected;
        evidence
    }

    pub(super) fn reserve(&mut self, world_sequence: u64) {
        let queue_sequence = self.allocate_sequence();
        self.reservations.insert(world_sequence, queue_sequence);
        self.slots
            .push_back(OutboundRequestSlot::Reserved(world_sequence));
    }

    pub(super) fn replace_reservation(
        &mut self,
        world_sequence: u64,
        request: PendingSubChunkRequest,
    ) -> bool {
        let Some(index) = self.slots.iter().position(|slot| {
            matches!(slot, OutboundRequestSlot::Reserved(reserved) if *reserved == world_sequence)
        }) else {
            return false;
        };
        let sequence = self
            .reservations
            .remove(&world_sequence)
            .unwrap_or_else(|| self.allocate_sequence());
        let identity = RequestIdentity::from(&request);
        self.slots[index] = OutboundRequestSlot::Ready(request);
        self.priorities.insert(
            identity,
            RequestPriority {
                sequence,
                retry: false,
                transport_retry: false,
                bypasses: 0,
            },
        );
        true
    }

    pub(super) fn push_ready(&mut self, request: PendingSubChunkRequest, retry: bool) {
        let identity = RequestIdentity::from(&request);
        let priority = RequestPriority {
            sequence: self.allocate_sequence(),
            retry,
            transport_retry: false,
            bypasses: 0,
        };
        self.priorities.insert(identity, priority);
        self.slots.push_back(OutboundRequestSlot::Ready(request));
    }

    pub(super) fn retry_front(&mut self, request: PendingSubChunkRequest) {
        let identity = RequestIdentity::from(&request);
        let mut priority = self
            .popped
            .remove(&identity)
            .unwrap_or_else(|| RequestPriority {
                sequence: self.allocate_sequence(),
                retry: false,
                transport_retry: true,
                bypasses: 0,
            });
        priority.transport_retry = true;
        self.priorities.insert(identity, priority);
        self.slots.push_front(OutboundRequestSlot::Ready(request));
    }

    pub(super) fn pop_next(
        &mut self,
        player_chunk: Option<ChunkKey>,
        required_columns: &BTreeSet<ChunkKey>,
    ) -> Option<PendingSubChunkRequest> {
        self.synchronize_priorities();
        let barrier = self
            .slots
            .iter()
            .position(|slot| matches!(slot, OutboundRequestSlot::Reserved(_)))
            .unwrap_or(self.slots.len());
        let candidates = self
            .slots
            .iter()
            .take(barrier)
            .enumerate()
            .filter_map(|(index, slot)| match slot {
                OutboundRequestSlot::Ready(request) => {
                    Some((index, RequestIdentity::from(request)))
                }
                OutboundRequestSlot::Reserved(_) => None,
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return None;
        }

        let transport_retry = candidates
            .iter()
            .filter(|(_, identity)| self.priorities[identity].transport_retry)
            .min_by_key(|(_, identity)| self.priorities[identity].sequence)
            .copied();
        let starved = candidates
            .iter()
            .filter(|(_, identity)| {
                self.priorities
                    .get(identity)
                    .is_some_and(|priority| priority.bypasses >= MAX_PRIORITY_BYPASSES)
            })
            .min_by_key(|(_, identity)| self.priorities[identity].sequence)
            .copied();
        let selected = transport_retry.or(starved).unwrap_or_else(|| {
            candidates
                .iter()
                .min_by_key(|(index, identity)| {
                    let request = match &self.slots[*index] {
                        OutboundRequestSlot::Ready(request) => request,
                        OutboundRequestSlot::Reserved(_) => unreachable!(),
                    };
                    let priority = self.priorities[identity];
                    (
                        request_class(
                            priority.retry,
                            request.chunk,
                            player_chunk,
                            required_columns,
                        ),
                        horizontal_distance_squared(request.chunk, player_chunk),
                        priority.sequence,
                    )
                })
                .copied()
                .expect("non-empty request candidates have a minimum")
        });

        for (_, identity) in &candidates {
            if *identity != selected.1
                && let Some(priority) = self.priorities.get_mut(identity)
            {
                priority.bypasses = priority.bypasses.saturating_add(1);
            }
        }
        let priority = self
            .priorities
            .remove(&selected.1)
            .expect("selected request has priority metadata");
        let selected_class = match &self.slots[selected.0] {
            OutboundRequestSlot::Ready(request) => request_class(
                priority.retry,
                request.chunk,
                player_chunk,
                required_columns,
            ),
            OutboundRequestSlot::Reserved(_) => unreachable!(),
        };
        if self.popped.len() >= OUTBOUND_REQUEST_CAPACITY
            && !self.popped.contains_key(&selected.1)
            && let Some(oldest) = self
                .popped
                .iter()
                .min_by_key(|(_, priority)| priority.sequence)
                .map(|(identity, _)| *identity)
        {
            self.popped.remove(&oldest);
        }
        self.popped.insert(selected.1, priority);
        self.last_popped_class = Some(selected_class);
        match self.slots.remove(selected.0) {
            Some(OutboundRequestSlot::Ready(request)) => Some(request),
            Some(OutboundRequestSlot::Reserved(_)) | None => unreachable!(),
        }
    }

    pub(super) fn confirm_popped(&mut self, request: &PendingSubChunkRequest) {
        self.popped.remove(&RequestIdentity::from(request));
    }

    pub(super) fn confirm_popped_identity(
        &mut self,
        chunk: ChunkKey,
        base_sub_chunk_y: i32,
        count: usize,
    ) {
        self.popped.retain(|identity, _| {
            identity.chunk != chunk
                || identity.base_sub_chunk_y != base_sub_chunk_y
                || identity.count != count
        });
    }

    pub(super) fn cancel_reservation(&mut self, world_sequence: u64) {
        self.reservations.remove(&world_sequence);
        self.slots.retain(|slot| {
            !matches!(slot, OutboundRequestSlot::Reserved(reserved) if *reserved == world_sequence)
        });
    }

    pub(super) fn forget_column(&mut self, chunk: ChunkKey) {
        self.priorities
            .retain(|identity, _| identity.chunk != chunk);
        self.popped.retain(|identity, _| identity.chunk != chunk);
    }

    fn allocate_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .expect("outbound request sequence space exhausted");
        sequence
    }

    fn synchronize_priorities(&mut self) {
        let ready = self
            .slots
            .iter()
            .filter_map(|slot| match slot {
                OutboundRequestSlot::Ready(request) => Some(RequestIdentity::from(request)),
                OutboundRequestSlot::Reserved(_) => None,
            })
            .collect::<Vec<_>>();
        let live = ready.iter().copied().collect::<HashSet<_>>();
        self.priorities
            .retain(|identity, _| live.contains(identity));
        for identity in ready {
            if !self.priorities.contains_key(&identity) {
                let sequence = self.allocate_sequence();
                self.priorities.insert(
                    identity,
                    RequestPriority {
                        sequence,
                        retry: false,
                        transport_retry: false,
                        bypasses: 0,
                    },
                );
            }
        }
    }
}

fn request_class(
    retry: bool,
    chunk: ChunkKey,
    player_chunk: Option<ChunkKey>,
    required_columns: &BTreeSet<ChunkKey>,
) -> RequestClass {
    if player_chunk == Some(chunk) {
        if retry {
            RequestClass::PlayerRetry
        } else {
            RequestClass::PlayerInitial
        }
    } else if required_columns.contains(&chunk) {
        if retry {
            RequestClass::VisibleRetry
        } else {
            RequestClass::VisibleInitial
        }
    } else if retry {
        RequestClass::PrefetchRetry
    } else {
        RequestClass::PrefetchInitial
    }
}

fn horizontal_distance_squared(chunk: ChunkKey, player_chunk: Option<ChunkKey>) -> u128 {
    let Some(player) = player_chunk.filter(|player| player.dimension == chunk.dimension) else {
        return 0;
    };
    let dx = i128::from(chunk.x) - i128::from(player.x);
    let dz = i128::from(chunk.z) - i128::from(player.z);
    dx.unsigned_abs()
        .pow(2)
        .saturating_add(dz.unsigned_abs().pow(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(chunk: ChunkKey, y: i32) -> PendingSubChunkRequest {
        PendingSubChunkRequest {
            packet: request_sub_chunk_column(chunk.dimension, chunk.x, chunk.z, y, 1).unwrap(),
            dimension: chunk.dimension,
            chunk,
            base_sub_chunk_y: y,
            count: 1,
        }
    }

    #[test]
    fn continuous_prefetch_cannot_starve_exact_retry() {
        let player = ChunkKey::new(0, 0, 0);
        let mut queue = RequestQueue::default();
        for x in 1..=32 {
            queue.push_ready(request(ChunkKey::new(0, x, 0), -4), false);
        }
        queue.push_ready(request(player, -4), true);

        assert_eq!(
            queue
                .pop_next(Some(player), &BTreeSet::new())
                .unwrap()
                .chunk,
            player
        );
    }

    #[test]
    fn bounded_aging_eventually_services_prefetch_under_continuous_player_work() {
        let player = ChunkKey::new(0, 0, 0);
        let prefetch = ChunkKey::new(0, 8, 0);
        let mut queue = RequestQueue::default();
        queue.push_ready(request(prefetch, -4), false);
        for y in 0..=i32::from(MAX_PRIORITY_BYPASSES) {
            queue.push_ready(request(player, y), true);
        }

        let mut served_prefetch = false;
        for _ in 0..=MAX_PRIORITY_BYPASSES {
            served_prefetch |= queue
                .pop_next(Some(player), &BTreeSet::new())
                .is_some_and(|request| request.chunk == prefetch);
        }
        assert!(served_prefetch);
    }

    #[test]
    fn unresolved_reservation_blocks_later_ready_work_without_losing_identity() {
        let player = ChunkKey::new(0, 0, 0);
        let prefetch = ChunkKey::new(0, 8, 0);
        let mut queue = RequestQueue::default();
        queue.reserve(7);
        queue.push_ready(request(player, -4), true);

        assert!(queue.pop_next(Some(player), &BTreeSet::new()).is_none());
        assert!(queue.replace_reservation(7, request(prefetch, -4)));
        assert_eq!(
            queue
                .pop_next(Some(player), &BTreeSet::new())
                .unwrap()
                .chunk,
            player
        );
        assert_eq!(
            queue
                .pop_next(Some(player), &BTreeSet::new())
                .unwrap()
                .chunk,
            prefetch
        );
    }

    #[test]
    fn unsent_transport_retry_precedes_new_higher_class_work() {
        let player = ChunkKey::new(0, 0, 0);
        let unsent = ChunkKey::new(0, 8, 0);
        let mut queue = RequestQueue::default();
        queue.retry_front(request(unsent, -4));
        queue.push_ready(request(player, -4), true);

        assert_eq!(
            queue
                .pop_next(Some(player), &BTreeSet::new())
                .unwrap()
                .chunk,
            unsent
        );
    }

    #[test]
    fn unconfirmed_popped_identity_retention_is_hard_bounded() {
        let mut queue = RequestQueue::default();
        for x in 0..i32::try_from(OUTBOUND_REQUEST_CAPACITY + 8).unwrap() {
            queue.push_ready(request(ChunkKey::new(0, x, 0), -4), false);
            queue.pop_next(None, &BTreeSet::new()).unwrap();
        }

        assert_eq!(queue.popped.len(), OUTBOUND_REQUEST_CAPACITY);
    }

    #[test]
    fn evidence_reports_fixed_priority_depths_barriers_and_actual_next_reason() {
        let player = ChunkKey::new(0, 0, 0);
        let visible = ChunkKey::new(0, 2, 0);
        let prefetch = ChunkKey::new(0, 8, 0);
        let required = BTreeSet::from([player, visible]);
        let mut queue = RequestQueue::default();
        queue.push_ready(request(prefetch, -4), false);
        queue.reserve(7);
        queue.push_ready(request(player, -4), true);
        queue.push_ready(request(visible, -4), false);

        let evidence = queue.evidence(Some(player), &required);

        assert_eq!(
            evidence
                .class_depths
                .map(|depth| (depth.class, depth.ready, depth.eligible)),
            [
                (RequestClass::PlayerRetry, 1, 0),
                (RequestClass::PlayerInitial, 0, 0),
                (RequestClass::VisibleRetry, 0, 0),
                (RequestClass::VisibleInitial, 1, 0),
                (RequestClass::PrefetchRetry, 0, 0),
                (RequestClass::PrefetchInitial, 1, 1),
            ]
        );
        assert_eq!(evidence.reservations, 1);
        assert_eq!(evidence.ready_blocked_by_reservation, 2);
        assert_eq!(evidence.next_class, Some(RequestClass::PrefetchInitial));
        assert!(!evidence.next_is_transport_retry);
        assert!(!evidence.next_is_starved);

        let unsent = queue.pop_next(Some(player), &required).unwrap();
        queue.retry_front(unsent);
        let retry = queue.evidence(Some(player), &required);
        assert_eq!(retry.next_class, Some(RequestClass::PrefetchInitial));
        assert!(retry.next_is_transport_retry);
        assert!(!retry.next_is_starved);
    }
}
