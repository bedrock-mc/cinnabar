use super::*;

/// Repeated revisions share one queue position and completed work leaves no retry record.
#[test]
fn ordinary_mesh_retries_coalesce_and_remove_current_revisions() {
    let mut jobs = KeyedJobs::<PendingMesh, u64, 1>::default();
    let count = MAX_PENDING_SCHEDULER_SCANS_PER_POLL * 3;
    let now = Instant::now();
    for revision in 1..=2 {
        for x in 0..count {
            jobs.enqueue(
                SubChunkKey::new(1, x as i32, 0, 0),
                PendingMesh {
                    revision,
                    since: now,
                    queued_at: now,
                    urgent: false,
                },
            );
        }
    }
    let removed = SubChunkKey::new(1, 1, 0, 0);
    jobs.remove_pending(&removed);
    assert_eq!(jobs.retries.begin_poll(), count - 1);
    for x in (0..count).filter(|x| *x != 1) {
        let retry = jobs.retries.pop().unwrap();
        assert_eq!(retry.key.x, x as i32);
        assert_eq!(retry.revision, jobs.pending[&retry.key].revision);
        assert_eq!(retry.revision, 2);
    }
    assert!(jobs.retries.pop().is_none());
    jobs.retries.enqueue(removed, 3);
    jobs.clear_queued();
    assert!(jobs.retries.pop().is_none());
    assert!(jobs.pending.is_empty());
}

/// Cooling retries rotate past untouched work, while fresh arrivals join behind existing waiters.
#[test]
fn cooling_retries_do_not_hide_deferred_work() {
    let mut queue = rechecks::RecheckQueue::default();
    let count = MAX_PENDING_SCHEDULER_SCANS_PER_POLL * 3;
    for x in 0..count {
        queue.enqueue(SubChunkKey::new(0, x as i32, 0, 0), 1);
    }
    for _ in 0..2 {
        let entries = queue.begin_poll();
        for _ in 0..entries {
            let retry = queue.pop().unwrap();
            queue.defer(retry);
        }
    }
    let eligible = SubChunkKey::new(0, count as i32, 0, 0);
    queue.enqueue(eligible, 1);
    let mut offered = None;
    for poll in 0..=count / MAX_PENDING_SCHEDULER_SCANS_PER_POLL {
        let budget = queue.begin_poll().min(MAX_PENDING_SCHEDULER_SCANS_PER_POLL);
        for _ in 0..budget {
            let retry = queue.pop().unwrap();
            if retry.key == eligible {
                offered = Some(retry.key);
                break;
            }
            if queue.is_due(&retry) {
                queue.defer(retry);
            } else {
                queue.requeue(retry);
            }
        }
        if offered.is_some() {
            break;
        }
        queue.enqueue(SubChunkKey::new(0, count as i32 + poll as i32 + 1, 0, 0), 1);
    }
    assert_eq!(offered, Some(eligible));
}

/// A slice that checks one candidate leaves the remaining retries eligible on the next poll.
#[test]
fn unexamined_deferred_rechecks_keep_their_eligibility() {
    let mut queue = rechecks::RecheckQueue::default();
    for x in 0..3 {
        queue.enqueue(SubChunkKey::new(0, x, 0, 0), 1);
    }
    for _ in 0..2 {
        let entries = queue.begin_poll();
        for _ in 0..entries {
            let retry = queue.pop().unwrap();
            queue.defer(retry);
        }
    }
    queue.begin_poll();
    queue.begin_poll();
    let checked = queue.pop().unwrap();
    assert!(queue.is_due(&checked));
    queue.defer(checked);
    queue.begin_poll();
    for x in [1, 2] {
        let unexamined = queue.pop().unwrap();
        assert_eq!(unexamined.key.x, x);
        assert!(queue.is_due(&unexamined));
    }
}

/// Camera reranking preserves the ordinary retry clock and its revision-specific cooldown.
#[test]
fn camera_refresh_preserves_deferred_retry_backoff() {
    let mut jobs = KeyedJobs::<PendingMesh, u64, 1>::default();
    let view = SchedulerView {
        position: [8.0; 3],
        forward: None,
        startup_center: None,
    };
    let key = SubChunkKey::new(0, 12, 0, 0);
    let now = Instant::now();
    jobs.enqueue(
        key,
        PendingMesh {
            revision: 1,
            since: now,
            queued_at: now,
            urgent: false,
        },
    );
    jobs.ingress(view, None, |_, _, _| (0, false));
    for _ in 0..2 {
        jobs.retries.begin_poll();
        let retry = jobs.retries.pop().unwrap();
        jobs.retries.defer(retry);
    }
    jobs.retries.begin_poll();
    let retry = jobs.retries.pop().unwrap();
    assert!(!jobs.retries.is_due(&retry));
    jobs.retries.requeue(retry);
    jobs.refresh.refresh(
        SchedulerView {
            position: [40.0, 8.0, 8.0],
            ..view
        },
        &mut jobs.lanes,
        None,
        |_, _| true,
    );
    let retry = jobs.retries.pop().unwrap();
    assert!(!jobs.retries.is_due(&retry));
    jobs.retries.requeue(retry);
    jobs.retries.begin_poll();
    let retry = jobs.retries.pop().unwrap();
    assert!(jobs.retries.is_due(&retry));
    assert_eq!(retry.revision, 1);
}

/// Replacing ranks and removing interior keys preserves max-priority order with one record per key.
#[test]
fn candidate_queue_replacements_and_removals_match_current_priorities() {
    let view = SchedulerView {
        position: [8.0; 3],
        forward: None,
        startup_center: None,
    };
    let mut queue = CandidateQueue::default();
    let mut expected = HashMap::new();
    for revision in 1..=3 {
        for x in 0..MAX_PENDING_SCHEDULER_SCANS_PER_POLL {
            let key = SubChunkKey::new(0, x as i32, 0, 0);
            let mut candidate = PendingSchedulerCandidate::new(key, revision, view, x % 5 == 0);
            candidate.transfer = (x + revision as usize).is_multiple_of(7);
            queue.push(candidate);
            expected.insert(key, candidate);
        }
        for x in (0..MAX_PENDING_SCHEDULER_SCANS_PER_POLL).step_by(3) {
            let key = SubChunkKey::new(0, x as i32, 0, 0);
            assert_eq!(queue.remove(&key), expected.remove(&key));
            assert!(queue.remove(&key).is_none());
        }
        assert_eq!(queue.len(), expected.len());
    }
    let mut expected: Vec<_> = expected.into_values().collect();
    expected.sort_unstable_by(|left, right| right.cmp(left));
    let actual: Vec<_> = std::iter::from_fn(|| queue.pop()).collect();
    assert_eq!(actual, expected);
    assert!(queue.is_empty());
    queue.clear();
    let candidate = PendingSchedulerCandidate::new(SubChunkKey::new(0, 0, 0, 0), 4, view, false);
    queue.push(candidate);
    assert_eq!(queue.pop(), Some(candidate));
}
