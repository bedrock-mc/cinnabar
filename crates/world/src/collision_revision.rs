use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use crate::CollisionRevisionError;

#[derive(Debug)]
pub(crate) struct CollisionRevisionAllocator {
    next: AtomicU64,
}

impl CollisionRevisionAllocator {
    pub(crate) const fn with_next(next: u64) -> Self {
        Self {
            next: AtomicU64::new(next),
        }
    }

    pub(crate) fn allocate(&self) -> Result<u64, CollisionRevisionError> {
        Ok(self
            .allocate_batch(1)?
            .into_iter()
            .next()
            .expect("one requested collision revision is returned"))
    }

    pub(crate) fn allocate_batch(&self, count: usize) -> Result<Vec<u64>, CollisionRevisionError> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let count = u64::try_from(count).map_err(|_| CollisionRevisionError::Exhausted)?;
        let mut next = self.next.load(Ordering::Relaxed);
        loop {
            if next == 0 {
                return Err(CollisionRevisionError::Exhausted);
            }
            let last = next
                .checked_add(count - 1)
                .ok_or(CollisionRevisionError::Exhausted)?;
            let replacement = last.checked_add(1).unwrap_or(0);
            match self.next.compare_exchange_weak(
                next,
                replacement,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok((next..=last).collect()),
                Err(observed) => next = observed,
            }
        }
    }
}

static COLLISION_REVISIONS: OnceLock<Arc<CollisionRevisionAllocator>> = OnceLock::new();

pub(crate) fn process_collision_revisions() -> Arc<CollisionRevisionAllocator> {
    Arc::clone(
        COLLISION_REVISIONS.get_or_init(|| Arc::new(CollisionRevisionAllocator::with_next(1))),
    )
}
