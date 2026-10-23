use super::*;

mod candidates;
pub(super) mod rechecks;

use candidates::CandidateQueue;

pub(super) const NEAR_CAMERA_RADIUS: i32 = 4;
/// Horizontal light halo around the directly prioritized mesh area.
const NEAR_LIGHT_COLUMN_RADIUS: i32 = NEAR_CAMERA_RADIUS + 1;

/// Longest wait, in readiness polls, before a still-blocked revision is rechecked.
pub(in crate::stream) const MAX_RECHECK_BACKOFF_POLLS: u64 = 32;

/// Ready work leads during ordinary streaming; startup also rechecks higher-priority deferred work.
#[derive(Default)]
pub(super) struct Lane {
    pub(super) ready: CandidateQueue,
    pub(super) deferred: CandidateQueue,
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

    fn heap_mut(&mut self, deferred: bool) -> &mut CandidateQueue {
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
    woken: Vec<SubChunkKey>, // nearby work made ready without a new scan record
}

impl<const L: usize> Default for SchedulerRefresh<L> {
    fn default() -> Self {
        Self {
            previous: std::array::from_fn(|_| Lane::default()),
            view: None,
            woken: Vec::new(),
        }
    }
}

impl<const L: usize> SchedulerRefresh<L> {
    /// Offers `key` to the next dispatch when it lies in the camera's neighbourhood.
    pub(super) fn wake_near(&mut self, key: SubChunkKey) {
        if self.view.is_some_and(|view| mesh_is_near(view, key)) {
            self.requeue_woken(key);
        }
    }

    /// Keeps an unexamined key for a later dispatch, even if the camera has moved.
    pub(super) fn requeue_woken(&mut self, key: SubChunkKey) {
        if !self.woken.contains(&key) {
            self.woken.push(key);
        }
    }

    /// Drains the keys [`Self::wake_near`] offered since the last dispatch.
    pub(super) fn take_woken(&mut self) -> Vec<SubChunkKey> {
        std::mem::take(&mut self.woken)
    }

    /// Refreshes bounded queue work and reports whether the view still needs direct local probes.
    pub(super) fn refresh(
        &mut self,
        view: SchedulerView,
        queues: &mut [Lane; L],
        deadline: Option<Instant>,
        is_current: impl Fn(SubChunkKey, u64) -> bool,
    ) -> bool {
        let moved = self.view.is_none_or(|previous| view.reranks_from(previous));
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
    /// Whether pending revisions join the ordinary deferred retry rotation.
    const RECHECK_DEFERRED: bool;

    fn revision(&self) -> u64;
    fn urgent(&self) -> bool;
}

impl PendingJob for PendingLight {
    const RECHECK_DEFERRED: bool = false;

    fn revision(&self) -> u64 {
        self.revision
    }
    fn urgent(&self) -> bool {
        self.urgent
    }
}

impl PendingJob for PendingMesh {
    const RECHECK_DEFERRED: bool = true;

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
    /// Ordinary retries keep their order and cooldown across camera refreshes.
    pub(super) retries: rechecks::RecheckQueue,
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
            retries: rechecks::RecheckQueue::default(),
            in_flight: HashMap::new(),
        }
    }
}

impl<P: PendingJob, J, const L: usize> KeyedJobs<P, J, L> {
    /// Stores work whose caller owns its ready queue, without geometry scans or retries.
    pub(super) fn enqueue_unrouted(&mut self, key: SubChunkKey, pending: P) {
        self.retire_lane_ownership(&key);
        self.retries.remove(&key);
        self.pending.insert(key, pending);
    }

    /// Makes bounded work from an older lane generation available without refreshing other lanes.
    pub(super) fn wake_lane(
        &mut self,
        lane: usize,
        view: SchedulerView,
        budget: usize,
        deadline: Option<Instant>,
    ) {
        for index in 0..budget.min(MAX_PENDING_SCHEDULER_SCANS_PER_POLL) {
            if index != 0 && deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                break;
            }
            let previous = &mut self.refresh.previous[lane];
            let Some(mut candidate) = previous.ready.pop().or_else(|| previous.deferred.pop())
            else {
                break;
            };
            if self
                .pending
                .get(&candidate.key)
                .is_some_and(|pending| pending.revision() == candidate.revision)
            {
                candidate.refresh_rank(view);
                self.lanes[lane].ready.push(candidate);
            }
        }
    }

    /// Startup dependencies enter ingress ahead of ordinary work but behind urgent records.
    pub(super) fn enqueue_prioritized(&mut self, key: SubChunkKey, pending: P, startup: bool) {
        if self
            .pending
            .get(&key)
            .is_none_or(|previous| previous.revision() != pending.revision())
        {
            self.retire_lane_ownership(&key);
        }
        if startup && !pending.urgent() {
            self.startup_scan.push_back((key, pending.revision()));
        } else {
            self.rescan(key, pending.revision(), pending.urgent());
        }
        if P::RECHECK_DEFERRED {
            self.retries.enqueue(key, pending.revision());
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

    /// Removes a pending job and retires its lane ownership and retry history.
    pub(super) fn remove_pending(&mut self, key: &SubChunkKey) -> Option<P> {
        self.retries.remove(key);
        self.retire_lane_ownership(key);
        self.pending.remove(key)
    }

    /// Retires a key from both lane generations without scanning unrelated candidates.
    fn retire_lane_ownership(&mut self, key: &SubChunkKey) {
        for lane in self.lanes.iter_mut().chain(&mut self.refresh.previous) {
            lane.ready.remove(key);
            lane.deferred.remove(key);
            lane.rechecks.remove(key);
        }
    }

    /// Drops queued work; dispatched jobs keep their records.
    pub(super) fn clear_queued(&mut self) {
        self.pending.clear();
        self.scan.clear();
        self.startup_scan.clear();
        self.retries.clear();
        for lane in self.lanes.iter_mut().chain(&mut self.refresh.previous) {
            lane.ready.clear();
            lane.deferred.clear();
            lane.rechecks.clear();
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

/// Whether `key` lies in the neighbourhood [`near_camera_keys`] probes.
pub(super) fn mesh_is_near(view: SchedulerView, key: SubChunkKey) -> bool {
    let [x, y, z] = view.cell();
    let radius = NEAR_CAMERA_RADIUS.unsigned_abs();
    key.x.abs_diff(x) <= radius && key.y.abs_diff(y) <= radius && key.z.abs_diff(z) <= radius
}

/// Whether `key`'s column lies in the halo [`near_light_columns`] probes.
pub(super) fn light_column_is_near(view: SchedulerView, key: SubChunkKey) -> bool {
    let [x, _, z] = view.cell();
    let radius = NEAR_LIGHT_COLUMN_RADIUS.unsigned_abs();
    key.x.abs_diff(x) <= radius && key.z.abs_diff(z) <= radius
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
    let radius = NEAR_LIGHT_COLUMN_RADIUS;
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
            deferred: CandidateQueue::default(),
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

    mod deferred;

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
        let before = test_alloc::thread_allocations();
        for _ in 0..64 {
            let mut budget = usize::MAX;
            lane.prioritize_deferred(&mut budget, None, |_, _| true);
        }
        assert_eq!(test_alloc::thread_allocations(), before);
        assert_eq!(
            lane.deferred.peek().map(|candidate| candidate.key),
            Some(spawn.key)
        );
        assert_eq!(lane.ready.len(), 1);
    }

    fn turn_view(forward: [f32; 3]) -> SchedulerView {
        let length = forward
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        SchedulerView {
            position: [8.0; 3],
            forward: Some(forward.map(|value| value / length)),
            startup_center: None,
        }
    }

    fn yaw(degrees: f32) -> SchedulerView {
        let radians = degrees.to_radians();
        turn_view([radians.cos(), 0.0, radians.sin()])
    }

    /// Runs bounded refresh passes until no re-rank remains in flight.
    fn settle(refresh: &mut SchedulerRefresh<1>, lanes: &mut [Lane; 1], view: SchedulerView) {
        for _ in 0..64 {
            refresh.refresh(view, lanes, None, |_, _| true);
            if refresh.previous[0].is_empty() {
                return;
            }
        }
        panic!("refresh did not settle");
    }

    fn lane_of(keys: &[SubChunkKey], view: SchedulerView, deferred: bool) -> Lane {
        let heap = keys
            .iter()
            .map(|&key| PendingSchedulerCandidate::new(key, 1, view, false))
            .collect();
        if deferred {
            Lane {
                deferred: heap,
                ..Lane::default()
            }
        } else {
            Lane {
                ready: heap,
                ..Lane::default()
            }
        }
    }

    /// 180 sections just ahead of a camera facing +X, and one straight above it.
    fn pitch_witness() -> (Vec<SubChunkKey>, SubChunkKey) {
        let above = SubChunkKey::new(0, 0, 10, 0);
        let ahead = (9..=12)
            .flat_map(|x| {
                (-2..=2).flat_map(move |y| (-4..=4).map(move |z| SubChunkKey::new(0, x, y, z)))
            })
            .chain([above])
            .collect();
        (ahead, above)
    }

    /// Looking up makes the section overhead the nearest in view, though the turn stays in one
    /// compass eighth and the sections ahead outnumber one refresh pass.
    #[test]
    fn a_pitch_only_turn_reranks_queued_sections() {
        let (keys, above) = pitch_witness();
        let level = turn_view([1.0, 0.0, 0.0]);
        let up = turn_view([0.1, 0.995, 0.0]);
        assert!(keys.len() > MAX_PENDING_SCHEDULER_SCANS_PER_POLL);
        let mut lanes = [lane_of(&keys, level, false)];
        let mut refresh = SchedulerRefresh::<1>::default();
        settle(&mut refresh, &mut lanes, level);
        assert_ne!(lanes[0].ready.peek().unwrap().key, above);

        settle(&mut refresh, &mut lanes, up);

        assert_eq!(lanes[0].ready.peek().unwrap().key, above);
        assert_eq!(lanes[0].ready.len(), keys.len());
    }

    /// Deferred work is re-ranked by the same turn, so a later wake dispatches the new favourite.
    #[test]
    fn a_pitch_only_turn_reranks_deferred_sections() {
        let (keys, above) = pitch_witness();
        let level = turn_view([1.0, 0.0, 0.0]);
        let mut lanes = [lane_of(&keys, level, true)];
        let mut refresh = SchedulerRefresh::<1>::default();
        settle(&mut refresh, &mut lanes, level);

        settle(&mut refresh, &mut lanes, turn_view([0.1, 0.995, 0.0]));

        assert_eq!(lanes[0].deferred.peek().unwrap().key, above);
        assert!(lanes[0].ready.is_empty());
    }

    /// A 42° turn inside one compass eighth moves one section into the view cone and another out.
    #[test]
    fn a_yaw_turn_within_one_compass_eighth_reranks() {
        let entering = SubChunkKey::new(0, -6, 0, 1);
        let leaving = SubChunkKey::new(0, 3, 0, 4);
        let (before, after) = (yaw(92.0), yaw(134.0));
        let mut lanes = [lane_of(&[entering, leaving], before, false)];
        let mut refresh = SchedulerRefresh::<1>::default();
        settle(&mut refresh, &mut lanes, before);
        assert_eq!(lanes[0].ready.peek().unwrap().key, leaving);

        settle(&mut refresh, &mut lanes, after);

        assert_eq!(lanes[0].ready.peek().unwrap().key, entering);
    }

    /// Turns below the threshold leave settled queues alone, so mouse jitter costs no re-rank.
    #[test]
    fn small_turns_do_no_refresh_work() {
        let (keys, _) = pitch_witness();
        let mut lanes = [lane_of(&keys, yaw(0.0), false)];
        let mut refresh = SchedulerRefresh::<1>::default();
        settle(&mut refresh, &mut lanes, yaw(0.0));
        for degrees in [3.0, 6.0, 9.0, 3.0] {
            assert!(!refresh.refresh(yaw(degrees), &mut lanes, None, |_, _| true));
            assert!(refresh.previous[0].is_empty());
        }
        assert!(refresh.refresh(yaw(12.0), &mut lanes, None, |_, _| true));
        assert!(!refresh.previous[0].is_empty());
    }

    /// A second turn during a bounded refresh still leaves every record ranked for the newest view.
    #[test]
    fn a_turn_during_an_active_refresh_settles_in_the_newest_order() {
        let keys = (-8..=8)
            .flat_map(|x| (-8..=8).map(move |z| SubChunkKey::new(0, x, (x + z) % 3, z)))
            .collect::<Vec<_>>();
        assert!(keys.len() > 2 * MAX_PENDING_SCHEDULER_SCANS_PER_POLL);
        let mut lanes = [lane_of(&keys, yaw(0.0), false)];
        let mut refresh = SchedulerRefresh::<1>::default();
        settle(&mut refresh, &mut lanes, yaw(0.0));
        refresh.refresh(yaw(90.0), &mut lanes, None, |_, _| true);
        assert!(!refresh.previous[0].is_empty());

        settle(&mut refresh, &mut lanes, yaw(200.0));
        settle(&mut refresh, &mut lanes, yaw(200.0));

        let newest = yaw(200.0);
        let mut ranks = Vec::new();
        while let Some(candidate) = lanes[0].ready.pop() {
            ranks.push(newest.rank(candidate.key));
        }
        assert_eq!(ranks.len(), keys.len());
        assert!(ranks.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    /// Spawn sections keep their startup priority through a turn that favours other sections.
    #[test]
    fn a_turn_keeps_startup_priority() {
        let spawn = SubChunkKey::new(0, 0, -2, 0);
        let ahead = SubChunkKey::new(0, 4, 0, 0);
        let with_startup = |view: SchedulerView| SchedulerView {
            startup_center: Some(ChunkKey::new(0, 0, 0)),
            ..view
        };
        let (level, up) = (
            with_startup(turn_view([1.0, 0.0, 0.0])),
            with_startup(turn_view([0.1, 0.995, 0.0])),
        );
        let mut lanes = [lane_of(&[spawn, ahead], level, false)];
        let mut refresh = SchedulerRefresh::<1>::default();
        settle(&mut refresh, &mut lanes, level);

        settle(&mut refresh, &mut lanes, up);

        assert_eq!(lanes[0].ready.peek().unwrap().key, spawn);
    }
}
