use super::*;

pub(super) const NEAR_CAMERA_RADIUS: i32 = 4;

/// Longest wait, in startup ingress polls, before a still-blocked promoted job is rechecked.
const MAX_RECHECK_BACKOFF_POLLS: u64 = 32;

/// Ready work leads during ordinary streaming; startup also rechecks higher-priority deferred work.
#[derive(Default)]
pub(super) struct Lane {
    pub(super) ready: BinaryHeap<PendingSchedulerCandidate>,
    pub(super) deferred: BinaryHeap<PendingSchedulerCandidate>,
    /// Promoted startup revisions: attempts and the poll before which a blocked one stays deferred.
    rechecks: HashMap<SubChunkKey, Recheck>,
    polls: u64,
    held: Vec<PendingSchedulerCandidate>, // cooldown scratch, reused across polls
}

#[derive(Clone, Copy)]
struct Recheck {
    revision: u64,
    attempts: u32,
    not_before: u64,
}

impl Lane {
    pub(super) fn is_empty(&self) -> bool {
        self.ready.is_empty() && self.deferred.is_empty()
    }

    fn heap_mut(&mut self, deferred: bool) -> &mut BinaryHeap<PendingSchedulerCandidate> {
        if deferred {
            &mut self.deferred
        } else {
            &mut self.ready
        }
    }

    /// Moves current deferred work that outranks the best ready job back to ready, within budget.
    /// A revision that keeps returning blocked backs off exponentially, so it cannot claim every
    /// deadline-limited dispatch ahead of ready work.
    fn prioritize_deferred(
        &mut self,
        budget: &mut usize,
        deadline: Option<Instant>,
        is_current: impl Fn(SubChunkKey, u64) -> bool,
    ) {
        self.polls += 1;
        let ready_best = self.ready.peek().copied();
        let mut examined = false;
        while *budget != 0
            && (!examined || deadline.is_none_or(|deadline| Instant::now() < deadline))
        {
            let Some(candidate) = self.deferred.peek().copied() else {
                break;
            };
            if ready_best.is_some_and(|ready| candidate <= ready) {
                break;
            }
            self.deferred.pop();
            *budget -= 1;
            examined = true;
            if !is_current(candidate.key, candidate.revision) {
                continue;
            }
            let recheck = self
                .rechecks
                .get(&candidate.key)
                .filter(|recheck| recheck.revision == candidate.revision)
                .copied();
            if recheck.is_some_and(|recheck| self.polls < recheck.not_before) {
                self.held.push(candidate);
                continue;
            }
            let attempts = recheck.map_or(0, |recheck| recheck.attempts);
            self.rechecks.insert(
                candidate.key,
                Recheck {
                    revision: candidate.revision,
                    attempts: attempts + 1,
                    not_before: self.polls
                        + (1_u64 << attempts.min(5)).min(MAX_RECHECK_BACKOFF_POLLS),
                },
            );
            self.ready.push(candidate);
        }
        self.deferred.extend(self.held.drain(..));
    }
}

/// Re-ranks a bounded part of existing queues after the view changes.
pub(super) struct SchedulerRefresh<const L: usize> {
    previous: [Lane; L],
    view: Option<SchedulerView>,
}

impl<const L: usize> Default for SchedulerRefresh<L> {
    fn default() -> Self {
        Self {
            previous: std::array::from_fn(|_| Lane::default()),
            view: None,
        }
    }
}

impl<const L: usize> SchedulerRefresh<L> {
    /// Refreshes bounded queue work and reports whether the view still needs direct local probes.
    pub(super) fn refresh(
        &mut self,
        view: SchedulerView,
        queues: &mut [Lane; L],
        deadline: Option<Instant>,
        is_current: impl Fn(SubChunkKey, u64) -> bool,
    ) -> bool {
        let moved = self.view.is_none_or(|previous| {
            previous.cell() != view.cell() || previous.startup_center != view.startup_center
        });
        if self.previous.iter().all(Lane::is_empty) && moved {
            std::mem::swap(&mut self.previous, queues);
            self.view = Some(view);
        }
        let probe_near = moved || self.previous.iter().any(|lane| !lane.is_empty());
        let mut refreshed = false;
        for _ in 0..MAX_PENDING_SCHEDULER_SCANS_PER_POLL {
            if refreshed && deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                break;
            }
            let Some((lane, deferred)) =
                self.previous.iter().enumerate().find_map(|(index, lane)| {
                    if !lane.ready.is_empty() {
                        Some((index, false))
                    } else if !lane.deferred.is_empty() {
                        Some((index, true))
                    } else {
                        None
                    }
                })
            else {
                break;
            };
            let mut candidate = self.previous[lane]
                .heap_mut(deferred)
                .pop()
                .expect("nonempty refresh queue");
            if is_current(candidate.key, candidate.revision) {
                candidate.refresh_rank(view);
                queues[lane].heap_mut(deferred).push(candidate);
                refreshed = true;
            }
        }
        probe_near
    }
}

/// A queued record superseded by a newer revision of its key.
pub(super) trait PendingJob: Copy {
    fn revision(&self) -> u64;
    fn urgent(&self) -> bool;
}

impl PendingJob for PendingLight {
    fn revision(&self) -> u64 {
        self.revision
    }
    fn urgent(&self) -> bool {
        self.urgent
    }
}

impl PendingJob for PendingMesh {
    fn revision(&self) -> u64 {
        self.revision
    }
    fn urgent(&self) -> bool {
        self.urgent
    }
}

/// Per-key worker jobs ordered by the camera: each key's latest pending revision, its lazily
/// invalidated scan and lane queues, and the jobs already dispatched.
pub(super) struct KeyedJobs<P, J, const L: usize> {
    pub(super) pending: HashMap<SubChunkKey, P>,
    /// Urgent records at the front, ordinary ones at the back.
    pub(super) scan: VecDeque<(SubChunkKey, u64)>,
    /// Startup dependencies, ingressed after urgent records and before ordinary ones.
    pub(super) startup_scan: VecDeque<(SubChunkKey, u64)>,
    pub(super) lanes: [Lane; L],
    pub(super) refresh: SchedulerRefresh<L>,
    pub(super) in_flight: HashMap<SubChunkKey, J>,
}

impl<P, J, const L: usize> Default for KeyedJobs<P, J, L> {
    fn default() -> Self {
        Self {
            pending: HashMap::new(),
            scan: VecDeque::new(),
            startup_scan: VecDeque::new(),
            lanes: std::array::from_fn(|_| Lane::default()),
            refresh: SchedulerRefresh::default(),
            in_flight: HashMap::new(),
        }
    }
}

impl<P: PendingJob, J, const L: usize> KeyedJobs<P, J, L> {
    /// Replaces the key's pending record and queues it for ingress.
    pub(super) fn enqueue(&mut self, key: SubChunkKey, pending: P) {
        self.enqueue_prioritized(key, pending, false);
    }

    /// Startup dependencies enter ingress ahead of ordinary work but behind urgent records.
    pub(super) fn enqueue_prioritized(&mut self, key: SubChunkKey, pending: P, startup: bool) {
        if startup && !pending.urgent() {
            self.startup_scan.push_back((key, pending.revision()));
        } else {
            self.rescan(key, pending.revision(), pending.urgent());
        }
        self.pending.insert(key, pending);
    }

    pub(super) fn rescan(&mut self, key: SubChunkKey, revision: u64, urgent: bool) {
        if urgent {
            self.scan.push_front((key, revision));
        } else {
            self.scan.push_back((key, revision));
        }
    }

    /// Drops queued work; dispatched jobs keep their records.
    pub(super) fn clear_queued(&mut self) {
        self.pending.clear();
        self.scan.clear();
        self.startup_scan.clear();
        for lane in &mut self.lanes {
            lane.ready.clear();
            lane.deferred.clear();
        }
    }

    /// Re-ranks queues for `view`, then moves bounded scan ingress into the lane `route` picks
    /// (`true` for ready); reports whether the view still needs direct local probes.
    pub(super) fn ingress(
        &mut self,
        view: SchedulerView,
        deadline: Option<Instant>,
        mut route: impl FnMut(SubChunkKey, u64, P) -> (usize, bool),
    ) -> bool {
        let pending = &self.pending;
        let is_current = |key, revision| {
            pending
                .get(&key)
                .is_some_and(|pending: &P| pending.revision() == revision)
        };
        let probe_near = self
            .refresh
            .refresh(view, &mut self.lanes, deadline, is_current);
        compact_scheduler_scan(&mut self.scan, pending.len(), is_current);
        compact_scheduler_scan(&mut self.startup_scan, pending.len(), is_current);
        let ingress_budget =
            (self.scan.len() + self.startup_scan.len()).min(MAX_PENDING_MESH_QUEUE_WORK_PER_POLL);
        let mut ingressed = false;
        for index in 0..ingress_budget {
            if deadline.is_some_and(|deadline| Instant::now() >= deadline)
                && (ingressed || index >= MAX_PENDING_SCHEDULER_SCANS_PER_POLL)
            {
                break;
            }
            // Urgent records sit at the scan front; startup work follows them, then ordinary work.
            // Superseded heads go first so a live urgent record behind them is still seen.
            while self.scan.front().is_some_and(|&(key, revision)| {
                self.pending
                    .get(&key)
                    .is_none_or(|pending| pending.revision() != revision)
            }) {
                self.scan.pop_front();
            }
            let urgent_front = self.scan.front().is_some_and(|&(key, revision)| {
                self.pending
                    .get(&key)
                    .is_some_and(|pending| pending.revision() == revision && pending.urgent())
            });
            let next = if urgent_front || self.startup_scan.is_empty() {
                self.scan.pop_front()
            } else {
                self.startup_scan.pop_front()
            };
            let Some((key, queued_revision)) = next else {
                break;
            };
            let Some(pending) = self
                .pending
                .get(&key)
                .copied()
                .filter(|pending| pending.revision() == queued_revision)
            else {
                continue;
            };
            let candidate =
                PendingSchedulerCandidate::new(key, queued_revision, view, pending.urgent());
            ingressed = true;
            let (lane, ready) = route(key, queued_revision, pending);
            self.lanes[lane].heap_mut(!ready).push(candidate);
        }
        let mut deferred_budget = MAX_PENDING_SCHEDULER_SCANS_PER_POLL;
        for lane in &mut self.lanes {
            if lane.ready.is_empty() {
                std::mem::swap(&mut lane.ready, &mut lane.deferred);
            }
            if view.startup_center.is_some() {
                lane.prioritize_deferred(&mut deferred_budget, deadline, is_current);
            } else if !lane.rechecks.is_empty() {
                lane.rechecks = HashMap::new();
            }
        }
        probe_near
    }
}

/// Drops superseded and duplicate scan entries once they outnumber live work
/// plus one poll of ingress, so a stationary dirty storm cannot grow the scan
/// history without bound. Surviving entries keep their queue order.
fn compact_scheduler_scan(
    scan: &mut VecDeque<(SubChunkKey, u64)>,
    live_len: usize,
    is_live: impl Fn(SubChunkKey, u64) -> bool,
) {
    if scan.len()
        <= live_len
            .saturating_mul(2)
            .saturating_add(MAX_PENDING_MESH_QUEUE_WORK_PER_POLL)
    {
        return;
    }
    let mut seen = HashSet::with_capacity(live_len);
    scan.retain(|&(key, revision)| is_live(key, revision) && seen.insert(key));
}

/// Probes the camera's immediate sub-chunk neighbourhood without scanning pending maps.
pub(super) fn near_camera_keys(
    view: SchedulerView,
    dimension: i32,
) -> impl Iterator<Item = SubChunkKey> {
    let [x, y, z] = view
        .position
        .map(|value| floor_to_i32(value).div_euclid(16));
    (-NEAR_CAMERA_RADIUS..=NEAR_CAMERA_RADIUS).flat_map(move |dx| {
        (-NEAR_CAMERA_RADIUS..=NEAR_CAMERA_RADIUS).flat_map(move |dy| {
            (-NEAR_CAMERA_RADIUS..=NEAR_CAMERA_RADIUS).filter_map(move |dz| {
                Some(SubChunkKey::new(
                    dimension,
                    x.checked_add(dx)?,
                    y.checked_add(dy)?,
                    z.checked_add(dz)?,
                ))
            })
        })
    })
}

/// Includes the horizontal light halo supporting the directly prioritized mesh area.
pub(super) fn near_light_columns(
    view: SchedulerView,
    dimension: i32,
) -> impl Iterator<Item = SubChunkKey> {
    let radius = NEAR_CAMERA_RADIUS + 1;
    let [x, y, z] = view
        .position
        .map(|value| floor_to_i32(value).div_euclid(16));
    (-radius..=radius).flat_map(move |dx| {
        (-radius..=radius).filter_map(move |dz| {
            Some(SubChunkKey::new(
                dimension,
                x.checked_add(dx)?,
                y,
                z.checked_add(dz)?,
            ))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Once queue priorities match a stationary camera, local probes add no new information.
    #[test]
    fn stationary_view_does_not_repeat_local_probes() {
        let view = SchedulerView {
            position: [0.0; 3],
            forward: None,
            startup_center: None,
        };
        let mut lanes = [Lane::default()];
        let mut refresh = SchedulerRefresh::<1>::default();
        assert!(refresh.refresh(view, &mut lanes, None, |_, _| true));
        assert!(!refresh.refresh(view, &mut lanes, None, |_, _| true));
    }

    #[test]
    fn camera_refresh_is_bounded_and_preserves_every_current_record() {
        let view = SchedulerView {
            position: [0.0; 3],
            forward: None,
            startup_center: None,
        };
        let count = MAX_PENDING_SCHEDULER_SCANS_PER_POLL * 4;
        let mut lanes = [Lane {
            ready: (0..count)
                .map(|x| {
                    PendingSchedulerCandidate::new(
                        SubChunkKey::new(0, x as i32, 0, 0),
                        1,
                        view,
                        false,
                    )
                })
                .collect(),
            deferred: BinaryHeap::new(),
            ..Lane::default()
        }];
        let mut refresh = SchedulerRefresh::<1>::default();
        refresh.refresh(view, &mut lanes, None, |_, _| true);
        assert_eq!(lanes[0].ready.len(), MAX_PENDING_SCHEDULER_SCANS_PER_POLL);
        for _ in 1..4 {
            refresh.refresh(view, &mut lanes, None, |_, _| true);
        }
        assert_eq!(lanes[0].ready.len(), count);
        assert!(refresh.previous[0].is_empty());
        let turned = SchedulerView {
            position: [16_384.0, 0.0, 0.0],
            forward: None,
            startup_center: None,
        };
        refresh.refresh(turned, &mut lanes, None, |key, _| key.x % 2 == 0);
        assert!(lanes[0].ready.len() <= MAX_PENDING_SCHEDULER_SCANS_PER_POLL);
        for _ in 1..4 {
            refresh.refresh(turned, &mut lanes, None, |key, _| key.x % 2 == 0);
        }
        assert_eq!(lanes[0].ready.len(), count / 2);
    }
    #[test]
    fn closing_loading_restores_camera_order_without_losing_jobs() {
        let mut view = SchedulerView {
            position: [8.0, 80.0, 8.0],
            forward: None,
            startup_center: Some(ChunkKey::new(0, 0, 0)),
        };
        let spawn = SubChunkKey::new(0, 1, 19, 0);
        let halo = SubChunkKey::new(0, 2, 19, 0);
        let distant = SubChunkKey::new(0, 3, 5, 0);
        let mut lanes = [Lane::default()];
        let mut refresh = SchedulerRefresh::<1>::default();
        refresh.refresh(view, &mut lanes, None, |_, _| true);
        for key in [spawn, halo, distant] {
            lanes[0]
                .ready
                .push(PendingSchedulerCandidate::new(key, 1, view, false));
        }
        assert_eq!(lanes[0].ready.peek().unwrap().key, spawn);
        view.startup_center = None;
        assert!(refresh.refresh(view, &mut lanes, None, |_, _| true));
        assert_eq!(lanes[0].ready.pop().unwrap().key, distant);
        assert_eq!(lanes[0].ready.len(), 2);
        assert!(!refresh.refresh(view, &mut lanes, None, |_, _| true));
    }

    /// One ordering serves every lane: transfer footing, then urgency, then spawn class, then camera.
    #[test]
    fn transfer_and_urgency_outrank_startup_class() {
        let view = SchedulerView {
            position: [8.0, 80.0, 8.0],
            forward: None,
            startup_center: Some(ChunkKey::new(0, 0, 0)),
        };
        let candidate = |x, urgent, transfer| {
            let mut candidate =
                PendingSchedulerCandidate::new(SubChunkKey::new(0, x, 5, 0), 1, view, urgent);
            candidate.transfer = transfer;
            candidate
        };
        let mut queue: BinaryHeap<_> = [
            candidate(0, false, false),
            candidate(6, false, false),
            candidate(2, false, false),
            candidate(7, true, false),
            candidate(8, false, true),
        ]
        .into_iter()
        .collect();
        let order: Vec<_> = std::iter::from_fn(|| queue.pop().map(|next| next.key.x)).collect();
        assert_eq!(order, [8, 7, 0, 2, 6]);
    }

    #[test]
    fn startup_order_work_witness() {
        let view = SchedulerView {
            position: [8.0, 80.0, 8.0],
            forward: None,
            startup_center: Some(ChunkKey::new(0, 0, 0)),
        };
        let ordinary = SchedulerView {
            startup_center: None,
            ..view
        };
        let range = vanilla_dimension_range(0).unwrap();
        let keys: Vec<_> = (-8..=8)
            .flat_map(|x| {
                (-8..=8).flat_map(move |z| {
                    (0..range.sub_chunk_count)
                        .map(move |y| SubChunkKey::new(0, x, range.base_sub_chunk_y + y as i32, z))
                })
            })
            .collect();
        let needed = keys
            .iter()
            .filter(|key| view.startup_class(**key) == 0)
            .count();
        let work_until_spawn = |view: SchedulerView| {
            let mut queue: BinaryHeap<_> = keys
                .iter()
                .map(|key| PendingSchedulerCandidate::new(*key, 1, view, false))
                .collect();
            let mut ready = 0;
            let mut work = 0;
            while ready < needed {
                let next = queue.pop().unwrap();
                work += 1;
                if next.key.x.abs_diff(0) <= cohort::STARTUP_RADIUS as u32
                    && next.key.z.abs_diff(0) <= cohort::STARTUP_RADIUS as u32
                {
                    ready += 1;
                }
            }
            work
        };
        let before = work_until_spawn(ordinary);
        let after = work_until_spawn(view);
        eprintln!(
            "join_scheduler_work sections={} before_spawn_complete={before} after_spawn_complete={after}",
            keys.len()
        );
        assert!(before > after);
        assert_eq!(after, needed);
    }

    /// A blocked startup job cooling down costs no allocation however many polls it waits.
    #[test]
    fn cooling_down_startup_job_reuses_scratch_storage() {
        let view = SchedulerView {
            position: [8.0, 80.0, 8.0],
            forward: None,
            startup_center: Some(ChunkKey::new(0, 0, 0)),
        };
        let spawn = PendingSchedulerCandidate::new(SubChunkKey::new(0, 0, 5, 0), 1, view, false);
        let distant = PendingSchedulerCandidate::new(SubChunkKey::new(0, 9, 5, 0), 1, view, false);
        let mut lane = Lane::default();
        lane.ready.push(distant);
        lane.deferred.push(spawn);
        lane.rechecks.insert(
            spawn.key,
            Recheck {
                revision: 1,
                attempts: 6,
                not_before: u64::MAX,
            },
        );
        let mut budget = usize::MAX;
        lane.prioritize_deferred(&mut budget, None, |_, _| true);
        let before = super::super::tests::allocation_count::thread_allocations();
        for _ in 0..64 {
            let mut budget = usize::MAX;
            lane.prioritize_deferred(&mut budget, None, |_, _| true);
        }
        assert_eq!(
            super::super::tests::allocation_count::thread_allocations(),
            before
        );
        assert_eq!(
            lane.deferred.peek().map(|candidate| candidate.key),
            Some(spawn.key)
        );
        assert_eq!(lane.ready.len(), 1);
    }
}
