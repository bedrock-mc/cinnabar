use super::*;
use std::sync::{Condvar, Mutex, mpsc};

/// Holds every shared worker until dropped, including when the test unwinds.
struct BusyPool(Arc<(Mutex<bool>, Condvar)>);

impl BusyPool {
    /// Waits until every worker has entered its blocking callback.
    fn new() -> Self {
        let pool = Self(Arc::new((Mutex::new(false), Condvar::new())));
        let workers = rayon::current_num_threads();
        let (entered, arrivals) = mpsc::channel();
        for _ in 0..workers {
            let gate = Arc::clone(&pool.0);
            let entered = entered.clone();
            rayon::spawn(move || {
                let (lock, ready) = &*gate;
                let mut released = lock.lock().unwrap();
                let _ = entered.send(());
                while !*released {
                    released = ready.wait(released).unwrap();
                }
            });
        }
        let mut count = 0;
        test_time::eventually("every shared worker blocked", || {
            count += arrivals.try_iter().count();
            count == workers
        });
        pool
    }
}

impl Drop for BusyPool {
    /// Releases background callbacks before a scoped sampling thread must be joined.
    fn drop(&mut self) {
        let (lock, ready) = &*self.0;
        *lock.lock().unwrap() = true;
        ready.notify_all();
    }
}

#[test]
fn sampling_completes_while_the_shared_pool_is_saturated() {
    let (serial, actors) = crowd(MIN_PARALLEL_SAMPLES as u64, 0);
    let (parallel, _) = crowd(MIN_PARALLEL_SAMPLES as u64, 0);
    let ids: Vec<_> = (1..=MIN_PARALLEL_SAMPLES as u64).collect();
    let mut expected = ActorRenderFrame::new(&serial, &actors, [0.0; 2], [0.0; 3], 0.37);
    let bytes = output(&mut expected, &ids);
    let remaining = expected.remaining_ops();
    std::thread::scope(|scope| {
        let busy = BusyPool::new();
        let sampling = scope.spawn(|| {
            assert!(rayon::current_thread_index().is_none());
            let mut frame = ActorRenderFrame::new(&parallel, &actors, [0.0; 2], [0.0; 3], 0.37);
            frame.sample_ordered(&ids);
            let first = (output(&mut frame, &ids), frame.remaining_ops());
            drop(frame);
            for _ in 0..16 {
                let mut frame = ActorRenderFrame::new(&parallel, &actors, [0.0; 2], [0.0; 3], 0.37);
                frame.sample_ordered(&ids);
                assert_eq!(output(&mut frame, &ids), first.0);
            }
            first
        });
        test_time::eventually("frame sampling before blocked workers are released", || {
            sampling.is_finished()
        });
        drop(busy);
        assert_eq!(sampling.join().unwrap(), (bytes, remaining));
    });
    assert_eq!(parallel.frame_fallbacks(), serial.frame_fallbacks());
}
