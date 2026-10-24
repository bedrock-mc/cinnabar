//! Explicit resource admission; a failed admission means defer/evict, never truncate a world.

use std::sync::{Arc, Mutex};

pub const MIB: usize = 1024 * 1024;

/// Initial application budgets, subject to real device qualification. These are not
/// a claim that all physical RAM is available to the process. CDRAM is a separate pool.
pub const DEFAULT_MAIN_LIMIT: usize = 160 * MIB;
pub const DEFAULT_CDRAM_LIMIT: usize = 96 * MIB;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pool {
    Main,
    Cdram,
}

impl Pool {
    const fn index(self) -> usize {
        match self {
            Self::Main => 0,
            Self::Cdram => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pressure {
    pub pool: Pool,
    pub requested: usize,
    pub available: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Usage {
    pub used: [usize; 2],
    pub peak: [usize; 2],
    pub limit: [usize; 2],
}

#[derive(Clone, Debug)]
pub struct Budget(Arc<Mutex<Usage>>);

impl Default for Budget {
    fn default() -> Self {
        Self::new(DEFAULT_MAIN_LIMIT, DEFAULT_CDRAM_LIMIT)
    }
}

impl Budget {
    pub fn new(main: usize, cdram: usize) -> Self {
        Self(Arc::new(Mutex::new(Usage {
            used: [0; 2],
            peak: [0; 2],
            limit: [main, cdram],
        })))
    }

    pub fn usage(&self) -> Usage {
        *self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Reserve before allocating. Keep the lease until CPU ownership AND GPU
    /// use have ended. Moving it into a fence retirement queue retains accounting.
    pub fn reserve(&self, pool: Pool, bytes: usize) -> Result<Lease, Pressure> {
        let mut usage = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let index = pool.index();
        let available = usage.limit[index] - usage.used[index];
        if bytes > available {
            return Err(Pressure {
                pool,
                requested: bytes,
                available,
            });
        }
        usage.used[index] += bytes;
        usage.peak[index] = usage.peak[index].max(usage.used[index]);
        Ok(Lease {
            budget: self.clone(),
            pool,
            bytes,
        })
    }
}

/// Unique ownership of an admitted allocation. Never clone a lease for a second buffer.
#[derive(Debug)]
pub struct Lease {
    budget: Budget,
    pool: Pool,
    bytes: usize,
}

impl Lease {
    pub const fn bytes(&self) -> usize {
        self.bytes
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let mut usage = self
            .budget
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        usage.used[self.pool.index()] -= self.bytes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_upload_retains_capacity_until_completion() {
        let budget = Budget::new(64, 32);
        let upload = budget.reserve(Pool::Main, 48).unwrap();
        let submitted = vec![upload];
        assert_eq!(budget.reserve(Pool::Main, 17).unwrap_err().available, 16);
        drop(submitted);
        let replacement = budget.reserve(Pool::Main, 64).unwrap();
        assert_eq!(budget.usage().used, [64, 0]);
        drop(replacement);
        assert_eq!(budget.usage().used, [0, 0]);
        assert_eq!(budget.usage().peak, [64, 0]);
    }

    #[test]
    fn pools_do_not_borrow_from_each_other_or_overflow() {
        let budget = Budget::new(64, 32);
        let _main = budget.reserve(Pool::Main, 64).unwrap();
        let _graphics = budget.reserve(Pool::Cdram, 32).unwrap();
        assert!(budget.reserve(Pool::Main, usize::MAX).is_err());
        assert!(budget.reserve(Pool::Cdram, 1).is_err());
        assert_eq!(budget.usage().used, [64, 32]);
    }
}
