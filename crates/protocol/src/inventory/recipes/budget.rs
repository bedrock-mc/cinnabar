use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

/// Requested recipe-owned allocation policy, not an allocator or total-RSS limit.
pub const RECIPE_OWNED_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_UPDATE_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_RECORDS: usize = 8192;

#[derive(Debug)]
pub(super) struct Credits {
    used: AtomicUsize,
    limit: usize,
}

impl Credits {
    #[cfg(test)]
    pub(super) fn isolated(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            used: AtomicUsize::new(0),
            limit,
        })
    }
    #[cfg(test)]
    pub(super) fn used(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }
    pub(super) fn shared() -> Arc<Self> {
        static OWNER: OnceLock<Arc<Credits>> = OnceLock::new();
        Arc::clone(OWNER.get_or_init(|| {
            Arc::new(Self {
                used: AtomicUsize::new(0),
                limit: RECIPE_OWNED_BYTES,
            })
        }))
    }

    pub(super) fn reserve(self: &Arc<Self>, bytes: usize) -> Option<Permit> {
        self.used
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|next| *next <= self.limit)
            })
            .ok()?;
        Some(Permit {
            owner: Arc::clone(self),
            bytes,
        })
    }
}

#[derive(Debug)]
pub(super) struct Permit {
    owner: Arc<Credits>,
    bytes: usize,
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.owner.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surviving_handles_share_credit_and_last_drop_releases_once() {
        let owner = Arc::new(Credits {
            used: AtomicUsize::new(0),
            limit: 12,
        });
        let old = Arc::new(owner.reserve(8).unwrap());
        let retained = Arc::clone(&old);
        drop(old);
        assert!(owner.reserve(5).is_none());
        let replacement = owner.reserve(4).unwrap();
        assert!(owner.reserve(1).is_none());
        drop(retained);
        assert_eq!(owner.used.load(Ordering::Acquire), 4);
        drop(replacement);
        assert_eq!(owner.used.load(Ordering::Acquire), 0);
        assert!(owner.reserve(usize::MAX).is_none());
    }

    #[test]
    fn production_sessions_share_one_process_lifetime_owner() {
        assert!(Arc::ptr_eq(&Credits::shared(), &Credits::shared()));
    }

    #[test]
    fn error_unwinds_a_successful_reservation_once() {
        fn fallible_scope(owner: &Arc<Credits>) -> Result<(), ()> {
            let _permit = owner.reserve(128).ok_or(())?;
            assert_eq!(owner.used(), 128);
            // A private unit witness of Rust error cleanup, not a reachable
            // packet failure or allocator fault-injection mechanism.
            Err(())
        }
        let owner = Credits::isolated(128);
        assert!(fallible_scope(&owner).is_err());
        assert_eq!(owner.used(), 0);
        assert!(owner.reserve(128).is_some());
    }

    #[test]
    fn concurrent_reservations_cannot_oversubscribe() {
        let owner = Credits::isolated(4096);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let owner = Arc::clone(&owner);
                scope.spawn(move || {
                    for _ in 0..1000 {
                        if let Some(permit) = owner.reserve(1024) {
                            assert!(owner.used() <= 4096);
                            std::thread::yield_now();
                            drop(permit);
                        }
                    }
                });
            }
        });
        assert_eq!(owner.used(), 0);
    }
}
