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
fn worker_samples_reuse_named_channel_bindings_and_clip_masks() {
    use super::super::tests::{crowd_with_channel_name, output};

    let count = MIN_PARALLEL_SAMPLES;
    let ids: Vec<_> = (1..=count as u64).collect();
    for unbind_channels in [false, true] {
        let (bound, actors) = crowd_with_channel_name(count as u64, 0, Some("rightitem"));
        let (mut reference, _) = crowd_with_channel_name(count as u64, 0, Some("rightitem"));
        for state in reference.rigs.values_mut() {
            if unbind_channels {
                state.channel_bindings = Arc::new(pose::ChannelBindings::reference());
            } else {
                state.motion_sampling = state
                    .motion_sampling
                    .as_ref()
                    .map(|sampling| sampling.reference_masks());
            }
        }
        let mut reference_scans = 0;
        for alpha in [0.0, 0.25, 0.5, 0.75, 1.0, 0.25] {
            let mut expected =
                ActorRenderFrame::new(&reference, &actors, [0.0; 2], [0.0; 3], alpha);
            let before = pose::name_lookups();
            let expected_bytes = output(&mut expected, &ids);
            reference_scans += pose::name_lookups() - before;
            let mut actual = ActorRenderFrame::new(&bound, &actors, [0.0; 2], [0.0; 3], alpha);
            let slots = std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        let mut slots = samples(count);
                        let before = pose::name_lookups();
                        for sample in &mut slots {
                            sample_one(sample, &actual);
                        }
                        // The counter belongs to the sampling thread, not its caller.
                        assert_eq!(pose::name_lookups(), before);
                        slots
                    })
                    .join()
                    .unwrap()
            });
            for sample in slots {
                actual.remaining_ops -= sample.used;
                actual.sampled_layers.insert(sample.id, sample.layers);
            }
            assert_eq!(output(&mut actual, &ids), expected_bytes);
            assert_eq!(actual.remaining_ops(), expected.remaining_ops());
        }
        assert!(reference_scans > 0, "unbind_channels={unbind_channels}");
        assert_eq!(bound.frame_fallbacks(), 0);
        assert_eq!(reference.frame_fallbacks(), 0);
    }
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
