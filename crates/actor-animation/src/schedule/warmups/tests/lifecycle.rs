use super::*;
use std::sync::{
    Barrier,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};

/// A queued callback may outlive the borrowed slice, but a closed board cannot touch that slice.
#[test]
fn busy_pool_helpers_start_after_the_caller_has_finished_and_closed_the_board() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    let release = Arc::new(Barrier::new(3));
    let (started, entered) = mpsc::channel();
    for _ in 0..2 {
        let started = started.clone();
        let release = Arc::clone(&release);
        pool.spawn(move || {
            started.send(()).unwrap();
            release.wait();
        });
    }
    entered.recv().unwrap();
    entered.recv().unwrap();
    let (done, finished) = mpsc::channel();
    let accessed = Arc::new(AtomicUsize::new(0));
    let board = with_jobs(clock_assets(None), RIGS_PER_HELPER, |jobs, catalogs| {
        for job in jobs.iter_mut() {
            job.warm_candidate = true;
        }
        let board = Arc::new(Board::new(jobs, catalogs, Pass::PreviousWarmup));
        let late = Arc::clone(&board);
        let accessed = Arc::clone(&accessed);
        pool.spawn(move || {
            late.help_with(|_| {
                accessed.fetch_add(1, Ordering::SeqCst);
            });
            done.send(()).unwrap();
        });
        board.help();
        board.settle();
        assert!(jobs.iter().all(|job| job.outcome.is_some()));
        assert_eq!(board.lock().busy, 0);
        board
    });
    // The rig store and its jobs have now been dropped. Both occupied workers can resume.
    release.wait();
    finished.recv().unwrap();
    assert_eq!(accessed.load(Ordering::SeqCst), 0);
    board.help_with(|_| panic!("a closed callback accessed expired jobs"));
}

/// Panic cleanup restores transient inputs while preserving direct evaluator effects.
#[test]
fn warm_guard_restores_history_and_context_when_evaluation_panics() {
    with_jobs(clock_assets(None), 1, |jobs, _| {
        let job = &mut jobs[0];
        let history = format!("{:?}", job.state.history);
        let length = job.state.history.len();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let guard = WarmContext::new(job);
            assert_eq!(guard.job.state.history.len(), length - 1);
            assert_eq!(guard.job.context.animation_elapsed_ticks, Some(3));
            guard.job.state.ui_pose = Some(Vec::new());
            panic!("controlled evaluator panic");
        }));
        assert!(result.is_err());
        assert_eq!(format!("{:?}", job.state.history), history);
        assert_eq!(job.context.animation_elapsed_ticks, Some(4));
        assert!(
            job.state.ui_pose.is_some(),
            "direct side effects are not rolled back"
        );
    });
}

/// A panic cannot escape the board while another claimed evaluator still owns a rig borrow.
#[test]
fn a_panicked_claim_settles_its_guard_and_waits_for_the_other_claim() {
    with_jobs(clock_assets(None), 2, |jobs, catalogs| {
        let before: Vec<_> = jobs
            .iter()
            .map(|job| format!("{:?}", job.state.history))
            .collect();
        let board = Arc::new(Board::new(jobs, catalogs, Pass::PreviousWarmup));
        let restored = AtomicBool::new(false);
        let (held, holding) = mpsc::channel();
        let (release, released) = mpsc::channel();
        std::thread::scope(|scope| {
            let first = Arc::clone(&board);
            let (claimed, claim_seen) = mpsc::channel();
            let (panic_now, panic_gate) = mpsc::channel();
            let first_worker = scope.spawn(move || {
                first.help_with(|job| {
                    assert_eq!(job.lifetime.runtime_id, 1);
                    let _guard = WarmContext::new(job);
                    claimed.send(()).unwrap();
                    panic_gate.recv().unwrap();
                    panic!("controlled claimed panic");
                })
            });
            claim_seen.recv().unwrap();
            let second = Arc::clone(&board);
            let restored = &restored;
            scope.spawn(move || {
                second.help_with(|job| {
                    assert_eq!(job.lifetime.runtime_id, 2);
                    {
                        let _guard = WarmContext::new(job);
                        held.send(()).unwrap();
                        released.recv().unwrap();
                    }
                    restored.store(true, Ordering::SeqCst);
                })
            });
            holding.recv().unwrap();
            panic_now.send(()).unwrap();
            first_worker.join().unwrap();
            {
                let claims = board.lock();
                assert_eq!(claims.busy, 1);
                assert!(claims.panic.is_some());
            }
            assert!(!restored.load(Ordering::SeqCst));
            release.send(()).unwrap();
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| board.settle()));
            assert!(panic.is_err());
            assert!(restored.load(Ordering::SeqCst));
            assert_eq!(board.lock().busy, 0);
        });
        for (job, history) in jobs.iter().zip(before) {
            assert_eq!(format!("{:?}", job.state.history), history);
            assert_eq!(job.context.animation_elapsed_ticks, Some(4));
        }
        board.help_with(|_| panic!("settled panic board accessed jobs"));
    });
}
