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

impl RequestPriority {
    const fn new(sequence: u64, retry: bool) -> Self {
        Self {
            sequence,
            retry,
            transport_retry: false,
            bypasses: 0,
        }
    }

    const fn starved(self) -> bool {
        self.bypasses >= MAX_PRIORITY_BYPASSES
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

/// The request `pop_next` would dispatch, and why.
#[derive(Clone, Copy)]
struct Selection {
    index: usize,
    class: RequestClass,
    transport_retry: bool,
    starved: bool,
}

#[derive(Default)]
pub(super) struct RequestQueue {
    slots: VecDeque<Slot>,
    /// Dispatched priorities kept until transport confirms, so a transport retry keeps its age.
    popped: HashMap<RequestIdentity, RequestPriority>,
    next_sequence: u64,
    mesh_blockers: BTreeSet<ChunkKey>,
    last_popped_class: Option<RequestClass>,
}

impl RequestQueue {
    pub(super) const fn last_popped_class(&self) -> Option<RequestClass> {
        self.last_popped_class
    }

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
        for slot in &self.slots {
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
        let Slot::Reserved { queue_sequence, .. } = self.slots[index] else {
            unreachable!("reservation index names a reservation");
        };
        self.slots[index] = Slot::Ready {
            request,
            priority: RequestPriority::new(queue_sequence, false),
        };
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
        for (index, slot) in self.slots.iter_mut().enumerate() {
            match slot {
                Slot::Reserved { .. } => break,
                Slot::Ready { priority, .. } if index != selected.index => {
                    priority.bypasses = priority.bypasses.saturating_add(1);
                }
                Slot::Ready { .. } => {}
            }
        }
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
        self.last_popped_class = Some(selected.class);
        Some(request)
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
        self.slots.retain(|slot| {
            !matches!(slot, Slot::Reserved { world_sequence: reserved, .. } if *reserved == world_sequence)
        });
    }

    /// Drops queued ready work; reservations stay in place.
    pub(super) fn cancel_ready(&mut self, cancel: impl Fn(&PendingSubChunkRequest) -> bool) {
        self.slots
            .retain(|slot| !matches!(slot, Slot::Ready { request, .. } if cancel(request)));
    }

    /// Drops retired columns' ready work and request identities in one pass each.
    pub(super) fn cancel_columns(&mut self, chunks: &BTreeSet<ChunkKey>) {
        self.cancel_ready(|request| chunks.contains(&request.chunk));
        self.mesh_blockers.retain(|chunk| !chunks.contains(chunk));
        self.popped
            .retain(|identity, _| !chunks.contains(&identity.chunk));
    }

    /// Gives requested neighbours preference within their existing request class.
    pub(super) fn prioritize_mesh_blocker(&mut self, chunk: ChunkKey) {
        self.mesh_blockers.insert(chunk);
    }

    /// Removes a dependency once its column has completed.
    pub(super) fn clear_mesh_blocker(&mut self, chunk: ChunkKey) {
        self.mesh_blockers.remove(&chunk);
    }

    fn reservation_index(&self, world_sequence: u64) -> Option<usize> {
        self.slots.iter().position(|slot| {
            matches!(slot, Slot::Reserved { world_sequence: reserved, .. } if *reserved == world_sequence)
        })
    }

    /// Dispatch order over ready work ahead of the first reservation: the oldest transport
    /// retry, then the oldest starved request, then class, mesh blocker, distance and age.
    fn select(
        &self,
        player_chunk: Option<ChunkKey>,
        required_columns: &BTreeSet<ChunkKey>,
    ) -> Option<Selection> {
        let eligible = || {
            self.slots
                .iter()
                .enumerate()
                .map_while(|(index, slot)| match slot {
                    Slot::Ready { request, priority } => Some((index, request, *priority)),
                    Slot::Reserved { .. } => None,
                })
        };
        let oldest = |pick: fn(RequestPriority) -> bool| {
            eligible()
                .filter(move |(_, _, priority)| pick(*priority))
                .min_by_key(|(_, _, priority)| priority.sequence)
        };
        let class = |request: &PendingSubChunkRequest, priority: RequestPriority| {
            request_class(
                priority.retry,
                request.chunk,
                player_chunk,
                required_columns,
            )
        };
        let (index, request, priority) = oldest(|priority| priority.transport_retry)
            .or_else(|| oldest(RequestPriority::starved))
            .or_else(|| {
                eligible().min_by_key(|(_, request, priority)| {
                    (
                        class(request, *priority),
                        !self.mesh_blockers.contains(&request.chunk),
                        horizontal_distance_squared(request.chunk, player_chunk),
                        priority.sequence,
                    )
                })
            })?;
        Some(Selection {
            index,
            class: class(request, priority),
            transport_retry: priority.transport_retry,
            // A starved transport retry was chosen as a transport retry.
            starved: !priority.transport_retry && priority.starved(),
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

    /// Blocker preference preserves player, starvation and reservation ordering.
    #[test]
    fn requested_mesh_blocker_precedes_same_class_prefetch() {
        let player = ChunkKey::new(0, 0, 0);
        let near = ChunkKey::new(0, 1, 0);
        let blocker = ChunkKey::new(0, 2, 0);
        let mut queue = RequestQueue::default();
        queue.push_ready(request(near, 0), false);
        queue.push_ready(request(blocker, 0), false);
        queue.push_ready(request(player, 0), false);
        queue.prioritize_mesh_blocker(blocker);
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
            blocker
        );
        assert_eq!(
            queue
                .pop_next(Some(player), &BTreeSet::new())
                .unwrap()
                .chunk,
            near
        );
        queue.reserve(1);
        queue.push_ready(request(blocker, 0), false);
        assert!(queue.pop_next(Some(player), &BTreeSet::new()).is_none());
    }
}
