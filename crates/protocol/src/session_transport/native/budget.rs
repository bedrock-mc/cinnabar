//! Byte credit for the original incoming body and its consumer's retained cursor.

use std::{future::Future, num::NonZeroU32, pin::Pin, sync::Arc};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

pub(super) type Reservation = Pin<Box<dyn Future<Output = ByteCredit> + Send>>;

/// One private budget; the watch value counts actual retained bytes, not semaphore weights.
pub(super) struct ByteBudget {
    capacity: usize,
    semaphore: Arc<Semaphore>,
    admitted: watch::Sender<usize>,
    changes: watch::Receiver<usize>,
}

/// Releases bytes only after the owning body or frame cursor has been dropped.
#[derive(Debug)]
pub(crate) struct ByteCredit {
    bytes: usize,
    admitted: watch::Sender<usize>,
    _permit: OwnedSemaphorePermit,
}

impl ByteBudget {
    /// Accepts only the internal nonzero, representable semaphore configuration.
    pub(super) fn new(capacity: usize) -> Self {
        let _ = NonZeroU32::new(u32::try_from(capacity).expect("private byte budget fits u32"))
            .expect("private byte budget is nonzero");
        assert!(
            capacity <= Semaphore::MAX_PERMITS,
            "private byte budget fits the semaphore"
        );
        let (admitted, changes) = watch::channel(0);
        Self {
            capacity,
            semaphore: Arc::new(Semaphore::new(capacity)),
            admitted,
            changes,
        }
    }

    /// Keeps the same fair weighted acquisition alive across unrelated driver wakes.
    pub(super) fn reserve(&self, bytes: usize) -> Reservation {
        let weight = u32::try_from(bytes.min(self.capacity))
            .expect("weight is bounded by the private byte budget");
        let semaphore = Arc::clone(&self.semaphore);
        let admitted = self.admitted.clone();
        Box::pin(async move {
            let permit = semaphore
                .acquire_many_owned(weight)
                .await
                .expect("private byte budget is never closed");
            // Publish the actual charge before exposing the credited body to the driver.
            admitted.send_modify(|total| {
                *total = total
                    .checked_add(bytes)
                    .expect("retained bytes are bounded");
            });
            ByteCredit {
                bytes,
                admitted,
                _permit: permit,
            }
        })
    }

    /// An oversized body occupies the budget alone and prevents another payload read.
    pub(super) fn allows_read(&self) -> bool {
        *self.admitted.borrow() <= self.capacity
    }

    /// Wakes a read-gated driver even when its original service work stays pending.
    pub(super) async fn changed(&mut self) {
        self.changes
            .changed()
            .await
            .expect("the budget retains its watch sender");
    }
}

impl Drop for ByteCredit {
    fn drop(&mut self) {
        // Owners drop body/cursor bytes first. Remove that charge before returning
        // semaphore capacity, so a new credit cannot observe an unreleased old charge.
        self.admitted.send_modify(|total| {
            *total = total
                .checked_sub(self.bytes)
                .expect("each retained body releases its charge once");
        });
    }
}
