use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};

/// Creates independent slots for each uniquely claimed actor.
fn samples<'a>(count: usize) -> Vec<Sample<'a>> {
    (1..=count)
        .map(|id| Sample {
            id: id as u64,
            layers: None,
            used: 0,
        })
        .collect()
}

#[test]
fn closed_callbacks_cannot_access_expired_frame_inputs() {
    for panic in [false, true] {
        let board = {
            let (store, actors) = super::super::tests::crowd(8, 0);
            let frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], 0.37);
            let mut slots = samples(8);
            let mut retained = None;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let borrow = Borrow::new(&mut slots, &frame);
                retained = Some(Arc::clone(&borrow.board));
                borrow.board.help();
                assert!(!panic, "controlled caller panic");
            }));
            assert_eq!(result.is_err(), panic);
            assert!(
                slots
                    .iter()
                    .all(|sample| sample.layers.is_some() && sample.used > 0)
            );
            retained.unwrap()
        };
        // All input storage has gone. A late callback must inspect only owned bookkeeping.
        board.help_with(|_, _| panic!("accessed an expired sample"));
        assert!(!board.lock().open);
        assert_eq!(board.lock().busy, 0);
    }
}

#[test]
fn a_panicked_chunk_settles_other_active_borrows_before_returning() {
    let (store, actors) = super::super::tests::crowd(4, 0);
    let frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], 0.37);
    let mut slots = samples(4);
    let borrow = Borrow::new(&mut slots, &frame);
    let board = Arc::clone(&borrow.board);
    let (entered, arrival) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let (closing, close_started) = mpsc::channel();
    let completed = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let active = Arc::clone(&board);
        let completed = &completed;
        scope.spawn(move || {
            active.help_with(|sample, frame| {
                if sample.id == 1 {
                    entered.send(()).unwrap();
                    released.recv().unwrap();
                }
                sample_one(sample, frame);
                if sample.id == CHUNK_SAMPLES as u64 {
                    completed.store(true, Ordering::Release);
                }
            });
        });
        arrival.recv().unwrap();
        board.help_with(|_, _| panic!("controlled claimed panic"));
        assert_eq!(board.lock().busy, 1);
        assert!(board.lock().panic.is_some());
        let closer = scope.spawn(move || {
            closing.send(()).unwrap();
            drop(borrow);
            assert!(completed.load(Ordering::Acquire));
        });
        close_started.recv().unwrap();
        release.send(()).unwrap();
        closer.join().unwrap();
    });
    assert_eq!(board.lock().busy, 0);
    assert!(!board.lock().open);
    assert!(
        slots[..CHUNK_SAMPLES]
            .iter()
            .all(|sample| sample.layers.is_some())
    );
    let panic = board.lock().panic.take().unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            std::panic::resume_unwind(panic);
        }))
        .is_err()
    );
}
