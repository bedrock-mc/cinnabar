use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct RequestIdentity {
    chunk: ChunkKey,
    base_sub_chunk_y: i32,
    count: usize,
    demand: u64,
}

impl ColumnOwned for RequestIdentity {
    /// Dispatched identities retain their request column.
    fn column(&self) -> Option<ChunkKey> {
        Some(self.chunk)
    }
}

impl From<&PendingSubChunkRequest> for RequestIdentity {
    fn from(request: &PendingSubChunkRequest) -> Self {
        Self {
            chunk: request.chunk,
            base_sub_chunk_y: request.base_sub_chunk_y,
            count: request.count,
            demand: request.demand,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RequestPriority {
    sequence: u64,
    retry: bool,
    transport_retry: bool,
}

impl RequestPriority {
    /// Keeps diagnostic class and rollback identity without changing dispatch order.
    const fn new(sequence: u64, retry: bool) -> Self {
        Self {
            sequence,
            retry,
            transport_retry: false,
        }
    }
}

enum Slot {
    /// Holds a world sequence's place; ready work behind it waits.
    Reserved {
        world_sequence: u64,
        queue_sequence: u64,
    },
    Ready {
        request: PendingSubChunkRequest,
        priority: RequestPriority,
    },
}

impl ColumnOwned for Slot {
    /// Unfilled reservations have no column and survive column eviction.
    fn column(&self) -> Option<ChunkKey> {
        match self {
            Self::Ready { request, .. } => Some(request.chunk),
            Self::Reserved { .. } => None,
        }
    }
}

/// The request `pop_next` would dispatch, and why.
#[derive(Clone, Copy)]
struct Selection {
    index: i128,
    class: RequestClass,
    transport_retry: bool,
    starved: bool,
}

#[derive(Default)]
pub(super) struct RequestQueue {
    slots: ColumnQueue<Slot>,
    /// Dispatched priorities kept until transport confirms, so a transport retry keeps its age.
    popped: ColumnRecords<RequestIdentity, RequestPriority>,
    next_sequence: u64,
}

impl RequestQueue {
    pub(super) fn len(&self) -> usize {
        self.slots.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub(super) fn ready_requests(&self) -> impl Iterator<Item = &PendingSubChunkRequest> {
        self.slots.iter().filter_map(|slot| match slot {
            Slot::Ready { request, .. } => Some(request),
            Slot::Reserved { .. } => None,
        })
    }

    pub(super) fn evidence(
        &self,
        player_chunk: Option<ChunkKey>,
        required_columns: &BTreeSet<ChunkKey>,
    ) -> RequestQueueEvidence {
        let mut evidence = RequestQueueEvidence::default();
        for slot in self.slots.iter() {
            match slot {
                Slot::Reserved { .. } => evidence.reservations += 1,
                Slot::Ready { request, priority } => {
                    let class = request_class(
                        priority.retry,
                        request.chunk,
                        player_chunk,
                        required_columns,
                    );
                    let depth = &mut evidence.class_depths[class.index()];
                    depth.ready += 1;
                    if evidence.reservations == 0 {
                        depth.eligible += 1;
                    } else {
                        evidence.ready_blocked_by_reservation += 1;
                    }
                }
            }
        }
        if let Some(next) = self.select(player_chunk, required_columns) {
            evidence.next_class = Some(next.class);
            evidence.next_is_transport_retry = next.transport_retry;
            evidence.next_is_starved = next.starved;
        }
        evidence
    }

    pub(super) fn reserve(&mut self, world_sequence: u64) {
        let queue_sequence = self.allocate_sequence();
        self.slots.push_back(Slot::Reserved {
            world_sequence,
            queue_sequence,
        });
    }

    pub(super) fn has_reservation(&self, world_sequence: u64) -> bool {
        self.reservation_index(world_sequence).is_some()
    }

    /// Fills a reservation in place; the request keeps the reservation's age.
    pub(super) fn replace_reservation(
        &mut self,
        world_sequence: u64,
        request: PendingSubChunkRequest,
    ) -> bool {
        let Some(index) = self.reservation_index(world_sequence) else {
            return false;
        };
        let Slot::Reserved { queue_sequence, .. } = self.slots.get(index).expect("a reserved slot")
        else {
            unreachable!("reservation index names a reservation");
        };
        self.slots.replace(
            index,
            Slot::Ready {
                request,
                priority: RequestPriority::new(*queue_sequence, false),
            },
        );
        true
    }

    pub(super) fn push_ready(&mut self, request: PendingSubChunkRequest, retry: bool) {
        let priority = RequestPriority::new(self.allocate_sequence(), retry);
        self.slots.push_back(Slot::Ready { request, priority });
    }

    /// Requeues work transport failed to send ahead of everything else.
    pub(super) fn retry_front(&mut self, request: PendingSubChunkRequest) {
        let mut priority = self
            .popped
            .remove(&RequestIdentity::from(&request))
            .unwrap_or_else(|| RequestPriority::new(self.allocate_sequence(), false));
        priority.transport_retry = true;
        self.slots.push_front(Slot::Ready { request, priority });
    }

    pub(super) fn pop_next(
        &mut self,
        player_chunk: Option<ChunkKey>,
        required_columns: &BTreeSet<ChunkKey>,
    ) -> Option<PendingSubChunkRequest> {
        let selected = self.select(player_chunk, required_columns)?;
        let Some(Slot::Ready { request, priority }) = self.slots.remove(selected.index) else {
            unreachable!("selection names ready work");
        };
        let identity = RequestIdentity::from(&request);
        if self.popped.len() >= OUTBOUND_REQUEST_CAPACITY
            && !self.popped.contains_key(&identity)
            && let Some(oldest) = self
                .popped
                .iter()
                .min_by_key(|(_, priority)| priority.sequence)
                .map(|(identity, _)| *identity)
        {
            self.popped.remove(&oldest);
        }
        self.popped.insert(identity, priority);
        Some(request)
    }

    /// Releases the popped intent and returns its retry classification for telemetry.
    pub(super) fn confirm_popped(&mut self, request: &PendingSubChunkRequest) -> Option<bool> {
        self.popped
            .remove(&RequestIdentity::from(request))
            .map(|priority| priority.retry)
    }

    pub(super) fn cancel_reservation(&mut self, world_sequence: u64) {
        self.slots.retain(|slot| {
            !matches!(slot, Slot::Reserved { world_sequence: reserved, .. } if *reserved == world_sequence)
        });
    }

    /// Drops queued ready work; reservations stay in place.
    pub(super) fn cancel_ready(&mut self, cancel: impl Fn(&PendingSubChunkRequest) -> bool) {
        self.slots
            .retain(|slot| !matches!(slot, Slot::Ready { request, .. } if cancel(request)));
    }

    /// Retires indexed ready work and dispatched identities without visiting other columns.
    pub(super) fn cancel_columns(&mut self, chunks: &BTreeSet<ChunkKey>) {
        for &chunk in chunks {
            self.slots.remove_column(chunk);
            self.popped.remove_column(chunk);
        }
    }

    /// Reads and resets visits made by column retirement of ready and popped requests.
    #[cfg(test)]
    pub(super) fn take_cleanup_visits(&mut self) -> usize {
        self.slots.take_cleanup_visits() + self.popped.take_cleanup_visits()
    }

    fn reservation_index(&self, world_sequence: u64) -> Option<i128> {
        self.slots.entries().find_map(|(position, slot)| {
            matches!(slot, Slot::Reserved { world_sequence: reserved, .. } if *reserved == world_sequence).then_some(position)
        })
    }

    /// Returns only the oldest ready intent; an unfilled reservation is a barrier.
    fn select(
        &self,
        player_chunk: Option<ChunkKey>,
        required_columns: &BTreeSet<ChunkKey>,
    ) -> Option<Selection> {
        let (index, slot) = self.slots.entries().next()?;
        let Slot::Ready { request, priority } = slot else {
            return None;
        };
        Some(Selection {
            index,
            class: request_class(
                priority.retry,
                request.chunk,
                player_chunk,
                required_columns,
            ),
            transport_retry: priority.transport_retry,
            starved: false,
        })
    }

    fn allocate_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .expect("outbound request sequence space exhausted");
        sequence
    }
}

pub(super) fn request_class(
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one bounded intent for queue ordering checks.
    fn request(chunk: ChunkKey, y: i32) -> PendingSubChunkRequest {
        PendingSubChunkRequest {
            demand: 0,
            chunk,
            base_sub_chunk_y: y,
            count: 1,
        }
    }

    /// Reservation replacement and transport retries preserve the derived column index.
    #[test]
    fn column_index_tracks_request_queue_mutations_and_order() {
        let a = ChunkKey::new(0, 0, 0);
        let b = ChunkKey::new(0, 4, 0);
        let mut queue = RequestQueue::default();
        queue.reserve(7);
        queue.push_ready(request(b, 0), false);
        queue.slots.assert_consistent();
        assert!(queue.replace_reservation(7, request(a, 0)));
        queue.slots.assert_consistent();
        let popped = queue.pop_next(None, &BTreeSet::new()).unwrap();
        queue.slots.assert_consistent();
        queue.retry_front(popped);
        queue.slots.assert_consistent();
        queue.reserve(8);
        queue.cancel_reservation(8);
        queue.slots.assert_consistent();
        queue.cancel_ready(|request| request.chunk == b);
        queue.slots.assert_consistent();
        queue.push_ready(request(b, 1), false);
        queue.cancel_columns(&BTreeSet::from([a]));
        queue.slots.assert_consistent();
        assert_eq!(queue.pop_next(None, &BTreeSet::new()).unwrap().chunk, b);
        queue.slots.assert_consistent();
        assert!(queue.is_empty());
    }

    #[test]
    fn admitted_requests_keep_order_before_player_and_mesh_work() {
        let player = ChunkKey::new(0, 0, 0);
        let first = ChunkKey::new(0, 8, 0);
        let mut queue = RequestQueue::default();
        queue.push_ready(request(first, -4), false);
        queue.push_ready(request(player, -4), true);

        assert_eq!(
            queue
                .pop_next(Some(player), &BTreeSet::from([player]))
                .unwrap()
                .chunk,
            first,
            "admitted work keeps its order across request classes"
        );
    }

    #[test]
    fn retry_waits_for_the_previously_admitted_prefix() {
        let player = ChunkKey::new(0, 0, 0);
        let mut queue = RequestQueue::default();
        for x in 1..=32 {
            queue.push_ready(request(ChunkKey::new(0, x, 0), -4), false);
        }
        queue.push_ready(request(player, -4), true);
        for x in 1..=32 {
            assert_eq!(
                queue
                    .pop_next(Some(player), &BTreeSet::new())
                    .unwrap()
                    .chunk
                    .x,
                x
            );
        }
        assert_eq!(
            queue
                .pop_next(Some(player), &BTreeSet::new())
                .unwrap()
                .chunk,
            player
        );
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
            prefetch
        );
        assert_eq!(
            queue
                .pop_next(Some(player), &BTreeSet::new())
                .unwrap()
                .chunk,
            player
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

    /// Required and player columns keep the same insertion order as other work.
    #[test]
    fn required_columns_do_not_reorder_admitted_intents() {
        let player = ChunkKey::new(0, 0, 0);
        let near = ChunkKey::new(0, 1, 0);
        let blocker = ChunkKey::new(0, 2, 0);
        let mut queue = RequestQueue::default();
        for chunk in [near, blocker, player] {
            queue.push_ready(request(chunk, 0), false);
        }
        for expected in [near, blocker, player] {
            assert_eq!(
                queue
                    .pop_next(Some(player), &BTreeSet::from([blocker]))
                    .unwrap()
                    .chunk,
                expected
            );
        }
        queue.reserve(1);
        queue.push_ready(request(blocker, 0), false);
        assert!(queue.pop_next(Some(player), &BTreeSet::new()).is_none());
    }
}
