use super::*;

/// Retired allocations must be released by cooperative turns without any worker threads.
#[test]
fn cooperative_retirement_drops_allocations_without_worker_threads() {
    let pool = WorldPool {
        shared: Arc::default(),
        size: PoolSize {
            foreground: 0,
            background: 0,
        },
    };
    let now = Moment::default();
    let mut scratch = world::LightSolverScratch::default();
    let retired = Arc::new(vec![0_u8; 4096]);
    let witness = Arc::downgrade(&retired);
    pool.retire(move || drop(retired));
    assert!(run_one(&pool.shared, now, &mut scratch));
    assert!(witness.upgrade().is_none());
    assert!(!run_one(&pool.shared, now, &mut scratch));
}

/// Mesh can use every world thread while light stays on the background share.
#[test]
fn pool_sizing_leaves_frame_cores_and_foreground_threads() {
    let sizes = [4, 8, 12].map(PoolSize::for_cores);
    assert_eq!(
        sizes[0],
        PoolSize {
            foreground: 1,
            background: 2
        }
    );
    assert_eq!(
        sizes[1],
        PoolSize {
            foreground: 2,
            background: 4
        }
    );
    assert_eq!(
        sizes[2],
        PoolSize {
            foreground: 4,
            background: 6
        }
    );
    for size in [1, 2, 3, 64].map(PoolSize::for_cores) {
        assert!(size.foreground >= 1);
        assert!(size.background >= super::super::MIN_EFFECTIVE_LIGHT_JOB_CAP);
    }
}

/// Saturating every light worker leaves both latency-sensitive lanes available.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn saturated_lighting_cannot_queue_ahead_of_mesh_or_decode() {
    let pool = WorldPool::new(PoolSize::for_cores(8));
    let (started_tx, started_rx) = crossbeam_channel::unbounded();
    let (release_tx, release_rx) = crossbeam_channel::unbounded();
    // More light jobs than threads: none may run on a foreground thread.
    for _ in 0..pool.size().threads() {
        let started = started_tx.clone();
        let release = release_rx.clone();
        // Surplus jobs may start after the test returns, so they must not panic.
        pool.spawn(Lane::Light, move || {
            let _ = started.send(());
            let _ = release.recv_timeout(Duration::from_secs(5));
        });
    }
    for _ in 0..pool.size().background {
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    assert!(started_rx.recv_timeout(Duration::from_millis(50)).is_err());
    let (done_tx, done_rx) = crossbeam_channel::unbounded();
    let mesh_done = done_tx.clone();
    pool.spawn(Lane::Mesh, move || mesh_done.send(()).unwrap());
    pool.spawn(Lane::Decode, move || done_tx.send(()).unwrap());
    let completed = (0..2).all(|_| done_rx.recv_timeout(Duration::from_secs(2)).is_ok());
    for _ in 0..pool.size().threads() {
        release_tx.send(()).unwrap();
    }
    assert!(completed, "lighting blocked another worker lane");
}

/// Light still runs below frame priority, but no lowered thread ever holds the queue lock the
/// frame thread dispatches through, where a starved holder would stall the frame for seconds.
#[cfg(windows)]
#[test]
fn lowered_workers_never_hold_the_queue_lock() {
    const JOBS: usize = 64;
    let pool = WorldPool::new(PoolSize {
        foreground: 1,
        background: 2,
    });
    let (done_tx, done_rx) = crossbeam_channel::unbounded();
    for _ in 0..JOBS {
        let done = done_tx.clone();
        pool.spawn(Lane::Light, move || {
            done.send(priority::is_lowered()).unwrap();
        });
    }
    let lowered: Vec<bool> = (0..JOBS)
        .map(|_| done_rx.recv_timeout(Duration::from_secs(5)).unwrap())
        .collect();
    assert!(lowered.iter().all(|lowered| *lowered), "{lowered:?}");
    assert_eq!(
        pool.shared
            .lowered_locks
            .load(std::sync::atomic::Ordering::Relaxed),
        0
    );
}

/// Light still solves below frame priority, but publishes its result at normal priority, so the
/// frame thread never waits in `try_recv` on a slot a starved lowered sender reserved.
#[cfg(windows)]
#[test]
fn lowered_workers_publish_results_at_normal_priority() {
    const JOBS: usize = 64;
    let pool = WorldPool::new(PoolSize {
        foreground: 1,
        background: 2,
    });
    let (result_tx, result_rx) = crossbeam_channel::bounded(JOBS);
    let (done_tx, done_rx) = crossbeam_channel::unbounded();
    for _ in 0..JOBS {
        let result = result_tx.clone();
        let done = done_tx.clone();
        pool.spawn(Lane::Light, move || {
            let solved_lowered = priority::is_lowered();
            send_result(&result, ());
            done.send((solved_lowered, priority::is_lowered())).unwrap();
        });
    }
    for _ in 0..JOBS {
        let (solved_lowered, published_lowered) =
            done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(solved_lowered, "light solves below frame priority");
        assert!(!published_lowered, "results publish at normal priority");
        result_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}

/// Pushes `queued` lanes at `queued_at` and drains them at `now` in worker order.
fn take_order(background: bool, queued: &[Lane], queued_at: Moment, now: Moment) -> Vec<Lane> {
    let mut queues = Queues::default();
    let (order_tx, order_rx) = crossbeam_channel::unbounded();
    for &lane in queued {
        let order = order_tx.clone();
        queues.push(
            lane,
            queued_at,
            Box::new(move |_| order.send(lane).unwrap()),
        );
    }
    while let Some(job) = queues.take(background, now) {
        job(&mut world::LightSolverScratch::default());
    }
    order_rx.try_iter().collect()
}

/// Fresh work runs mesh, then decode, then light; foreground workers never take light.
#[test]
fn fresh_work_follows_lane_priority() {
    let queued = [
        Lane::Summary,
        Lane::Light,
        Lane::Decode,
        Lane::Mesh,
        Lane::Mesh,
    ];
    let at = Moment::default();
    assert_eq!(
        take_order(true, &queued, at, at),
        [
            Lane::Mesh,
            Lane::Mesh,
            Lane::Decode,
            Lane::Light,
            Lane::Summary
        ]
    );
    assert_eq!(
        take_order(false, &queued, at, at),
        [Lane::Mesh, Lane::Mesh, Lane::Decode]
    );
}

/// Overdue light and decode jump a mesh flood; overdue light still never runs foreground.
#[test]
fn overdue_lower_lanes_jump_a_mesh_flood() {
    let queued = [
        Lane::Summary,
        Lane::Light,
        Lane::Decode,
        Lane::Mesh,
        Lane::Mesh,
    ];
    let at = Moment::default();
    let decode_late = at.checked_add(DECODE_MAX_WAIT).unwrap();
    assert_eq!(
        take_order(true, &queued, at, decode_late),
        [
            Lane::Decode,
            Lane::Mesh,
            Lane::Mesh,
            Lane::Light,
            Lane::Summary
        ]
    );
    let light_late = at.checked_add(LIGHT_MAX_WAIT).unwrap();
    assert_eq!(
        take_order(true, &queued, at, light_late),
        [
            Lane::Light,
            Lane::Decode,
            Lane::Mesh,
            Lane::Mesh,
            Lane::Summary
        ]
    );
    assert_eq!(
        take_order(false, &queued, at, light_late),
        [Lane::Decode, Lane::Mesh, Lane::Mesh]
    );
}

/// A batch becomes visible together while each job retains the pool's lane ordering.
#[test]
fn dispatch_batch_publishes_independent_jobs_together() {
    let pool = WorldPool {
        shared: Arc::default(),
        size: PoolSize {
            foreground: 0,
            background: 0,
        },
    };
    let (done, completed) = crossbeam_channel::unbounded();
    let mut batch = pool.batch(Lane::Mesh);
    for index in 0..16 {
        let done = done.clone();
        batch.spawn(false, move || done.send(index).unwrap());
    }
    assert!(pool.shared.lock().mesh.is_empty());
    drop(batch);
    let mut queues = pool.shared.lock();
    assert_eq!(queues.mesh.len(), 16);
    let mut scratch = world::LightSolverScratch::default();
    while let Some(job) = queues.take(false, Moment::default()) {
        job(&mut scratch);
    }
    assert_eq!(
        completed.try_iter().collect::<Vec<_>>(),
        (0..16).collect::<Vec<_>>()
    );
}

/// Work on the borrowed world cores runs as wide as the world pool, below frame priority, so a
/// join's compile cannot crowd out the main and render threads.
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
#[test]
fn borrowed_world_cores_run_as_wide_as_the_world_pool_at_normal_priority() {
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let (threads, lowered) = on_idle_world_cores(|| {
        (
            rayon::current_num_threads(),
            rayon::broadcast(|_| priority::is_lowered()),
        )
    });
    assert_eq!(threads, PoolSize::for_cores(cores).threads());
    // A join compile must not wait behind unrelated background processes.
    assert!(lowered.iter().all(|lowered| !*lowered), "{lowered:?}");
}

/// Draining returns immediately on an empty queue and executes only one accepted job.
#[test]
fn local_driver_runs_one_job_and_reuses_the_native_queue_order() {
    let shared = Shared::default();
    let now = Moment::default();
    let mut scratch = world::LightSolverScratch::default();
    assert!(!run_one(&shared, now, &mut scratch));
    let (sent, received) = crossbeam_channel::bounded(3);
    for lane in [Lane::Summary, Lane::Light, Lane::Decode, Lane::Mesh] {
        let sent = sent.clone();
        shared
            .lock()
            .push(lane, now, Box::new(move |_| sent.try_send(lane).unwrap()));
    }
    for expected in [Lane::Mesh, Lane::Decode, Lane::Light, Lane::Summary] {
        assert!(run_one(&shared, now, &mut scratch));
        assert_eq!(received.try_recv().unwrap(), expected);
        assert!(received.try_recv().is_err());
    }
    assert!(!run_one(&shared, now, &mut scratch));
}

/// Ready meshes retain a foreground turn while overdue lighting remains queued.
#[test]
fn local_driver_does_not_starve_meshes_behind_overdue_lighting() {
    let shared = Shared::default();
    let queued = Moment::default();
    let now = queued.checked_add(LIGHT_MAX_WAIT).unwrap();
    let (sent, received) = crossbeam_channel::unbounded();
    for lane in [Lane::Light, Lane::Light, Lane::Light, Lane::Mesh] {
        let sent = sent.clone();
        shared
            .lock()
            .push(lane, queued, Box::new(move |_| sent.send(lane).unwrap()));
    }
    let mut scratch = world::LightSolverScratch::default();
    for _ in 0..2 {
        assert!(run_one(&shared, now, &mut scratch));
    }
    assert_eq!(
        received.try_iter().collect::<Vec<_>>(),
        [Lane::Light, Lane::Mesh]
    );
    for _ in 0..2 {
        assert!(run_one(&shared, now, &mut scratch));
        assert_eq!(received.try_recv().unwrap(), Lane::Light);
    }
    assert!(!run_one(&shared, now, &mut scratch));
}

/// Summary age never promotes it above urgent or near work; foreground leaves it queued.
#[test]
fn summaries_never_jump_urgent_or_near_work() {
    let mut queues = Queues::default();
    let (tx, rx) = crossbeam_channel::unbounded();
    let oldest = Moment::default();
    for lane in [Lane::Summary, Lane::Light, Lane::Decode, Lane::Mesh] {
        let tx = tx.clone();
        queues.push(lane, oldest, Box::new(move |_| tx.send(lane).unwrap()));
    }
    let urgent = tx.clone();
    queues.urgent.push_back((
        Lane::Mesh,
        Box::new(move |_| urgent.send(Lane::Mesh).unwrap()),
    ));
    let now = oldest.checked_add(Duration::from_secs(3600)).unwrap();
    let mut scratch = world::LightSolverScratch::default();
    while let Some(job) = queues.take(false, now) {
        job(&mut scratch);
    }
    assert_eq!(
        rx.try_iter().collect::<Vec<_>>(),
        [Lane::Mesh, Lane::Decode, Lane::Mesh]
    );
    assert_eq!(queues.summary.len(), 1);
    while let Some(job) = queues.take(true, now) {
        job(&mut scratch);
    }
    assert_eq!(
        rx.try_iter().collect::<Vec<_>>(),
        [Lane::Light, Lane::Summary]
    );
}

/// A summary submission wakes the eligible background waiter even with a foreground waiter.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn summary_submission_wakes_background_waiters() {
    let pool = WorldPool {
        shared: Arc::default(),
        size: PoolSize {
            foreground: 0,
            background: 0,
        },
    };
    let (waiting, entered) = crossbeam_channel::unbounded();
    let (done, completed) = crossbeam_channel::unbounded();
    std::thread::scope(|scope| {
        for background in [false, true] {
            let shared = &pool.shared;
            let waiting = waiting.clone();
            scope.spawn(move || {
                let mut queues = shared.lock();
                waiting.send(()).unwrap();
                loop {
                    if queues.shutdown {
                        break;
                    }
                    if let Some(job) = queues.take(background, Moment::default()) {
                        drop(queues);
                        job(&mut world::LightSolverScratch::default());
                        queues = shared.lock();
                    } else {
                        queues = shared.wait(queues);
                    }
                }
            });
        }
        entered.recv().unwrap();
        entered.recv().unwrap();
        pool.spawn(Lane::Summary, move || done.send(()).unwrap());
        completed.recv().unwrap();
        pool.shared.lock().shutdown = true;
        pool.shared.ready.notify_all();
    });
}

/// A core cap only ever shrinks the pools; malformed or zero caps keep the machine's count.
#[test]
fn core_cap_shrinks_pools_and_ignores_invalid_values() {
    assert_eq!(capped_cores(12, None), 12);
    assert_eq!(capped_cores(12, Some("2")), 2);
    assert_eq!(capped_cores(12, Some(" 4 ")), 4);
    assert_eq!(capped_cores(4, Some("16")), 4);
    assert_eq!(capped_cores(12, Some("0")), 12);
    assert_eq!(capped_cores(12, Some("two")), 12);
}
