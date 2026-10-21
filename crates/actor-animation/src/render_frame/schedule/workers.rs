//! Caller-owned samples with helpers that may arrive after the frame has finished.
use super::*;
use std::marker::PhantomData;
use std::sync::{
    Condvar, Mutex, MutexGuard, PoisonError,
    atomic::{AtomicUsize, Ordering},
};

/// Small chunks keep eight-body batches useful on both two- and four-thread pools.
const CHUNK_SAMPLES: usize = 2;

/// Includes queued callbacks, so a busy pool cannot accumulate helpers across frames.
static HELPERS: AtomicUsize = AtomicUsize::new(0);

struct Permit;

impl Permit {
    /// Reserves one helper within the current pool's quota, without waiting for capacity.
    fn take(limit: usize) -> Option<Self> {
        HELPERS
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                (count < limit).then_some(count + 1)
            })
            .ok()
            .map(|_| Self)
    }
}

impl Drop for Permit {
    /// Returns capacity when a helper finishes or its queued closure is dropped.
    fn drop(&mut self) {
        HELPERS.fetch_sub(1, Ordering::Relaxed);
    }
}

struct Claims {
    next: usize,
    open: bool,
    busy: usize,
    panic: Option<Box<dyn std::any::Any + Send>>,
}

struct Inputs {
    samples: *mut Sample<'static>,
    frame: *const ActorRenderFrame<'static>,
}

// SAFETY: constructors check the worker trait bounds. Only uniquely claimed chunks access these
// pointers, and Borrow closes claims and waits for active chunks before either borrow can end.
unsafe impl Send for Inputs {}
// SAFETY: chunks never overlap; frame inputs are shared and immutable until Borrow closes.
unsafe impl Sync for Inputs {}

struct Board {
    claims: Mutex<Claims>,
    settled: Condvar,
    inputs: Inputs,
    len: usize,
}

/// Keeps borrowed inputs alive through normal completion and caller-side unwinding.
struct Borrow<'s, 'a> {
    board: Arc<Board>,
    inputs: PhantomData<(&'s mut [Sample<'a>], &'s ActorRenderFrame<'a>)>,
}

impl Drop for Borrow<'_, '_> {
    /// Seals queued callbacks and waits only for chunks that were already claimed.
    fn drop(&mut self) {
        self.board.close();
    }
}

impl<'s, 'a> Borrow<'s, 'a> {
    /// Borrows the inputs until every active chunk settles, even if the caller unwinds.
    fn new(samples: &'s mut [Sample<'a>], frame: &'s ActorRenderFrame<'a>) -> Self {
        // Keep the ordinary worker trait requirements despite the erased pointers.
        let _: &dyn Send = &samples;
        let _: &dyn Sync = frame;
        Self {
            board: Arc::new(Board {
                claims: Mutex::new(Claims {
                    next: 0,
                    open: true,
                    busy: 0,
                    panic: None,
                }),
                settled: Condvar::new(),
                inputs: Inputs {
                    samples: samples.as_mut_ptr().cast(),
                    frame: std::ptr::from_ref(frame).cast(),
                },
                len: samples.len(),
            }),
            inputs: PhantomData,
        }
    }
}

impl Board {
    /// Recovers bookkeeping after a panic so outstanding borrows can still settle.
    fn lock(&self) -> MutexGuard<'_, Claims> {
        self.claims.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Claims disjoint chunks until the run is exhausted or closed.
    fn help(&self) {
        self.help_with(sample_one);
    }

    /// Settles each claim after evaluation and panic cleanup, before releasing its borrow.
    fn help_with(&self, evaluate: impl Fn(&mut Sample<'static>, &ActorRenderFrame<'static>)) {
        loop {
            let chunk = {
                let mut claims = self.lock();
                if !claims.open || claims.next == self.len {
                    return;
                }
                let start = claims.next;
                claims.next = self.len.min(start + CHUNK_SAMPLES);
                claims.busy += 1;
                start..claims.next
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // SAFETY: the claim was acquired while open. Borrow keeps the frame and every
                // sample alive until this chunk finishes, and no other chunk aliases its samples.
                let frame = unsafe { &*self.inputs.frame };
                for index in chunk {
                    // SAFETY: this sample belongs exclusively to the current chunk's claim.
                    let sample = unsafe { &mut *self.inputs.samples.add(index) };
                    evaluate(sample, frame);
                }
            }));
            let mut claims = self.lock();
            claims.busy -= 1;
            if let Err(panic) = result {
                claims.panic.get_or_insert(panic);
                claims.open = false;
            }
            if claims.busy == 0 {
                self.settled.notify_all();
            }
        }
    }

    /// Prevents late helpers from touching inputs and joins only chunks already in progress.
    fn close(&self) {
        let mut claims = self.lock();
        claims.open = false;
        while claims.busy != 0 {
            claims = self
                .settled
                .wait(claims)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// Runs a sample with its reserved allowance, leaving budget publication to the ordered caller.
fn sample_one<'a>(sample: &mut Sample<'a>, frame: &ActorRenderFrame<'a>) {
    let mut left = MAX_MOLANG_OPS_PER_ACTOR_FRAME;
    sample.layers = ActorRenderFrame::sample(
        frame.store,
        frame.actors,
        (frame.camera_rotation, frame.camera_position, frame.alpha),
        sample.id,
        &mut left,
        true,
    );
    sample.used = MAX_MOLANG_OPS_PER_ACTOR_FRAME - left;
}

/// Lets the caller finish all unclaimed work even when every shared worker is occupied.
pub(super) fn sample<'a>(samples: &mut [Sample<'a>], frame: &ActorRenderFrame<'a>) {
    let helpers = rayon::current_num_threads()
        .saturating_sub(1)
        .min(samples.len().div_ceil(CHUNK_SAMPLES).saturating_sub(1));
    let permits: Vec<_> = (0..helpers).map_while(|_| Permit::take(helpers)).collect();
    if permits.is_empty() {
        for sample in samples {
            sample_one(sample, frame);
        }
        return;
    }
    let borrow = Borrow::new(samples, frame);
    let board = Arc::clone(&borrow.board);
    for permit in permits {
        let board = Arc::clone(&board);
        rayon::spawn(move || {
            let _permit = permit;
            board.help();
        });
    }
    board.help();
    drop(borrow);
    let panic = board.lock().panic.take();
    if let Some(panic) = panic {
        std::panic::resume_unwind(panic);
    }
}

#[cfg(test)]
mod tests;
