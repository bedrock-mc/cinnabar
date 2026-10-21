//! Image job queues. Native hosts keep their dedicated workers; browser hosts drain one job
//! after yielding and collect its result through the atlas's existing completion channels.

#[cfg(target_arch = "wasm32")]
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;

/// An image read or decode that owns its cancellation and completion channels.
pub(crate) trait ImageJob: Send + 'static {
    /// Runs one finite job and publishes its result unless its pack has been cancelled.
    fn run(self);
}

/// One running job and one waiting input per image work class.
pub(crate) struct Worker<J> {
    sender: Sender<J>,
    #[cfg(target_arch = "wasm32")]
    receiver: Receiver<J>,
}

impl<J: ImageJob> Worker<J> {
    /// Starts a native worker, or retains the bounded queue for the browser's job driver.
    pub(crate) fn new(name: &str) -> Option<Self> {
        let (sender, receiver) = crossbeam_channel::bounded::<J>(1);
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::thread::Builder::new()
                .name(name.into())
                .spawn(move || {
                    while let Ok(job) = receiver.recv() {
                        job.run();
                    }
                })
                .ok()?;
            Some(Self { sender })
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = name;
            Some(Self { sender, receiver })
        }
    }

    /// The bounded input channel used by each pack's pending/retry state.
    pub(crate) fn sender(&self) -> &Sender<J> {
        &self.sender
    }

    /// Runs at most one queued job; the caller must yield before calling again.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn run_one(&self) -> bool {
        let Ok(job) = self.receiver.try_recv() else {
            return false;
        };
        job.run();
        true
    }
}

/// Browser-owned driver for the existing image queues, with no synchronous spawn fallback.
#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub struct ImageJobDriver {
    next: usize,
}

#[cfg(target_arch = "wasm32")]
impl ImageJobDriver {
    /// Runs one finite read or decode after the host has yielded. Indivisible job cost still
    /// needs a browser frame-budget check; this method does not make CPU work preemptible.
    pub fn run_one(&mut self) -> bool {
        let workers: &[fn() -> bool] = &[
            crate::atlas::native_sheets::run_one,
            crate::atlas::source_reads::run_one,
            crate::atlas::fitted::run_one,
            crate::atlas::outlines::run_one,
        ];
        for offset in 0..workers.len() {
            let index = (self.next + offset) % workers.len();
            if workers[index]() {
                self.next = (index + 1) % workers.len();
                return true;
            }
        }
        false
    }
}
